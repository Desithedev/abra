use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;
use std::sync::mpsc::{channel, Sender};
use wmi::{COMLibrary, WMIConnection};

static LEVEL: AtomicI32 = AtomicI32::new(-1);
static SOFT: AtomicBool = AtomicBool::new(false);

const FLOOR: f32 = 0.35;
const PROBE: Duration = Duration::from_secs(4);

fn store(v: i32) {
    if !SOFT.load(Ordering::Relaxed) {
        LEVEL.store(v, Ordering::Relaxed);
    }
}

pub fn is_software() -> bool {
    SOFT.load(Ordering::Relaxed)
}

pub fn ramp_for(p: u8) -> [[u16; 256]; 3] {
    let mult = FLOOR + (1.0 - FLOOR) * (p.min(100) as f32 / 100.0);
    let mut ramp = [[0u16; 256]; 3];
    for i in 0..256 {
        let v = ((i as f32 * 257.0) * mult).round().min(65535.0) as u16;
        for c in ramp.iter_mut() {
            c[i] = v;
        }
    }
    ramp
}

pub fn apply_gamma(p: u8) -> bool {
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    use windows::Win32::UI::ColorSystem::SetDeviceGammaRamp;
    let ramp = ramp_for(p);
    unsafe {
        let dc = GetDC(None);
        if dc.is_invalid() {
            return false;
        }
        let ok = SetDeviceGammaRamp(dc, ramp.as_ptr() as *const std::ffi::c_void).as_bool();
        ReleaseDC(None, dc);
        ok
    }
}

pub fn restore() {
    if SOFT.load(Ordering::Relaxed) {
        apply_gamma(100);
    }
}

pub fn latest() -> Option<u8> {
    let v = LEVEL.load(Ordering::Relaxed);
    (v >= 0).then_some(v as u8)
}

pub enum Cmd {
    Set(u8),
    Refresh,
}

