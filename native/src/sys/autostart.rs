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
use crate::sys::win::*;
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

pub(crate) const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub(crate) const RUN_VALUE: &str = "YuanBaoDeskPet";

pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) unsafe fn autostart_on() -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };
    let mut k = HKEY::default();
    let key_w = to_wide(RUN_KEY);
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        PCWSTR(key_w.as_ptr()),
        0,
        KEY_QUERY_VALUE,
        &mut k,
    )
    .is_err()
    {
        return false;
    }
    let name = to_wide(RUN_VALUE);
    let mut cb: u32 = 0;
    let ok = RegQueryValueExW(
        k,
        PCWSTR(name.as_ptr()),
        None,
        None,
        None,
        Some(&mut cb),
    )
    .is_ok()
        && cb > 0;
    let _ = RegCloseKey(k);
    ok
}

pub(crate) unsafe fn set_autostart(on: bool) -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };
    let mut k = HKEY::default();
    let key_w = to_wide(RUN_KEY);
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        PCWSTR(key_w.as_ptr()),
        0,
        KEY_SET_VALUE,
        &mut k,
    )
    .is_err()
    {
        return false;
    }
    let name = to_wide(RUN_VALUE);
    let ok = if on {
        let exe = std::env::current_exe().unwrap_or_default();
        // 用引号包住，路径带空格也能正确启动
        let val = format!("\"{}\"", exe.display());
        let w: Vec<u16> = val.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes =
            std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * std::mem::size_of::<u16>());
        RegSetValueExW(k, PCWSTR(name.as_ptr()), 0, REG_SZ, Some(bytes)).is_ok()
    } else {
        RegDeleteValueW(k, PCWSTR(name.as_ptr())).is_ok()
    };
    let _ = RegCloseKey(k);
    ok
}
