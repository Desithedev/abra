use crate::dock::menu::{Item, Menu, MENU_CLOSE, MENU_PICK};
use crate::dock::pins;
use crate::popup::Popup;
use crate::prefs;
use nacre_core::search;
use nacre_platform::apps::{self, App};
use nacre_platform::icons::{self, Bitmap};
use nacre_platform::text::{measure, render as render_text, render_face, Style, ICON_FACE};
use nacre_platform::{input, launch, monitors, sys};
use nacre_ui::gfx::Gfx;
use nacre_ui::glass::Tier;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use windows::core::w;
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionDrawingSurface, CompositionEasingFunction, CompositionStretch, Compositor,
    ContainerVisual,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT,
    VK_RETURN, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::HSTRING;
use windows_numerics::{Vector2, Vector3};

const WM_DISMISS: u32 = WM_APP + 150;
const WM_ICONS_READY: u32 = WM_APP + 151;
const WM_MOUSELEAVE: u32 = 0x02A3;
const T_HIDE: usize = 1;

const COLS: usize = 6;
const VISIBLE_ROWS: usize = 3;

const CMD_SLEEP: usize = 1;
const CMD_RESTART: usize = 2;
const CMD_SHUTDOWN: usize = 3;
const CMD_LOCK: usize = 4;

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    ToggleAll,
    App(usize),
    User,
    Settings,
    Power,
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

struct StartMenu {
    comp: Compositor,
    gfx: Gfx,
    popup: Popup,
    power_menu: Option<Menu>,
    dpi: f32,
    size: (f32, f32),
    win_rect: (i32, i32, i32, i32),
    pop_at: Vector3,
    closing: bool,
    all_apps: bool,
    query: String,
    apps: Vec<App>,
    names: Vec<String>,
    pinned: Vec<App>,
    shown: Vec<App>,
    scroll: usize,
    sel: Option<usize>,
    hover: Option<Hit>,
    hits: Vec<(Rect, Hit)>,
    tracking: bool,
    text_cache: HashMap<String, (CompositionDrawingSurface, i32, i32)>,
    icon_cache: HashMap<String, Option<CompositionDrawingSurface>>,
}

static ICON_CACHE: Mutex<Option<HashMap<String, Option<Bitmap>>>> = Mutex::new(None);
static ICON_TX: Mutex<Option<Sender<(String, String, i32, bool, nacre_core::settings::IconStyle, u8, bool, isize)>>> =
    Mutex::new(None);
static ICON_ASKED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

thread_local! {
    static MENU: RefCell<Option<StartMenu>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut StartMenu) -> R) -> Option<R> {
    MENU.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

fn ensure(comp: &Compositor) -> bool {
    if MENU.with(|c| c.borrow().is_some()) {
        return true;
    }
    let Ok(gfx) = Gfx::shared(comp) else {
        return false;
    };
    let glass = prefs::glass((10.0, 10.0), 20.0, prefs::Role::Themed);
    let Ok(popup) = Popup::new(comp, w!("AbraStartMenu"), Some(proc), WINDOW_EX_STYLE(0), glass)
    else {
        return false;
    };
    let tier = {
        let c = sys::conditions();
        if !c.transparency || c.battery_saver || c.remote_session {
            Tier::Solid
        } else {
            Tier::Full
        }
    };
    let _ = popup.panel.set_tier(tier);
    prefs::listen(popup.hwnd);
    MENU.with(|c| {
        *c.borrow_mut() = Some(StartMenu {
            comp: comp.clone(),
            gfx,
            popup,
            power_menu: None,
            dpi: 1.0,
            size: (540.0, 540.0),
            win_rect: (0, 0, 0, 0),
            pop_at: Vector3 {
                X: 270.0,
                Y: 540.0,
                Z: 0.0,
            },
            closing: false,
            all_apps: false,
            query: String::new(),
            apps: Vec::new(),
            names: Vec::new(),
            pinned: Vec::new(),
            shown: Vec::new(),
            scroll: 0,
            sel: None,
            hover: None,
            hits: Vec::new(),
            tracking: false,
            text_cache: HashMap::new(),
            icon_cache: HashMap::new(),
        });
    });
    true
}

pub fn is_open() -> bool {
    MENU.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|m| m.popup.is_visible() && !m.closing)
    })
}

pub fn close() {
    with(|m| m.close_animated());
}

pub fn toggle(comp: &Compositor, anchor_x: i32, bottom_y: i32, dpi: f32) {
    if !ensure(comp) {
        return;
    }
    with(|m| {
        if m.popup.is_visible() && !m.closing {
            m.close_animated();
            return;
        }
        m.open(anchor_x, bottom_y, dpi);
    });
}

fn user_name() -> String {
    std::env::var("USERNAME")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "ABRA".to_string())
}

