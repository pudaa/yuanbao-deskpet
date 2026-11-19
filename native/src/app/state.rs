#![allow(unused_imports, dead_code)]
// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::{PENDING_UPDATE, WM_UPDATE_READY};
use crate::app::cmd::*;
use crate::app::tick::*;
use crate::input::follow::*;
use crate::input::hit::*;
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

// ---- 素材与几何参数：一律来自 assets/settings.json，代码不持有字面量 ----
pub(crate) fn frame_count() -> usize {
    settings::get().frames.count
}

pub(crate) fn main_start() -> usize {
    settings::get().frames.main_start
}

pub(crate) fn pad() -> i32 {
    settings::get().geometry.pad
}

pub(crate) fn focus_y() -> f64 {
    settings::get().geometry.focus_y
}

pub(crate) fn idle_drift() -> usize {
    settings::get().frames.idle_drift_frames
}

pub(crate) fn idle_enter() -> f64 {
    settings::get().geometry.idle_enter
}

pub(crate) fn idle_exit_factor() -> f64 {
    settings::get().geometry.idle_exit_factor
}

pub(crate) fn hit_alpha() -> u8 {
    settings::get().geometry.hit_alpha
}

/// 猫的逻辑显示边长（设置面板可改）
pub(crate) static DISPLAY_I: AtomicI32 = AtomicI32::new(0);

pub(crate) fn disp() -> i32 {
    DISPLAY_I.load(Ordering::Relaxed)
}

pub(crate) fn win() -> i32 {
    disp() + pad() * 2
}

pub(crate) struct Frame {
    /// 预乘 alpha 的 BGRA，尺寸 win()×win()
    pub(crate) bgra: Vec<u8>,
}

pub(crate) struct Dib {
    pub(crate) memdc: HDC,
    /// 只是持有句柄保证位图不被回收，代码里不直接读
    #[allow(dead_code)]
    pub(crate) bmp: HBITMAP,
    pub(crate) bits: *mut u32,
}

pub(crate) struct State {
    /// 按需填充的帧槽位：未加载为 None（省内存），远处帧会被淘汰
    pub(crate) frames: Vec<Option<Frame>>,
    pub(crate) cur: usize,
    pub(crate) dib: Option<Dib>,
    pub(crate) hwnd: HWND,

    // 拖动 / 甩出
    pub(crate) dragging: bool,
    pub(crate) grab_dx: i32,
    pub(crate) grab_dy: i32,
    pub(crate) trail: Vec<(f64, f64, f64)>, // 光标轨迹 (x, y, t_ms)
    pub(crate) body: Option<physics::Body>,
    pub(crate) last_tick: std::time::Instant,

    // 按压动画阶段（关键帧曲线对齐 Web 版）
    pub(crate) press: Press,
    pub(crate) pressed_hit: bool,
    pub(crate) press_win: (i32, i32),
    pub(crate) press_cursor: (i32, i32),
    pub(crate) in_idle: bool,
    pub(crate) locked: bool,

    // 可调参数（设置面板通过 IPC 改）
    pub(crate) opacity: f64,
    pub(crate) mirror: bool,
    pub(crate) idle_radius: f64,

    // 上一帧的"已绘制状态"快照，用于跳过无变化的重绘
    pub(crate) last_cur: usize,
    pub(crate) last_sx: f64,
    pub(crate) last_sy: f64,
    pub(crate) last_pos: (i32, i32),
    pub(crate) last_op: f64,
    pub(crate) last_mirror: bool,
    /// 最近一次成功绘制的帧号（目标帧未就绪时兜底）
    pub(crate) last_frame: Option<usize>,
}

unsafe impl Send for State {}

pub(crate) static ST: OnceLock<Mutex<State>> = OnceLock::new();

pub(crate) fn s_opacity() -> f64 {
    ST.get()
        .and_then(|st| st.lock().ok().map(|g| g.opacity))
        .unwrap_or(1.0)
}

pub(crate) fn s_mirror() -> bool {
    ST.get()
        .and_then(|st| st.lock().ok().map(|g| g.mirror))
        .unwrap_or(false)
}
