// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::app::state::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub(crate) unsafe fn screen_size() -> (i32, i32) {
    let mut r = RECT::default();
    let _ = GetClientRect(GetDesktopWindow(), &mut r);
    (r.right, r.bottom)
}

pub(crate) unsafe fn set_locked(hwnd: HWND, locked: bool) {
    let mut style = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
    if locked {
        style |= WS_EX_TRANSPARENT;
    } else {
        style &= !WS_EX_TRANSPARENT;
    }
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style.0 as isize);
    if let Some(st) = ST.get() {
        if let Ok(mut s) = st.lock() {
            s.locked = locked;
        }
    }
}

pub(crate) unsafe fn to_bottom_right(hwnd: HWND) {
    let (sw, sh) = screen_size();
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        sw - win() - 40,
        sh - win() - 100,
        0,
        0,
        SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

pub(crate) fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}
