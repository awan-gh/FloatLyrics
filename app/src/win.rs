//! Win32 plumbing: overlay windows, tray icon, positions, autostart.
use crate::{with_app, App};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

pub fn single_instance() -> bool {
    unsafe {
        let _ = CreateMutexW(None, true, w!("FloatLyrics.SingleInstance.Rust"));
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

pub fn init_process() {
    unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); }
}

pub fn com_init_mta() { unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); } }

pub fn create_overlay(taskbar: bool) -> HWND {
    unsafe {
        let inst = GetModuleHandleW(None).unwrap();
        let class = w!("FloatLyricsOverlay");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc); // second call fails harmlessly
        let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
        if taskbar { ex |= WS_EX_TRANSPARENT; }
        let h = CreateWindowExW(ex, class, if taskbar { w!("FloatLyrics Taskbar") } else { w!("FloatLyrics") }, WS_POPUP,
            -32000, -32000, 10, 10, None, None, Some(inst.into()), None).unwrap();
        if !taskbar { let _ = ShowWindow(h, SW_SHOWNOACTIVATE); }
        h
    }
}

pub fn set_click_through(h: HWND, on: bool) {
    unsafe {
        let ex = GetWindowLongW(h, GWL_EXSTYLE);
        let t = WS_EX_TRANSPARENT.0 as i32;
        SetWindowLongW(h, GWL_EXSTYLE, if on { ex | t } else { ex & !t });
    }
}

pub fn show_taskbar(h: HWND, on: bool) {
    unsafe {
        let vis = IsWindowVisible(h).as_bool();
        if on != vis { let _ = ShowWindow(h, if on { SW_SHOWNOACTIVATE } else { SW_HIDE }); }
    }
}

pub fn dpi_scale(h: HWND) -> f32 { unsafe { GetDpiForWindow(h).max(96) as f32 / 96.0 } }

fn rect(h: HWND) -> RECT { let mut r = RECT::default(); unsafe { let _ = GetWindowRect(h, &mut r); } r }

fn work_area(m: HMONITOR) -> RECT {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { let _ = GetMonitorInfoW(m, &mut mi); }
    mi.rcWork
}

fn move_px(h: HWND, x: i32, y: i32) {
    unsafe { let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE); }
}

pub fn save_pos(a: &mut App) { let r = rect(a.hwnd); a.s.left = r.left; a.s.top = r.top; crate::mark_dirty(a); }

pub fn restore_pos(a: &mut App) {
    let p = POINT { x: a.s.left + 20, y: a.s.top + 10 };
    let on_screen = unsafe { !MonitorFromPoint(p, MONITOR_DEFAULTTONULL).is_invalid() };
    if a.s.left != -1 && on_screen { move_px(a.hwnd, a.s.left, a.s.top); }
    else { move_to(a, 0.5, 0.85, Some(unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) })); }
}

/// Place at a fraction of the monitor work area (0 = left/top, 0.5 = centre, 1 = right/bottom). NaN keeps that axis.
pub fn move_to(a: &mut App, fx: f64, fy: f64, mon: Option<HMONITOR>) {
    // draw once so the window has its real size before we centre it
    let r = rect(a.hwnd);
    // before the first draw the window is tiny; use the configured width so centring is right
    let (w, h) = ((r.right - r.left).max((a.s.width as f32 * dpi_scale(a.hwnd)) as i32), (r.bottom - r.top).max(60));
    let wa = work_area(mon.unwrap_or_else(|| unsafe { MonitorFromWindow(a.hwnd, MONITOR_DEFAULTTONEAREST) }));
    let m = 24;
    let x = if fx.is_nan() { r.left } else { wa.left + m + ((wa.right - wa.left - w - 2 * m) as f64 * fx) as i32 };
    let y = if fy.is_nan() { r.top } else { wa.top + m + ((wa.bottom - wa.top - h - 2 * m) as f64 * fy) as i32 };
    move_px(a.hwnd, x, y);
    save_pos(a);
}

pub fn next_monitor(a: &mut App) {
    let mut mons: Vec<HMONITOR> = vec![];
    unsafe extern "system" fn cb(m: HMONITOR, _: HDC, _: *mut RECT, l: LPARAM) -> BOOL {
        (*(l.0 as *mut Vec<HMONITOR>)).push(m);
        TRUE
    }
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut mons as *mut _ as isize)); }
    if mons.len() < 2 { return; }
    let here = unsafe { MonitorFromWindow(a.hwnd, MONITOR_DEFAULTTONEAREST) };
    let i = mons.iter().position(|m| *m == here).unwrap_or(0);
    let r = rect(a.hwnd);
    let wa = work_area(here);
    let fy = ((r.top - wa.top) as f64 / ((wa.bottom - wa.top) - (r.bottom - r.top)).max(1) as f64).clamp(0.0, 1.0);
    move_to(a, 0.5, fy, Some(mons[(i + 1) % mons.len()]));
}

