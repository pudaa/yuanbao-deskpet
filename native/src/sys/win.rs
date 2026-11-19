#![allow(unused_imports, dead_code)]
// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::{PENDING_UPDATE, WM_UPDATE_READY};
use crate::app::cmd::*;
use crate::app::state::*;
use crate::app::tick::*;
use crate::input::follow::*;
use crate::input::hit::*;
use crate::render::frames::*;
use crate::render::present::*;
use crate::render::press::*;
use crate::sys::autostart::*;
use crate::ui::menu::*;
use crate::ui::{menu_ui, tray};
use crate::{audio, config, physics, settings, update};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
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