fn build_pinned(apps: &[App]) -> Vec<App> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let pins_file = crate::paths::pins();
    for pin in pins::load(&pins_file) {
        let p_low = pin.path.to_ascii_lowercase();
        if let Some(found) = apps.iter().find(|a| a.path.to_ascii_lowercase() == p_low) {
            if seen.insert(found.path.to_ascii_lowercase()) {
                out.push(found.clone());
            }
        } else if !pin.path.is_empty()
            && (p_low.starts_with("shell:") || std::path::Path::new(&pin.path).exists())
        {
            let name = std::path::Path::new(&pin.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("App")
                .to_string();
            if seen.insert(pin.path.to_ascii_lowercase()) {
                out.push(App { name, path: pin.path });
            }
        }
        if out.len() >= COLS * VISIBLE_ROWS {
            return out;
        }
    }

    let essentials = [
        "edge",
        "zen",
        "chrome",
        "firefox",
        "explorer",
        "file explorer",
        "settings",
        "cài đặt",
        "store",
        "microsoft store",
        "terminal",
        "powershell",
        "calculator",
        "notepad",
        "photos",
        "snipping",
        "paint",
        "task manager",
        "outlook",
    ];
    for kw in essentials {
        if let Some(found) = apps
            .iter()
            .find(|a| a.name.to_ascii_lowercase().contains(kw))
        {
            if seen.insert(found.path.to_ascii_lowercase()) {
                out.push(found.clone());
            }
            if out.len() >= COLS * VISIBLE_ROWS {
                return out;
            }
        }
    }

    for a in apps {
        if seen.insert(a.path.to_ascii_lowercase()) {
            out.push(a.clone());
        }
        if out.len() >= COLS * VISIBLE_ROWS {
            break;
        }
    }
    out
}

impl StartMenu {
    fn open(&mut self, anchor_x: i32, bottom_y: i32, dpi: f32) {
        if self.closing {
            self.closing = false;
            unsafe {
                let _ = KillTimer(Some(self.popup.hwnd), T_HIDE);
            }
            self.popup.hide();
        }
        self.dpi = dpi;
        self.all_apps = false;
        self.query.clear();
        self.scroll = 0;
        self.sel = None;
        self.hover = None;

        self.apps = apps::snapshot();
        self.names = self.apps.iter().map(|a| a.name.clone()).collect();
        self.pinned = build_pinned(&self.apps);
        self.rebuild_shown();

        let mon = monitors::at(anchor_x, bottom_y);
        let work = mon.work;
        let max_w = (work.size().0 as f32 - 24.0 * dpi).max(360.0);
        let max_h = (bottom_y - work.y - 16).max(320) as f32;
        let w = (540.0 * dpi).min(max_w).round();
        let h = (536.0 * dpi).min(max_h).round();
        self.size = (w, h);

        let wi = w as i32;
        let hi = h as i32;
        let x = work.clamp_x(anchor_x - wi / 2, wi, 8);
        let y = work.clamp_y(bottom_y - hi, hi, 8);
        self.win_rect = (x, y, x + wi, y + hi);
        self.pop_at = Vector3 {
            X: w / 2.0,
            Y: h,
            Z: 0.0,
        };

        let glass = prefs::glass((10.0, 10.0), 20.0 * dpi, prefs::Role::Themed);
        let _ = self.popup.panel.set_params(glass);
        let _ = self.popup.set_rect(x, y, wi, hi);

        self.render();
        self.animate_open();
        let _ = self.popup.show();
        unsafe {
            let _ = SetForegroundWindow(self.popup.hwnd);
            let _ = SetFocus(Some(self.popup.hwnd));
        }
        input::begin_dismiss(self.win_rect, self.popup.hwnd.0 as isize, WM_DISMISS);
    }

    fn close_animated(&mut self) {
        if !self.popup.is_visible() || self.closing {
            return;
        }
        if let Some(pm) = self.power_menu.as_mut() {
            pm.hide();
        }
        input::end_dismiss();
        self.closing = true;
        self.animate_close();
        unsafe {
            let _ = SetTimer(Some(self.popup.hwnd), T_HIDE, 150, None);
        }
    }

    fn animate_open(&self) {
        if !prefs::get().anim || prefs::get().reduce_motion {
            let _ = self.popup.root().SetOpacity(1.0);
            let _ = self.popup.root().SetScale(Vector3 {
                X: 1.0,
                Y: 1.0,
                Z: 1.0,
            });
            return;
        }
        let go = || -> windows_core::Result<()> {
            let c = &self.comp;
            let ease = CompositionEasingFunction::CreateCubicBezierEasingFunction(
                c,
                Vector2 { X: 0.16, Y: 1.0 },
                Vector2 { X: 0.3, Y: 1.0 },
            )?;
            let _ = self.popup.root().SetCenterPoint(self.pop_at);

            let sc = c.CreateVector3KeyFrameAnimation()?;
            sc.SetDuration(windows::Foundation::TimeSpan {
                Duration: 200 * 10_000,
            })?;
            sc.InsertKeyFrameWithEasingFunction(
                0.0,
                Vector3 {
                    X: 0.94,
                    Y: 0.94,
                    Z: 1.0,
                },
                &ease,
            )?;
            sc.InsertKeyFrameWithEasingFunction(
                1.0,
                Vector3 {
                    X: 1.0,
                    Y: 1.0,
                    Z: 1.0,
                },
                &ease,
            )?;

            let op = c.CreateScalarKeyFrameAnimation()?;
            op.SetDuration(windows::Foundation::TimeSpan {
                Duration: 150 * 10_000,
            })?;
            op.InsertKeyFrameWithEasingFunction(0.0, 0.0, &ease)?;
            op.InsertKeyFrameWithEasingFunction(1.0, 1.0, &ease)?;

            let _ = self.popup.root().StartAnimation(&HSTRING::from("Scale"), &sc);
            let _ = self
                .popup
                .root()
                .StartAnimation(&HSTRING::from("Opacity"), &op);
            Ok(())
        };
        let _ = go();
    }

