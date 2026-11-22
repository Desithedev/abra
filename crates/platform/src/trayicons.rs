use crate::icons::Bitmap;
use std::collections::{HashMap, HashSet};
use windows::core::{w, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_BINARY, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};

#[derive(Clone)]
pub struct TrayApp {
    pub name: String,
    pub exe: String,
    pub pids: Vec<u32>,
    pub icon: Option<Bitmap>,
}

fn running_exes() -> HashMap<String, (String, Vec<u32>)> {
    let mut map: HashMap<String, (String, Vec<u32>)> = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return map };
        let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut pe).is_ok();
        while ok {
            let pid = pe.th32ProcessID;
            if pid > 4 {
                let n = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                let short = String::from_utf16_lossy(&pe.szExeFile[..n]).to_ascii_lowercase();
                if !short.is_empty() {
                    let mut full = String::new();
                    if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        let mut buf = [0u16; 520];
                        let mut len = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                            full = String::from_utf16_lossy(&buf[..len as usize]);
                        }
                        let _ = CloseHandle(h);
                    }
                    let entry = map.entry(short).or_insert_with(|| (full.clone(), Vec::new()));
                    if entry.0.is_empty() && !full.is_empty() {
                        entry.0 = full;
                    }
                    entry.1.push(pid);
                }
            }
            ok = Process32NextW(snap, &mut pe).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    map
}

fn reg_str(key: HKEY, name: PCWSTR) -> Option<String> {
    let mut kind = REG_VALUE_TYPE(0);
    let mut len = 0u32;
    unsafe {
        if RegQueryValueExW(key, name, None, Some(&mut kind), None, Some(&mut len)).is_err() || len < 2 {
            return None;
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return None;
        }
        let mut buf = vec![0u16; (len as usize + 1) / 2];
        if RegQueryValueExW(key, name, None, Some(&mut kind), Some(buf.as_mut_ptr() as *mut u8), Some(&mut len)).is_err() {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

fn reg_bytes(key: HKEY, name: PCWSTR) -> Option<Vec<u8>> {
    let mut kind = REG_VALUE_TYPE(0);
    let mut len = 0u32;
    unsafe {
        if RegQueryValueExW(key, name, None, Some(&mut kind), None, Some(&mut len)).is_err() || len == 0 || len > 512 * 1024 {
            return None;
        }
        if kind != REG_BINARY {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if RegQueryValueExW(key, name, None, Some(&mut kind), Some(buf.as_mut_ptr()), Some(&mut len)).is_err() {
            return None;
        }
        buf.truncate(len as usize);
        Some(buf)
    }
}

fn friendly_name(tip: Option<String>, exe: &str) -> String {
    if let Some(t) = tip {
        let line = t.lines().next().unwrap_or("").trim();
        if !line.is_empty() {
            return line.to_string();
        }
    }
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    let mut chars = stem.chars();
    match chars.next() {
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
        None => stem.to_string(),
    }
}

pub fn running_tray_apps(icon_px: u32) -> Vec<TrayApp> {
    let procs = running_exes();
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let skip = ["abra.exe", "svchost.exe", "explorer.exe", "sihost.exe", "monotificationux.exe"];

    unsafe {
        let mut root = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, w!(r"Control Panel\NotifyIconSettings"), Some(0), KEY_READ, &mut root).is_err() {
            return out;
        }
        let mut idx = 0u32;
        loop {
            let mut sub_name = [0u16; 128];
            let mut sub_len = sub_name.len() as u32;
            if RegEnumKeyExW(root, idx, Some(PWSTR(sub_name.as_mut_ptr())), &mut sub_len, None, None, None, None).is_err() {
                break;
            }
            idx += 1;
            let mut sub = HKEY::default();
            if RegOpenKeyExW(root, PCWSTR(sub_name.as_ptr()), Some(0), KEY_READ, &mut sub).is_err() {
                continue;
            }
            if let Some(exe_raw) = reg_str(sub, w!("ExecutablePath")) {
                let short = exe_raw.rsplit(['\\', '/']).next().unwrap_or("").to_ascii_lowercase();
                if !short.is_empty() && !skip.contains(&short.as_str()) && !seen.contains(&short) {
                    if let Some((real_exe, pids)) = procs.get(&short) {
                        seen.insert(short);
                        let exe = if real_exe.is_empty() { exe_raw } else { real_exe.clone() };
                        let name = friendly_name(reg_str(sub, w!("InitialTooltip")), &exe);
                        let icon = reg_bytes(sub, w!("IconSnapshot"))
                            .and_then(|png| crate::wic::decode_square(&png, icon_px))
                            .or_else(|| if !exe.is_empty() { crate::icons::load(&exe, icon_px as i32) } else { None });
                        out.push(TrayApp { name, exe, pids: pids.clone(), icon });
                    }
                }
            }
            let _ = RegCloseKey(sub);
        }
        let _ = RegCloseKey(root);
    }

    out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    out
}

pub fn activate_app(pids: &[u32], exe: &str) {
    struct Ctx<'a> {
        pids: &'a [u32],
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx<'_>);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if ctx.pids.contains(&pid) && GetWindowTextLengthW(h) > 0 {
            let mut r = RECT::default();
            let has_area = GetWindowRect(h, &mut r).is_ok() && (r.right - r.left > 20 || IsIconic(h).as_bool());
            if IsWindowVisible(h).as_bool() || has_area {
                ctx.found = Some(h);
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    let mut ctx = Ctx { pids, found: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx<'_> as isize));
        if let Some(h) = ctx.found {
            let _ = ShowWindow(h, if IsIconic(h).as_bool() { SW_RESTORE } else { SW_SHOW });
            let _ = SetForegroundWindow(h);
            return;
        }
    }
    if !exe.is_empty() {
        crate::launch::open(exe);
    }
}

pub fn end_task(pids: &[u32]) {
    for &pid in pids {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(h, 1);
                let _ = CloseHandle(h);
            }
        }
    }
}
