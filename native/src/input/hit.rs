#![allow(unused_imports, dead_code)]
// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::{PENDING_UPDATE, WM_UPDATE_READY};
use crate::app::cmd::*;
use crate::app::state::*;
use crate::app::tick::*;
use crate::input::follow::*;
use crate::render::frames::*;
use crate::render::present::*;
use crate::render::press::*;
use crate::sys::autostart::*;
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

/// 点是否落在猫的实体像素上（读当前帧的 alpha）
pub(crate) unsafe fn hit_test(s: &mut State, px: i32, py: i32) -> bool {
    s.ensure_frame(s.cur);
    let mut wr = RECT::default();
    let _ = GetWindowRect(s.hwnd, &mut wr);
    let x = px - wr.left;
    let y = py - wr.top;
    if x < 0 || y < 0 || x >= win() || y >= win() {
        return false;
    }
    let idx = (((y * win() + x) * 4) + 3) as usize;
    s.frames
        .get(s.cur)
        .and_then(|f| f.as_ref())
        .and_then(|f| f.bgra.get(idx))
        .copied()
        .unwrap_or(0)
        > hit_alpha()
}