    fn animate_close(&self) {
        if !prefs::get().anim || prefs::get().reduce_motion {
            self.popup.hide();
            return;
        }
        let go = || -> windows_core::Result<()> {
            let c = &self.comp;
            let ease = CompositionEasingFunction::CreateCubicBezierEasingFunction(
                c,
                Vector2 { X: 0.4, Y: 0.0 },
                Vector2 { X: 1.0, Y: 1.0 },
            )?;
            let _ = self.popup.root().SetCenterPoint(self.pop_at);

            let sc = c.CreateVector3KeyFrameAnimation()?;
            sc.SetDuration(windows::Foundation::TimeSpan {
                Duration: 130 * 10_000,
            })?;
            sc.InsertKeyFrameWithEasingFunction(
                1.0,
                Vector3 {
                    X: 0.95,
                    Y: 0.95,
                    Z: 1.0,
                },
                &ease,
            )?;

            let op = c.CreateScalarKeyFrameAnimation()?;
            op.SetDuration(windows::Foundation::TimeSpan {
                Duration: 120 * 10_000,
            })?;
            op.InsertKeyFrameWithEasingFunction(1.0, 0.0, &ease)?;

            let _ = self.popup.root().StartAnimation(&HSTRING::from("Scale"), &sc);
            let _ = self
                .popup
                .root()
                .StartAnimation(&HSTRING::from("Opacity"), &op);
            Ok(())
        };
        let _ = go();
    }

    fn rebuild_shown(&mut self) {
        let q = self.query.trim();
        if !q.is_empty() {
            let idxs = search::rank(q, &self.names, 60);
            self.shown = idxs.into_iter().map(|i| self.apps[i].clone()).collect();
            self.scroll = 0;
            self.sel = if self.shown.is_empty() { None } else { Some(0) };
        } else if self.all_apps {
            let mut list = self.apps.clone();
            list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            self.shown = list;
            self.scroll = 0;
            self.sel = None;
        } else {
            self.shown = self.pinned.clone();
            self.scroll = 0;
            self.sel = None;
        }
    }