pub struct Brightness {
    pub commands: Sender<Cmd>,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightness")]
#[serde(rename_all = "PascalCase")]
struct Current {
    current_brightness: u8,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightnessEvent")]
#[serde(rename_all = "PascalCase")]
struct Event {
    brightness: u8,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightnessMethods")]
struct Methods {
    #[serde(rename = "__Path")]
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SetA {
    timeout: u32,
    brightness: u8,
}
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SetB {
    timeout: i32,
    brightness: i32,
}
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SetC {
    timeout: u8,
    brightness: u8,
}
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SetD {
    timeout: String,
    brightness: String,
}

static VARIANT: AtomicI32 = AtomicI32::new(0);

fn set(con: &WMIConnection, path: &str, p: u8) -> bool {
    let m = "WmiSetBrightness";
    let start = VARIANT.load(Ordering::Relaxed);
    for off in 0..4 {
        let v = (start + off) % 4;
        let ok = match v {
            0 => con.exec_instance_method::<Methods, _, ()>(m, path, SetA { timeout: 1, brightness: p }).is_ok(),
            1 => con.exec_instance_method::<Methods, _, ()>(m, path, SetB { timeout: 1, brightness: p as i32 }).is_ok(),
            2 => con.exec_instance_method::<Methods, _, ()>(m, path, SetC { timeout: 1, brightness: p }).is_ok(),
            _ => con.exec_instance_method::<Methods, _, ()>(m, path, SetD { timeout: "1".into(), brightness: p.to_string() }).is_ok(),
        };
        if ok {
            if v != start {
                crate::log::line("brightness", &format!("varian {v} bekerja"));
            }
            VARIANT.store(v, Ordering::Relaxed);
            return true;
        }
    }
    false
}

fn read(con: &WMIConnection) -> Option<u8> {
    con.query::<Current>().ok()?.first().map(|c| c.current_brightness.min(100))
}

pub fn spawn(notify: impl Fn() + Send + Sync + 'static) -> Brightness {
    let (tx, rx) = channel::<Cmd>();
    let notify: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(notify);
    std::thread::spawn(move || {
        let forced = std::env::var("NACRE_BRIGHTNESS").is_ok_and(|v| v == "software");
        let hardware = if forced { None } else { start_wmi(notify.clone()) };
        match hardware {
            Some(wtx) => {
                while let Ok(cmd) = rx.recv() {
                    if wtx.send(cmd).is_err() {
                        break;
                    }
                }
            }
            None => {
                SOFT.store(true, Ordering::Relaxed);
                crate::log::line("brightness", "WMI tidak tersedia atau terlalu lambat; memakai redup software (gamma)");
                LEVEL.store(100, Ordering::Relaxed);
                notify();
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Cmd::Set(p) => {
                            let p = p.min(100);
                            if apply_gamma(p) {
                                LEVEL.store(p as i32, Ordering::Relaxed);
                            } else {
                                crate::log::line("brightness", "gamma ramp ditolak oleh driver");
                            }
                            notify();
                        }
                        Cmd::Refresh => notify(),
                    }
                }
            }
        }
    });
    Brightness { commands: tx }
}

fn start_wmi(notify: std::sync::Arc<dyn Fn() + Send + Sync>) -> Option<Sender<Cmd>> {
    let (wtx, wrx) = channel::<Cmd>();
    let (rtx, rrx) = channel::<bool>();
    let n1 = notify.clone();
    std::thread::spawn(move || {
        let Ok(com) = COMLibrary::new() else {
            let _ = rtx.send(false);
            return;
        };
        let Ok(con) = WMIConnection::with_namespace_path("root\\WMI", com) else {
            let _ = rtx.send(false);
            return;
        };
        match read(&con) {
            Some(v) => {
                store(v as i32);
                let _ = rtx.send(true);
                n1();
            }
            None => {
                crate::log::line("brightness", "panel tidak mendukung kecerahan WMI");
                let _ = rtx.send(false);
                return;
            }
        }
        while let Ok(cmd) = wrx.recv() {
            match cmd {
                Cmd::Set(p) => {
                    let p = p.min(100);
                    let path = con.query::<Methods>().ok().and_then(|m| m.into_iter().next()).map(|m| m.path);
                    if let Some(path) = path {
                        if set(&con, &path, p) {
                            store(p as i32);
                            n1();
                        } else {
                            crate::log::line("brightness", "set gagal (semua varian)");
                        }
                    }
                }
                Cmd::Refresh => {
                    if let Some(v) = read(&con) {
                        store(v as i32);
                        n1();
                    }
                }
            }
        }
    });
    if !rrx.recv_timeout(PROBE).unwrap_or(false) {
        return None;
    }
    let n2 = notify;
    std::thread::spawn(move || {
        let Ok(com) = COMLibrary::new() else { return };
        let Ok(con) = WMIConnection::with_namespace_path("root\\WMI", com) else { return };
        let Ok(events) = con.raw_notification::<Event>("SELECT * FROM WmiMonitorBrightnessEvent") else { return };
        for ev in events {
            let Ok(ev) = ev else { break };
            store(ev.brightness.min(100) as i32);
            n2();
        }
    });
    Some(wtx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_is_identity_at_full_and_never_black() {
        let full = ramp_for(100);
        assert_eq!((full[0][0], full[0][255], full[1][128]), (0, 65535, (128.0f32 * 257.0).round() as u16));
        let low = ramp_for(0);
        assert!(low[2][255] >= (65535.0 * FLOOR) as u16 - 1);
        assert!(low[0].windows(2).all(|w| w[0] <= w[1]), "harus naik monoton agar driver menerimanya");
        assert!(ramp_for(50)[0][255] > low[0][255] && ramp_for(50)[0][255] < full[0][255]);
    }

    #[test]
    fn gamma_can_be_set_and_restored_on_this_display() {
        use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
        use windows::Win32::UI::ColorSystem::GetDeviceGammaRamp;
        if std::env::var_os("NACRE_TEST_GAMMA").is_none() {
            return;
        }
        let read = || unsafe {
            let dc = GetDC(None);
            let mut r = [[0u16; 256]; 3];
            let ok = GetDeviceGammaRamp(dc, r.as_mut_ptr() as *mut std::ffi::c_void).as_bool();
            ReleaseDC(None, dc);
            ok.then_some(r)
        };
        let before = read().expect("baca ramp");
        assert!(apply_gamma(50));
        let dim = read().expect("baca ramp redup");
        assert!(dim[0][255] < before[0][255]);
        assert!(apply_gamma(100));
        let after = read().expect("baca ramp pulih");
        assert_eq!(after[0][255], 65535);
    }
}