// ---------- taskbar strip ----------
pub fn place_taskbar(tb_hwnd: HWND, taskbar_width: f64, side: &str) {
    unsafe {
        let Ok(tb) = FindWindowW(w!("Shell_TrayWnd"), None) else { return };
        let r = rect(tb);
        if r.right - r.left < r.bottom - r.top { return; } // vertical taskbar: not supported
        let scale = dpi_scale(tb);
        let (h, w) = (r.bottom - r.top, (taskbar_width as f32 * scale) as i32);
        let x = if side == "Left" { r.left + 12 } else {
            let tray = FindWindowExW(Some(tb), None, w!("TrayNotifyWnd"), None).ok();
            tray.map(|t| rect(t).left).unwrap_or(r.right - 300) - w - 8
        };
        // re-assert topmost: the taskbar likes to cover us
        let _ = SetWindowPos(tb_hwnd, Some(HWND_TOPMOST), x, r.top, w, h, SWP_NOACTIVATE);
    }
}

pub fn taskbar_size(a: &App) -> (i32, i32) { let r = rect(a.tb_hwnd); (r.right - r.left, r.bottom - r.top) }

// ---------- tray ----------
fn icon() -> HICON {
    static PNG: &[u8] = include_bytes!("../ui/icon.png");
    unsafe { CreateIconFromResourceEx(PNG, true, 0x00030000, 32, 32, LR_DEFAULTCOLOR).unwrap_or_default() }
}

fn tray_data(h: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: h, uID: TRAY_ID, ..Default::default() }
}

fn copy_w(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

pub fn tray_add(h: HWND) {
    let mut d = tray_data(h);
    d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = icon();
    copy_w(&mut d.szTip, "FloatLyrics");
    unsafe { let _ = Shell_NotifyIconW(NIM_ADD, &d); }
}

pub fn tray_remove(h: HWND) { unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(h)); } }

pub fn tray_tip(a: &App) {
    let mut t = match &a.track { None => "FloatLyrics · nothing playing".to_string(), Some(t) => format!("{} — {}\n{}", t.title, t.artist, a.status) };
    if a.song_offset != 0.0 { t += &format!("\noffset {:+.1}s", a.song_offset); }
    if !a.hotkey_errors.is_empty() { t += &format!("\n{}", a.hotkey_errors); }
    let mut d = tray_data(a.hwnd);
    d.uFlags = NIF_TIP;
    copy_w(&mut d.szTip, &t);
    unsafe { let _ = Shell_NotifyIconW(NIM_MODIFY, &d); }
}

pub fn balloon(h: HWND, title: &str, text: &str) {
    let mut d = tray_data(h);
    d.uFlags = NIF_INFO;
    copy_w(&mut d.szInfoTitle, title);
    copy_w(&mut d.szInfo, text);
    d.dwInfoFlags = NIIF_INFO;
    unsafe { let _ = Shell_NotifyIconW(NIM_MODIFY, &d); }
}

// ---------- autostart (HKCU Run, no admin) ----------
pub fn apply_autostart(on: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"), Some(0), KEY_SET_VALUE, &mut key).is_err() { return; }
        if on {
            let exe = std::env::current_exe().unwrap_or_default();
            let v: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
            let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
            let _ = RegSetValueExW(key, w!("FloatLyrics"), Some(0), REG_SZ, Some(bytes));
        } else {
            let _ = RegDeleteValueW(key, w!("FloatLyrics"));
        }
        let _ = RegCloseKey(key);
    }
}

pub fn open_folder(p: &std::path::Path) {
    let _ = std::fs::create_dir_all(p);
    let _ = std::process::Command::new("explorer.exe").arg(p).spawn();
}

// ---------- window procedure ----------
unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let is_main = with_app(|a| a.hwnd == h).unwrap_or(false);
    match msg {
        WM_NCHITTEST if is_main => {
            // movable unless click-through: the whole window is a drag handle (works across monitors)
            return LRESULT(HTCAPTION as isize);
        }
        WM_NCLBUTTONDBLCLK if is_main => { with_app(|a| move_to(a, 0.5, f64::NAN, None)); return LRESULT(0); }
        WM_NCRBUTTONUP if is_main => { crate::ui::show_menu(); return LRESULT(0); }
        WM_EXITSIZEMOVE if is_main => { with_app(save_pos); }
        WM_HOTKEY => { crate::keys::on_hotkey(wp.0 as i32); return LRESULT(0); }
        WM_TRAY => {
            match (lp.0 as u32) & 0xFFFF {
                x if x == WM_RBUTTONUP || x == WM_CONTEXTMENU => crate::ui::show_menu(),
                x if x == WM_LBUTTONDBLCLK => crate::ui::open_settings(),
                x if x == NIN_BALLOONUSERCLICK => { with_app(crate::update::install_now); }
                _ => {}
            }
            return LRESULT(0);
        }
        _ => {
            if msg == RegisterWindowMessageW(w!("TaskbarCreated")) && is_main { tray_add(h); } // explorer restarted
        }
    }
    DefWindowProcW(h, msg, wp, lp)
}

/// Give unused pages back to Windows (e.g. after the settings window closes).
pub fn trim_memory() {
    unsafe { let _ = windows::Win32::System::ProcessStatus::K32EmptyWorkingSet(windows::Win32::System::Threading::GetCurrentProcess()); }
}