    fn text_surface(
        &mut self,
        label: &str,
        px: i32,
        weight: i32,
        face: Option<&str>,
    ) -> Option<(CompositionDrawingSurface, i32, i32)> {
        let key = format!("{px}|{weight}|{}|{label}", face.unwrap_or(""));
        if let Some(v) = self.text_cache.get(&key) {
            return Some(v.clone());
        }
        let st = Style { px, weight };
        let (mw, mh) = measure(label, &st);
        let w = (mw + 4).max(4);
        let h = (mh + 4).max(4);
        let bmp = match face {
            Some(f) => render_face(label, &st, f, w, h)?,
            None => render_text(label, &st, w, h)?,
        };
        let surf = self.gfx.surface_from_bgra(bmp.w, bmp.h, &bmp.bgra).ok()?;
        let val = (surf, bmp.w as i32, bmp.h as i32);
        if self.text_cache.len() > 400 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, val.clone());
        Some(val)
    }

    fn draw_text(
        &mut self,
        parent: &ContainerVisual,
        text: &str,
        x: f32,
        y: f32,
        px: i32,
        weight: i32,
        rgb: [u8; 3],
        alpha: f32,
    ) -> f32 {
        let Some((surf, tw, th)) = self.text_surface(text, px, weight, None) else {
            return 0.0;
        };
        let go = || -> windows_core::Result<()> {
            let brush = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            brush.SetStretch(CompositionStretch::None)?;
            let mask = self.comp.CreateSpriteVisual()?;
            mask.SetSize(Vector2 {
                X: tw as f32,
                Y: th as f32,
            })?;
            mask.SetBrush(&brush)?;
            let tint = self.comp.CreateColorBrushWithColor(Color {
                A: 255,
                R: rgb[0],
                G: rgb[1],
                B: rgb[2],
            })?;
            let mb = self.comp.CreateMaskBrush()?;
            mb.SetMask(&brush)?;
            mb.SetSource(&tint)?;
            let sp = self.comp.CreateSpriteVisual()?;
            sp.SetSize(Vector2 {
                X: tw as f32,
                Y: th as f32,
            })?;
            sp.SetOffset(Vector3 {
                X: x.round(),
                Y: y.round(),
                Z: 0.0,
            })?;
            sp.SetOpacity(alpha)?;
            sp.SetBrush(&mb)?;
            parent.Children()?.InsertAtTop(&sp)?;
            Ok(())
        };
        let _ = go();
        tw as f32
    }

    fn draw_glyph(
        &mut self,
        parent: &ContainerVisual,
        glyph: &str,
        cx: f32,
        cy: f32,
        px: i32,
        rgb: [u8; 3],
        alpha: f32,
    ) {
        let Some((surf, tw, th)) = self.text_surface(glyph, px, 400, Some(ICON_FACE)) else {
            return;
        };
        let go = || -> windows_core::Result<()> {
            let brush = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            brush.SetStretch(CompositionStretch::None)?;
            let tint = self.comp.CreateColorBrushWithColor(Color {
                A: 255,
                R: rgb[0],
                G: rgb[1],
                B: rgb[2],
            })?;
            let mb = self.comp.CreateMaskBrush()?;
            mb.SetMask(&brush)?;
            mb.SetSource(&tint)?;
            let sp = self.comp.CreateSpriteVisual()?;
            sp.SetSize(Vector2 {
                X: tw as f32,
                Y: th as f32,
            })?;
            sp.SetOffset(Vector3 {
                X: (cx - tw as f32 / 2.0).round(),
                Y: (cy - th as f32 / 2.0).round(),
                Z: 0.0,
            })?;
            sp.SetOpacity(alpha)?;
            sp.SetBrush(&mb)?;
            parent.Children()?.InsertAtTop(&sp)?;
            Ok(())
        };
        let _ = go();
    }

    fn draw_rounded(
        &self,
        parent: &ContainerVisual,
        r: Rect,
        radius: f32,
        c: Color,
    ) {
        let go = || -> windows_core::Result<()> {
            let g = self.comp.CreateRoundedRectangleGeometry()?;
            g.SetSize(Vector2 { X: r.w, Y: r.h })?;
            g.SetCornerRadius(Vector2 {
                X: radius,
                Y: radius,
            })?;
            let shape = self.comp.CreateSpriteShapeWithGeometry(&g)?;
            shape.SetFillBrush(&self.comp.CreateColorBrushWithColor(c)?)?;
            let sv = self.comp.CreateShapeVisual()?;
            sv.SetSize(Vector2 { X: r.w, Y: r.h })?;
            sv.SetOffset(Vector3 {
                X: r.x.round(),
                Y: r.y.round(),
                Z: 0.0,
            })?;
            sv.Shapes()?.Append(&shape)?;
            parent.Children()?.InsertAtTop(&sv)?;
            Ok(())
        };
        let _ = go();
    }

    fn draw_rounded_border(
        &self,
        parent: &ContainerVisual,
        r: Rect,
        radius: f32,
        fill: Color,
        stroke: Color,
    ) {
        let go = || -> windows_core::Result<()> {
            let g = self.comp.CreateRoundedRectangleGeometry()?;
            g.SetSize(Vector2 { X: r.w, Y: r.h })?;
            g.SetCornerRadius(Vector2 {
                X: radius,
                Y: radius,
            })?;
            let shape = self.comp.CreateSpriteShapeWithGeometry(&g)?;
            shape.SetFillBrush(&self.comp.CreateColorBrushWithColor(fill)?)?;
            shape.SetStrokeBrush(&self.comp.CreateColorBrushWithColor(stroke)?)?;
            shape.SetStrokeThickness(1.0)?;
            let sv = self.comp.CreateShapeVisual()?;
            sv.SetSize(Vector2 { X: r.w, Y: r.h })?;
            sv.SetOffset(Vector3 {
                X: r.x.round(),
                Y: r.y.round(),
                Z: 0.0,
            })?;
            sv.Shapes()?.Append(&shape)?;
            parent.Children()?.InsertAtTop(&sv)?;
            Ok(())
        };
        let _ = go();
    }

    fn app_icon(&mut self, path: &str, sz: i32) -> Option<CompositionDrawingSurface> {
        let s = prefs::get();
        let look = prefs::look(prefs::Role::Themed);
        let key = format!(
            "{path}|{sz}|{}|{:?}|{}|{}",
            s.dock_plates, s.icon_style, s.icon_tint, look.dark
        );
        if let Some(hit) = self.icon_cache.get(&key) {
            return hit.clone();
        }
        let cached = ICON_CACHE
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|m| m.get(&key).cloned()));
        if let Some(opt_bmp) = cached {
            let surf = opt_bmp.and_then(|b| self.gfx.surface_from_bgra(b.w, b.h, &b.bgra).ok());
            if self.icon_cache.len() > 250 {
                self.icon_cache.clear();
            }
            self.icon_cache.insert(key, surf.clone());
            return surf;
        }
        let should_ask = ICON_ASKED
            .lock()
            .ok()
            .is_some_and(|mut g| g.get_or_insert_with(HashSet::new).insert(key.clone()));
        if should_ask {
            let tx = {
                let mut g = ICON_TX.lock().unwrap();
                if g.is_none() {
                    let (tx, rx) = std::sync::mpsc::channel::<(
                        String,
                        String,
                        i32,
                        bool,
                        nacre_core::settings::IconStyle,
                        u8,
                        bool,
                        isize,
                    )>();
                    *g = Some(tx);
                    std::thread::spawn(move || {
                        use windows::Win32::System::Com::{
                            CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
                        };
                        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
                        let mut pending_notify: Option<isize> = None;
                        while let Ok((k, p, sz, plates, style, tint, dark, hwnd_raw)) = rx.recv()
                        {
                            let mut batch = vec![(k, p, sz, plates, style, tint, dark, hwnd_raw)];
                            while let Ok(item) = rx.try_recv() {
                                batch.push(item);
                            }
                            for (k, p, sz, plates, style, tint, dark, hr) in batch {
                                let bmp = icons::load(&p, sz).map(|mut b| {
                                    if !icons::is_themed(&p) {
                                        if plates && b.w == b.h {
                                            b.bgra = nacre_core::iconplate::plate(&b.bgra, b.w);
                                        }
                                        b.bgra = nacre_core::iconplate::style_icon_v(
                                            &b.bgra, style, tint, dark,
                                        );
                                    }
                                    b
                                });
                                if let Ok(mut cg) = ICON_CACHE.lock() {
                                    cg.get_or_insert_with(Default::default).insert(k, bmp);
                                }
                                pending_notify = Some(hr);
                            }
                            if let Some(hr) = pending_notify.take() {
                                unsafe {
                                    let _ = PostMessageW(
                                        Some(HWND(hr as *mut _)),
                                        WM_ICONS_READY,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                }
                            }
                        }
                        if com {
                            unsafe { CoUninitialize() };
                        }
                    });
                }
                g.clone()
            };
            if let Some(tx) = tx {
                let _ = tx.send((
                    key,
                    path.to_string(),
                    sz,
                    s.dock_plates,
                    s.icon_style,
                    s.icon_tint,
                    look.dark,
                    self.popup.hwnd.0 as isize,
                ));
            }
        }
        None
    }

    fn fit_label(&self, name: &str, max_w: f32, px: i32) -> String {
        let st = Style { px, weight: 400 };
        if measure(name, &st).0 as f32 <= max_w {
            return name.to_string();
        }
        let mut s = String::new();
        for ch in name.chars() {
            s.push(ch);
            let try_s = format!("{s}…");
            if measure(&try_s, &st).0 as f32 > max_w {
                s.pop();
                return format!("{s}…");
            }
        }
        s
    }

    fn render(&mut self) {
        self.hits.clear();
        let Ok(kids) = self.popup.content.Children() else {
            return;
        };
        let _ = kids.RemoveAll();

        let d = self.dpi;
        let (w, h) = self.size;
        let look = prefs::look(prefs::Role::Themed);
        let ink: [u8; 3] = if look.dark {
            [245, 245, 248]
        } else {
            [22, 22, 26]
        };
        let sub_ink: [u8; 3] = if look.dark {
            [175, 178, 188]
        } else {
            [90, 94, 105]
        };
        let pad = 22.0 * d;
        let inner_w = w - 2.0 * pad;
        let parent = self.popup.content.clone();

        // 1. Search bar
        let sb_rect = Rect {
            x: pad,
            y: 18.0 * d,
            w: inner_w,
            h: 38.0 * d,
        };
        let sb_fill = if look.dark {
            Color {
                A: 34,
                R: 255,
                G: 255,
                B: 255,
            }
        } else {
            Color {
                A: 180,
                R: 255,
                G: 255,
                B: 255,
            }
        };
        let sb_stroke = if look.dark {
            Color {
                A: 38,
                R: 255,
                G: 255,
                B: 255,
            }
        } else {
            Color {
                A: 36,
                R: 0,
                G: 0,
                B: 0,
            }
        };
        self.draw_rounded_border(&parent, sb_rect, 19.0 * d, sb_fill, sb_stroke);
        self.draw_glyph(
            &parent,
            "\u{E721}",
            sb_rect.x + 19.0 * d,
            sb_rect.y + sb_rect.h / 2.0,
            (13.0 * d).round() as i32,
            sub_ink,
            0.85,
        );

        let q_px = (13.0 * d).round() as i32;
        if self.query.is_empty() {
            let ph = prefs::tr("Cari aplikasi, pengaturan…").to_string();
            self.draw_text(
                &parent,
                &ph,
                sb_rect.x + 36.0 * d,
                sb_rect.y + (sb_rect.h - 17.0 * d) / 2.0,
                q_px,
                400,
                sub_ink,
                0.75,
            );
        } else {
            let q = format!("{}|", self.query);
            self.draw_text(
                &parent,
                &q,
                sb_rect.x + 36.0 * d,
                sb_rect.y + (sb_rect.h - 17.0 * d) / 2.0,
                q_px,
                500,
                ink,
                0.98,
            );
        }

        // 2. Section header
        let hdr_y = 70.0 * d;
        let hdr_title = if !self.query.trim().is_empty() {
            prefs::tr("Hasil pencarian")
        } else if self.all_apps {
            prefs::tr("Semua aplikasi")
        } else {
            prefs::tr("Disematkan")
        };
        self.draw_text(
            &parent,
            hdr_title,
            pad + 4.0 * d,
            hdr_y + 2.0 * d,
            (13.5 * d).round() as i32,
            600,
            ink,
            0.95,
        );

        if self.query.trim().is_empty() {
            let btn_label = prefs::tr(if self.all_apps {
                "◂ Kembali"
            } else {
                "Semua ▸"
            });
            let bw = measure(
                btn_label,
                &Style {
                    px: (12.0 * d).round() as i32,
                    weight: 500,
                },
            )
            .0 as f32
                + 20.0 * d;
            let bh = 24.0 * d;
            let btn_rect = Rect {
                x: w - pad - bw,
                y: hdr_y,
                w: bw,
                h: bh,
            };
            let btn_bg = if self.hover == Some(Hit::ToggleAll) {
                if look.dark {
                    Color {
                        A: 55,
                        R: 255,
                        G: 255,
                        B: 255,
                    }
                } else {
                    Color {
                        A: 30,
                        R: 0,
                        G: 0,
                        B: 0,
                    }
                }
            } else if look.dark {
                Color {
                    A: 26,
                    R: 255,
                    G: 255,
                    B: 255,
                }
            } else {
                Color {
                    A: 16,
                    R: 0,
                    G: 0,
                    B: 0,
                }
            };
            self.draw_rounded(&parent, btn_rect, 12.0 * d, btn_bg);
            self.draw_text(
                &parent,
                btn_label,
                btn_rect.x + 10.0 * d,
                btn_rect.y + 4.0 * d,
                (12.0 * d).round() as i32,
                500,
                ink,
                0.9,
            );
            self.hits.push((btn_rect, Hit::ToggleAll));
        }

        // 3. App Grid
        let grid_y = 104.0 * d;
        let footer_h = 56.0 * d;
        let grid_h = (h - footer_h - grid_y - 8.0 * d).max(120.0);
        let cell_w = inner_w / COLS as f32;
        let cell_h = (grid_h / VISIBLE_ROWS as f32).floor();
        let icon_sz = (40.0 * d).round() as i32;
        let lbl_px = (11.5 * d).round() as i32;

        if self.shown.is_empty() {
            let empty = prefs::tr("Tidak ada hasil");
            let tw = measure(empty, &Style { px: q_px, weight: 400 }).0 as f32;
            self.draw_text(
                &parent,
                empty,
                (w - tw) / 2.0,
                grid_y + grid_h * 0.35,
                q_px,
                400,
                sub_ink,
                0.7,
            );
        } else {
            let start = self.scroll * COLS;
            let shown = self.shown.clone();
            for (slot, app) in shown.iter().skip(start).take(COLS * VISIBLE_ROWS).enumerate() {
                let idx = start + slot;
                let col = slot % COLS;
                let row = slot / COLS;
                let r = Rect {
                    x: pad + col as f32 * cell_w + 2.0 * d,
                    y: grid_y + row as f32 * cell_h + 2.0 * d,
                    w: cell_w - 4.0 * d,
                    h: cell_h - 4.0 * d,
                };
                let is_hover = self.hover == Some(Hit::App(idx));
                let is_sel = self.sel == Some(idx);
                if is_hover || is_sel {
                    let bg = if is_sel {
                        prefs::accent_c(if look.dark { 75 } else { 55 })
                    } else if look.dark {
                        Color {
                            A: 34,
                            R: 255,
                            G: 255,
                            B: 255,
                        }
                    } else {
                        Color {
                            A: 24,
                            R: 0,
                            G: 0,
                            B: 0,
                        }
                    };
                    self.draw_rounded(&parent, r, 12.0 * d, bg);
                }

                let ix = r.x + (r.w - icon_sz as f32) / 2.0;
                let iy = r.y + 12.0 * d;
                if let Some(surf) = self.app_icon(&app.path, icon_sz) {
                    let go = || -> windows_core::Result<()> {
                        let brush = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
                        brush.SetStretch(CompositionStretch::Uniform)?;
                        let sp = self.comp.CreateSpriteVisual()?;
                        sp.SetSize(Vector2 {
                            X: icon_sz as f32,
                            Y: icon_sz as f32,
                        })?;
                        sp.SetOffset(Vector3 {
                            X: ix.round(),
                            Y: iy.round(),
                            Z: 0.0,
                        })?;
                        sp.SetBrush(&brush)?;
                        parent.Children()?.InsertAtTop(&sp)?;
                        Ok(())
                    };
                    let _ = go();
                } else {
                    self.draw_rounded(
                        &parent,
                        Rect {
                            x: ix,
                            y: iy,
                            w: icon_sz as f32,
                            h: icon_sz as f32,
                        },
                        10.0 * d,
                        Color {
                            A: 24,
                            R: ink[0],
                            G: ink[1],
                            B: ink[2],
                        },
                    );
                }

                let label = self.fit_label(&app.name, r.w - 8.0 * d, lbl_px);
                let lw = measure(
                    &label,
                    &Style {
                        px: lbl_px,
                        weight: 400,
                    },
                )
                .0 as f32;
                self.draw_text(
                    &parent,
                    &label,
                    r.x + (r.w - lw) / 2.0,
                    iy + icon_sz as f32 + 7.0 * d,
                    lbl_px,
                    400,
                    ink,
                    0.92,
                );
                self.hits.push((r, Hit::App(idx)));
            }
        }

        // 4. Footer bar
        let fy = h - footer_h;
        let div_go = || -> windows_core::Result<()> {
            let line = self.comp.CreateSpriteVisual()?;
            line.SetSize(Vector2 {
                X: w - 2.0 * pad,
                Y: 1.0,
            })?;
            line.SetOffset(Vector3 {
                X: pad,
                Y: fy,
                Z: 0.0,
            })?;
            line.SetBrush(&self.comp.CreateColorBrushWithColor(Color {
                A: if look.dark { 28 } else { 24 },
                R: ink[0],
                G: ink[1],
                B: ink[2],
            })?)?;
            parent.Children()?.InsertAtTop(&line)?;
            Ok(())
        };
        let _ = div_go();

        // User profile pill
        let uname = user_name();
        let av_d = 30.0 * d;
        let uw = measure(
            &uname,
            &Style {
                px: (13.0 * d).round() as i32,
                weight: 500,
            },
        )
        .0 as f32;
        let user_rect = Rect {
            x: pad - 4.0 * d,
            y: fy + (footer_h - 38.0 * d) / 2.0,
            w: av_d + 18.0 * d + uw,
            h: 38.0 * d,
        };
        if self.hover == Some(Hit::User) {
            self.draw_rounded(
                &parent,
                user_rect,
                10.0 * d,
                Color {
                    A: if look.dark { 30 } else { 20 },
                    R: ink[0],
                    G: ink[1],
                    B: ink[2],
                },
            );
        }
        let av_rect = Rect {
            x: user_rect.x + 4.0 * d,
            y: user_rect.y + (user_rect.h - av_d) / 2.0,
            w: av_d,
            h: av_d,
        };
        self.draw_rounded(&parent, av_rect, av_d / 2.0, prefs::accent_c(255));
        let initial = uname
            .chars()
            .next()
            .unwrap_or('A')
            .to_uppercase()
            .to_string();
        let ipx = (13.5 * d).round() as i32;
        let (iw, ih) = measure(&initial, &Style { px: ipx, weight: 600 });
        self.draw_text(
            &parent,
            &initial,
            av_rect.x + (av_d - iw as f32) / 2.0,
            av_rect.y + (av_d - ih as f32) / 2.0 - 1.0,
            ipx,
            600,
            [255, 255, 255],
            1.0,
        );
        self.draw_text(
            &parent,
            &uname,
            av_rect.x + av_d + 10.0 * d,
            user_rect.y + (user_rect.h - 17.0 * d) / 2.0,
            (13.0 * d).round() as i32,
            500,
            ink,
            0.95,
        );
        self.hits.push((user_rect, Hit::User));

        // Right footer buttons: Settings + Power
        let btn_sz = 34.0 * d;
        let pwr_rect = Rect {
            x: w - pad - btn_sz,
            y: fy + (footer_h - btn_sz) / 2.0,
            w: btn_sz,
            h: btn_sz,
        };
        if self.hover == Some(Hit::Power) {
            self.draw_rounded(
                &parent,
                pwr_rect,
                10.0 * d,
                Color {
                    A: if look.dark { 34 } else { 22 },
                    R: ink[0],
                    G: ink[1],
                    B: ink[2],
                },
            );
        }
        self.draw_glyph(
            &parent,
            "\u{E7E8}",
            pwr_rect.x + btn_sz / 2.0,
            pwr_rect.y + btn_sz / 2.0,
            (15.0 * d).round() as i32,
            ink,
            0.92,
        );
        self.hits.push((pwr_rect, Hit::Power));

        let set_rect = Rect {
            x: pwr_rect.x - btn_sz - 6.0 * d,
            y: pwr_rect.y,
            w: btn_sz,
            h: btn_sz,
        };
        if self.hover == Some(Hit::Settings) {
            self.draw_rounded(
                &parent,
                set_rect,
                10.0 * d,
                Color {
                    A: if look.dark { 34 } else { 22 },
                    R: ink[0],
                    G: ink[1],
                    B: ink[2],
                },
            );
        }
        self.draw_glyph(
            &parent,
            "\u{E713}",
            set_rect.x + btn_sz / 2.0,
            set_rect.y + btn_sz / 2.0,
            (15.0 * d).round() as i32,
            ink,
            0.92,
        );
        self.hits.push((set_rect, Hit::Settings));
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, h)| *h)
    }

    fn launch_app(&mut self, idx: usize) {
        if let Some(app) = self.shown.get(idx).cloned() {
            self.close_animated();
            launch::open(&app.path);
        }
    }

    fn open_power_menu(&mut self, r: Rect) {
        if self.power_menu.is_none() {
            let tier = {
                let c = sys::conditions();
                if !c.transparency || c.battery_saver || c.remote_session {
                    Tier::Solid
                } else {
                    Tier::Full
                }
            };
            self.power_menu = Menu::new(&self.comp, self.popup.hwnd, self.dpi, tier).ok();
        }
        let Some(pm) = self.power_menu.as_mut() else {
            return;
        };
        let items = vec![
            Item::action(CMD_SLEEP, prefs::tr("Tidur"), true),
            Item::action(CMD_RESTART, prefs::tr("Mulai Ulang…"), true),
            Item::action(CMD_SHUTDOWN, prefs::tr("Matikan…"), true),
            Item::separator(),
            Item::action(CMD_LOCK, prefs::tr("Kunci Layar"), true),
        ];
        let mut pt = POINT {
            x: (r.x + r.w / 2.0).round() as i32,
            y: (r.y - 6.0 * self.dpi).round() as i32,
        };
        unsafe {
            let _ = ClientToScreen(self.popup.hwnd, &mut pt);
        }
        input::end_dismiss();
        let _ = pm.show(&self.comp, &self.gfx, items, pt.x, pt.y);
    }

    fn run_power_cmd(&mut self, cmd: usize) {
        self.close_animated();
        match cmd {
            CMD_SLEEP => launch::run("rundll32.exe", "powrprof.dll,SetSuspendState 0,1,0"),
            CMD_RESTART => confirm_power("Mulai ulang komputer sekarang?", "shutdown.exe", "/r /t 0"),
            CMD_SHUTDOWN => confirm_power("Matikan komputer sekarang?", "shutdown.exe", "/s /t 0"),
            CMD_LOCK => launch::run("rundll32.exe", "user32.dll,LockWorkStation"),
            _ => {}
        }
    }

    fn move_sel(&mut self, dx: isize, dy: isize) {
        if self.shown.is_empty() {
            return;
        }
        let cur = self.sel.unwrap_or(0) as isize;
        let next = (cur + dx + dy * COLS as isize).clamp(0, self.shown.len() as isize - 1) as usize;
        self.sel = Some(next);
        let row = next / COLS;
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + VISIBLE_ROWS {
            self.scroll = row + 1 - VISIBLE_ROWS;
        }
        self.render();
    }

    fn on_click(&mut self, x: f32, y: f32) {
        let Some(hit) = self.hit_at(x, y) else {
            return;
        };
        match hit {
            Hit::ToggleAll => {
                self.all_apps = !self.all_apps;
                self.rebuild_shown();
                self.render();
            }
            Hit::App(i) => self.launch_app(i),
            Hit::User => {
                self.close_animated();
                launch::open("ms-settings:yourinfo");
            }
            Hit::Settings => {
                let comp = self.comp.clone();
                self.close_animated();
                crate::settings_win::open(&comp);
            }
            Hit::Power => {
                if let Some((r, _)) = self.hits.iter().find(|(_, h)| *h == Hit::Power).copied() {
                    self.open_power_menu(r);
                }
            }
        }
    }
}

