use crate::dock::menu::{Item, Menu, MENU_CLOSE, MENU_PICK};
use crate::popup::Popup;
use crate::prefs;
use nacre_platform::text::{measure, render as render_text, Style};
use nacre_platform::trayicons::{self, TrayApp};
use nacre_platform::{input, launch, monitors, sys};
use nacre_ui::gfx::Gfx;
use nacre_ui::glass::Tier;
use std::cell::RefCell;
use windows::core::w;
use windows::UI::Color;
use windows::UI::Composition::{CompositionStretch, Compositor, ContainerVisual, SpriteVisual};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::{Vector2, Vector3};

const WM_DISMISS: u32 = WM_APP + 145;
const WM_MOUSELEAVE: u32 = 0x02A3;
const T_HIDE: usize = 1;

const COLS: usize = 5;
const CTX_OPEN: usize = 1;
const CTX_END: usize = 2;
const CTX_TASKMGR: usize = 3;

struct Cell {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

struct Tray {
    comp: Compositor,
    gfx: Gfx,
    popup: Popup,
    ctx: Option<Menu>,
    ctx_idx: Option<usize>,
    apps: Vec<TrayApp>,
    cells: Vec<Cell>,
    hover: Option<usize>,
    hl: Option<SpriteVisual>,
    label_layer: Option<ContainerVisual>,
    footer_y: f32,
    size: (f32, f32),
    pop_at: Vector3,
    dpi: f32,
    closing: bool,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Tray) -> R) -> Option<R> {
    TRAY.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

fn ensure(comp: &Compositor) -> bool {
    if TRAY.with(|c| c.borrow().is_some()) {
        return true;
    }
    let Ok(gfx) = Gfx::shared(comp) else { return false };
    let glass = prefs::glass((10.0, 10.0), 14.0, prefs::Role::Themed);
    let Ok(popup) = Popup::new(comp, w!("AbraTrayGrid"), Some(proc), WINDOW_EX_STYLE(0), glass) else {
        return false;
    };
    let tier = {
        let c = sys::conditions();
        if !c.transparency || c.battery_saver || c.remote_session { Tier::Solid } else { Tier::Full }
    };
    let _ = popup.panel.set_tier(tier);
    prefs::listen(popup.hwnd);
    TRAY.with(|c| {
        *c.borrow_mut() = Some(Tray {
            comp: comp.clone(),
            gfx,
            popup,
            ctx: None,
            ctx_idx: None,
            apps: Vec::new(),
            cells: Vec::new(),
            hover: None,
            hl: None,
            label_layer: None,
            footer_y: 0.0,
            size: (240.0, 120.0),
            pop_at: Vector3 { X: 120.0, Y: 0.0, Z: 0.0 },
            dpi: 1.0,
            closing: false,
        })
    });
    true
}

pub fn is_open() -> bool {
    with(|t| t.popup.is_visible() && !t.closing).unwrap_or(false)
}

pub fn toggle(comp: &Compositor, x: i32, y: i32, dpi: f32) {
    if !ensure(comp) {
        return;
    }
    with(|t| {
        if t.popup.is_visible() && !t.closing {
            t.close_animated();
            return;
        }
        t.open(x, y, dpi);
    });
}

impl Tray {
    fn open(&mut self, anchor_x: i32, top: i32, dpi: f32) {
        if self.closing {
            self.closing = false;
            unsafe {
                let _ = KillTimer(Some(self.popup.hwnd), T_HIDE);
            }
            self.popup.hide();
        }
        self.dpi = dpi;
        self.hover = None;
        self.ctx_idx = None;
        let px = (22.0 * dpi).round().max(16.0) as u32;
        self.apps = trayicons::running_tray_apps(px);
        self.build_grid(anchor_x, top);
        let _ = crate::motion::pop_in(&self.comp, self.popup.root(), self.pop_at, 0.45, 1.025, 0, crate::motion::CC_OPEN_MS);
        crate::menubar::refresh();
    }

    fn build_grid(&mut self, anchor_x: i32, top: i32) {
        let d = self.dpi;
        let pad = 10.0 * d;
        let gap = 2.0 * d;
        let cell = 42.0 * d;
        let icon_sz = (20.0 * d).round();
        let footer_h = 26.0 * d;
        let count = self.apps.len();
        let cols = if count == 0 { 1 } else { COLS.min(count.max(3)) };
        let rows = if count == 0 { 1 } else { count.div_ceil(cols) };
        let grid_w = cols as f32 * cell + (cols.saturating_sub(1)) as f32 * gap;
        let w = (grid_w + pad * 2.0).max(200.0 * d);
        let grid_h = if count == 0 { 36.0 * d } else { rows as f32 * cell + (rows.saturating_sub(1)) as f32 * gap };
        let h = pad + grid_h + 6.0 * d + footer_h + pad * 0.6;
        let (wi, hi) = (w.ceil() as i32, h.ceil() as i32);

        let mon = monitors::at(anchor_x, top);
        let px = mon.area.clamp_x(anchor_x - wi / 2, wi, 6);
        let py = mon.area.clamp_y(top + 4, hi, 6);

        let r = 14.0 * d * prefs::round(prefs::Role::Themed);
        let _ = self.popup.panel.set_params(prefs::glass((10.0, 10.0), r, prefs::Role::Themed));
        let _ = self.popup.set_rect(px, py, wi, hi);
        let _ = self.popup.panel.set_radius(r);

        if let Ok(ch) = self.popup.content.Children() {
            let _ = ch.RemoveAll();
        }
        self.cells.clear();
        self.size = (w, h);
        self.footer_y = pad + grid_h + 6.0 * d;
        self.pop_at = Vector3 { X: (anchor_x - px).clamp(16, wi - 16) as f32, Y: 0.0, Z: 0.0 };

        let look = prefs::look(prefs::Role::Themed);
        let hl_color = if look.dark { Color { A: 36, R: 255, G: 255, B: 255 } } else { Color { A: 26, R: 0, G: 0, B: 0 } };
        if let Ok(hl) = self.comp.CreateSpriteVisual() {
            if let Ok(br) = self.comp.CreateColorBrushWithColor(hl_color) {
                let _ = hl.SetBrush(&br);
            }
            if let Ok(g) = self.comp.CreateRoundedRectangleGeometry() {
                let _ = g.SetSize(Vector2 { X: cell, Y: cell });
                let _ = g.SetCornerRadius(Vector2 { X: 8.0 * d, Y: 8.0 * d });
                if let Ok(clip) = self.comp.CreateGeometricClip() {
                    let _ = clip.SetGeometry(&g);
                    let _ = hl.SetClip(&clip);
                }
            }
            let _ = hl.SetSize(Vector2 { X: cell, Y: cell });
            let _ = hl.SetOpacity(0.0);
            let _ = self.popup.content.Children().and_then(|c| c.InsertAtTop(&hl));
            self.hl = Some(hl);
        }

        let start_x = ((w - grid_w) / 2.0).round();
        for (i, app) in self.apps.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let cx = start_x + col as f32 * (cell + gap);
            let cy = pad + row as f32 * (cell + gap);
            self.cells.push(Cell { x: cx, y: cy, w: cell, h: cell });
            if let Some(bmp) = &app.icon {
                if let Ok(surf) = self.gfx.surface_from_bgra(bmp.w, bmp.h, &bmp.bgra) {
                    if let Ok(sp) = self.comp.CreateSpriteVisual() {
                        if let Ok(br) = self.comp.CreateSurfaceBrushWithSurface(&surf) {
                            let _ = br.SetStretch(CompositionStretch::Fill);
                            let _ = sp.SetBrush(&br);
                        }
                        let _ = sp.SetSize(Vector2 { X: icon_sz, Y: icon_sz });
                        let _ = sp.SetOffset(Vector3 {
                            X: (cx + (cell - icon_sz) / 2.0).round(),
                            Y: (cy + (cell - icon_sz) / 2.0).round(),
                            Z: 0.0,
                        });
                        let _ = self.popup.content.Children().and_then(|c| c.InsertAtTop(&sp));
                    }
                }
            }
        }

        // Divider line above footer
        if let Ok(sep) = self.comp.CreateSpriteVisual() {
            let sep_c = if look.dark { Color { A: 28, R: 255, G: 255, B: 255 } } else { Color { A: 24, R: 0, G: 0, B: 0 } };
            if let Ok(br) = self.comp.CreateColorBrushWithColor(sep_c) {
                let _ = sep.SetBrush(&br);
            }
            let _ = sep.SetSize(Vector2 { X: w - pad * 2.0, Y: 1.0 });
            let _ = sep.SetOffset(Vector3 { X: pad, Y: self.footer_y - 2.0 * d, Z: 0.0 });
            let _ = self.popup.content.Children().and_then(|c| c.InsertAtTop(&sep));
        }

        if let Ok(ll) = self.comp.CreateContainerVisual() {
            let _ = self.popup.content.Children().and_then(|c| c.InsertAtTop(&ll));
            self.label_layer = Some(ll);
        }
        self.render_footer();

        self.popup.show();
        unsafe {
            let _ = SetWindowPos(self.popup.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        input::begin_dismiss((px, py, px + wi, py + hi), self.popup.hwnd.0 as isize, WM_DISMISS);
    }

    fn render_footer(&mut self) {
        let Some(ll) = &self.label_layer else { return };
        if let Ok(ch) = ll.Children() {
            let _ = ch.RemoveAll();
        }
        let d = self.dpi;
        let pad = 10.0 * d;
        let max_w = (self.size.0 - pad * 2.0).max(40.0) as i32;
        let text = match self.hover.and_then(|i| self.apps.get(i)) {
            Some(app) => app.name.clone(),
            None if self.apps.is_empty() => prefs::tr("Tidak ada aplikasi tray yang berjalan").to_string(),
            None => format!("{} {}", self.apps.len(), prefs::tr("aplikasi berjalan")),
        };
        let style = Style { px: (11.5 * d).round().max(10.0) as i32, weight: 400 };
        let (tw, th) = measure(&text, &style);
        let rw = tw.min(max_w).max(16);
        let rh = th.max(16);
        if let Some(bmp) = render_text(&text, &style, rw, rh) {
            if let Ok(surf) = self.gfx.surface_from_bgra(bmp.w, bmp.h, &bmp.bgra) {
                if let Ok(sp) = self.comp.CreateSpriteVisual() {
                    if let Ok(br) = self.comp.CreateSurfaceBrushWithSurface(&surf) {
                        let _ = sp.SetBrush(&br);
                    }
                    let _ = sp.SetSize(Vector2 { X: bmp.w as f32, Y: bmp.h as f32 });
                    let x = ((self.size.0 - bmp.w as f32) / 2.0).max(pad).round();
                    let y = (self.footer_y + (26.0 * d - bmp.h as f32) / 2.0).round();
                    let _ = sp.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 });
                    let _ = sp.SetOpacity(if self.hover.is_some() { 0.95 } else { 0.62 });
                    let _ = ch_insert(ll, &sp);
                }
            }
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.cells.iter().position(|c| x >= c.x && x < c.x + c.w && y >= c.y && y < c.y + c.h)
    }

    fn set_hover(&mut self, next: Option<usize>) {
        if self.hover == next {
            return;
        }
        self.hover = next;
        if let Some(hl) = &self.hl {
            if let Some(i) = next {
                if let Some(c) = self.cells.get(i) {
                    let _ = hl.SetOffset(Vector3 { X: c.x, Y: c.y, Z: 0.0 });
                    let _ = hl.SetOpacity(1.0);
                }
            } else {
                let _ = hl.SetOpacity(0.0);
            }
        }
        self.render_footer();
    }

    fn on_left_click(&mut self, x: f32, y: f32) {
        if let Some(i) = self.hit(x, y) {
            if let Some(app) = self.apps.get(i) {
                let pids = app.pids.clone();
                let exe = app.exe.clone();
                self.close_animated();
                trayicons::activate_app(&pids, &exe);
            }
        }
    }

    fn on_right_click(&mut self, x: i32, y: i32) {
        let Some(i) = self.hit(x as f32, y as f32) else { return };
        let Some(app) = self.apps.get(i) else { return };
        self.ctx_idx = Some(i);
        let mut pt = POINT { x, y };
        unsafe {
            let _ = ClientToScreen(self.popup.hwnd, &mut pt);
        }
        if self.ctx.is_none() {
            let c = sys::conditions();
            let tier = if !c.transparency || c.battery_saver || c.remote_session { Tier::Solid } else { Tier::Full };
            self.ctx = Menu::new(&self.comp, self.popup.hwnd, self.dpi, tier).ok();
        }
        let Some(ctx) = self.ctx.as_mut() else { return };
        let items = vec![
            Item::heading(&app.name),
            Item::action(CTX_OPEN, prefs::tr("Buka"), true),
            Item::action(CTX_END, prefs::tr("Akhiri tugas"), true),
            Item::separator(),
            Item::action(CTX_TASKMGR, "Task Manager...", true),
        ];
        if ctx.show_below(&self.comp, &self.gfx, items, pt.x, pt.y).is_ok() {
            ctx.raise();
        }
    }

    fn on_ctx_pick(&mut self, id: usize) {
        let idx = self.ctx_idx.take();
        match id {
            CTX_OPEN => {
                if let Some(app) = idx.and_then(|i| self.apps.get(i)) {
                    let pids = app.pids.clone();
                    let exe = app.exe.clone();
                    self.close_animated();
                    trayicons::activate_app(&pids, &exe);
                }
            }
            CTX_END => {
                if let Some(app) = idx.and_then(|i| self.apps.get(i)) {
                    trayicons::end_task(&app.pids);
                }
                self.close_animated();
            }
            CTX_TASKMGR => {
                self.close_animated();
                launch::open("taskmgr.exe");
            }
            _ => {}
        }
    }

    fn on_ctx_close(&mut self) {
        self.ctx_idx = None;
        if self.popup.is_visible() && !self.closing {
            let r = self.popup.rect;
            input::begin_dismiss((r.0, r.1, r.0 + r.2, r.1 + r.3), self.popup.hwnd.0 as isize, WM_DISMISS);
        }
    }

    fn close_animated(&mut self) {
        if !self.popup.is_visible() || self.closing {
            return;
        }
        input::end_dismiss();
        if let Some(c) = self.ctx.as_mut() {
            c.hide();
        }
        if crate::motion::pop_out(&self.comp, self.popup.root(), self.pop_at, 0.55, crate::motion::CC_CLOSE_MS).is_ok() {
            self.closing = true;
            unsafe {
                SetTimer(Some(self.popup.hwnd), T_HIDE, crate::motion::CC_CLOSE_MS + 20, None);
            }
        } else {
            self.popup.hide();
        }
        crate::menubar::refresh();
    }
}

fn ch_insert(parent: &ContainerVisual, child: &SpriteVisual) -> windows_core::Result<()> {
    parent.Children()?.InsertAtTop(child)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            });
            let x = (lp.0 & 0xFFFF) as i16 as f32;
            let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32;
            with(|t| {
                let hit = t.hit(x, y);
                t.set_hover(hit);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|t| t.set_hover(None));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lp.0 & 0xFFFF) as i16 as f32;
            let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32;
            with(|t| t.on_left_click(x, y));
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let x = (lp.0 & 0xFFFF) as i16 as i32;
            let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
            with(|t| t.on_right_click(x, y));
            LRESULT(0)
        }
        MENU_PICK => {
            with(|t| t.on_ctx_pick(wp.0));
            LRESULT(0)
        }
        MENU_CLOSE => {
            with(|t| t.on_ctx_close());
            LRESULT(0)
        }
        WM_DISMISS => {
            with(|t| {
                if !t.ctx.as_ref().is_some_and(|c| c.is_open()) {
                    t.close_animated();
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wp.0 == T_HIDE => {
            let _ = KillTimer(Some(hwnd), T_HIDE);
            with(|t| {
                t.closing = false;
                t.popup.hide();
            });
            LRESULT(0)
        }
        prefs::WM_PREFS => {
            with(|t| {
                let r = 14.0 * t.dpi * prefs::round(prefs::Role::Themed);
                let _ = t.popup.panel.set_params(prefs::glass((10.0, 10.0), r, prefs::Role::Themed));
                if t.popup.is_visible() {
                    t.render_footer();
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