fn confirm_power(question: &'static str, file: &'static str, params: &'static str) {
    let text = prefs::tr(question).to_string();
    std::thread::spawn(move || unsafe {
        let r = MessageBoxW(
            None,
            &HSTRING::from(text),
            w!("ABRA"),
            MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
        );
        if r == IDYES {
            launch::run(file, params);
        }
    });
}

fn xy(lp: LPARAM) -> (f32, f32) {
    ((lp.0 as i16) as f32, ((lp.0 >> 16) as i16) as f32)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            with(|m| {
                if !m.tracking {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    if TrackMouseEvent(&mut t).is_ok() {
                        m.tracking = true;
                    }
                }
                let h = m.hit_at(x, y);
                if h != m.hover {
                    m.hover = h;
                    m.render();
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|m| {
                m.tracking = false;
                if m.hover.take().is_some() {
                    m.render();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            with(|m| m.on_click(x, y));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wp.0 >> 16) as i16;
            with(|m| {
                if !m.all_apps && m.query.trim().is_empty() {
                    return;
                }
                let max_scroll = m.shown.len().div_ceil(COLS).saturating_sub(VISIBLE_ROWS);
                let next = if delta < 0 {
                    (m.scroll + 1).min(max_scroll)
                } else {
                    m.scroll.saturating_sub(1)
                };
                if next != m.scroll {
                    m.scroll = next;
                    m.render();
                }
            });
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(ch) = char::from_u32(wp.0 as u32) {
                if !ch.is_control() {
                    with(|m| {
                        m.query.push(ch);
                        m.rebuild_shown();
                        m.render();
                    });
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u16;
            if vk == VK_ESCAPE.0 {
                with(|m| {
                    if !m.query.is_empty() {
                        m.query.clear();
                        m.rebuild_shown();
                        m.render();
                    } else {
                        m.close_animated();
                    }
                });
                return LRESULT(0);
            }
            if vk == VK_BACK.0 {
                with(|m| {
                    if m.query.pop().is_some() {
                        m.rebuild_shown();
                        m.render();
                    }
                });
                return LRESULT(0);
            }
            if vk == VK_RETURN.0 {
                with(|m| {
                    if let Some(i) = m.sel {
                        m.launch_app(i);
                    }
                });
                return LRESULT(0);
            }
            let (dx, dy) = match vk {
                v if v == VK_LEFT.0 => (-1, 0),
                v if v == VK_RIGHT.0 => (1, 0),
                v if v == VK_UP.0 => (0, -1),
                v if v == VK_DOWN.0 => (0, 1),
                _ => (0, 0),
            };
            if dx != 0 || dy != 0 {
                with(|m| m.move_sel(dx, dy));
                return LRESULT(0);
            }
            LRESULT(0)
        }
        WM_ICONS_READY => {
            with(|m| {
                if m.popup.is_visible() && !m.closing {
                    m.render();
                }
            });
            LRESULT(0)
        }
        MENU_PICK => {
            with(|m| m.run_power_cmd(wp.0));
            LRESULT(0)
        }
        MENU_CLOSE => {
            with(|m| {
                if m.popup.is_visible() && !m.closing {
                    input::begin_dismiss(m.win_rect, m.popup.hwnd.0 as isize, WM_DISMISS);
                }
            });
            LRESULT(0)
        }
        WM_DISMISS => {
            with(|m| {
                if !m.power_menu.as_ref().is_some_and(|pm| pm.is_open()) {
                    m.close_animated();
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wp.0 == T_HIDE => {
            let _ = KillTimer(Some(hwnd), T_HIDE);
            with(|m| {
                if m.closing {
                    m.closing = false;
                    m.popup.hide();
                }
            });
            LRESULT(0)
        }
        crate::prefs::WM_PREFS => {
            with(|m| {
                let glass = prefs::glass((10.0, 10.0), 20.0 * m.dpi, prefs::Role::Themed);
                let _ = m.popup.panel.set_params(glass);
                m.text_cache.clear();
                m.icon_cache.clear();
                if m.popup.is_visible() && !m.closing {
                    m.render();
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
