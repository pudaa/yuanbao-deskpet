#![allow(unused_imports, dead_code)]
// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::{PENDING_UPDATE, WM_UPDATE_READY};
use crate::app::cmd::*;
use crate::app::state::*;
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

/// 每 16ms 一次：物理 → 角度 → 绘制（全程只锁一次，避免重入死锁）
pub(crate) unsafe fn on_tick() {
    let Some(st) = ST.get() else { return };
    let Ok(mut s) = st.lock() else { return };

    let now = std::time::Instant::now();
    let dt = now.duration_since(s.last_tick).as_secs_f64();
    s.last_tick = now;

    // 物理只算位置，不单独 SetWindowPos —— 位置通过 UpdateLayeredWindow 的 pptDst
    // 一并提交，省掉一半的系统调用（之前甩出时每帧两次调用，明显掉帧）。
    let mut body_pos: Option<(i32, i32)> = None;
    let mut land_hit = false;
    let mut land_impact = 0.0;
    if s.body.is_some() {
        let (sw, sh) = screen_size();
        let bounds = physics::Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: (sw - win()) as f64,
            max_y: (sh - win()) as f64,
        };
        if let Some(body) = s.body.as_mut() {
            let r = body.step(dt, bounds);
            body_pos = Some((body.x as i32, body.y as i32));
            // Web 版 pet:landed 用 280 做阈值，这里保持一致
            land_hit = r.landed && r.impact > 280.0;
            land_impact = r.impact;
            if r.at_rest {
                s.body = None;
            }
        }
    }
    // 贴底时 landed 每帧都为 true，必须只在动画未进行时触发，
    // 否则 Land 会被不断重置、动画永不结束、每帧强制重绘。
    if land_hit && !matches!(s.press, Press::Land(..) | Press::Bounce(_)) {
        s.press = Press::Land(0.0, impact_depth(land_impact));
    }

    // 轨迹改为按时间采样（而不是只在 mousemove 时记）：
    // 否则停住手之后轨迹里还留着"快速拖动"那一段，松手会被判成高速甩出（弹弓效应）。
    if s.dragging {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            let t = now_ms();
            s.trail.push((pt.x as f64, pt.y as f64, t));
            if s.trail.len() > 32 {
                s.trail.remove(0);
            }
        }
    }

    // 音效行为的时间推进：延迟触发、循环续播、淡出
    audio::tick();

    // 按压动画按 Web 版关键帧推进
    s.tick_press(dt);
    let scale = press_scale(s.press);

    if !s.dragging && s.body.is_some() {
        // 甩出飞行途中也要继续看着鼠标（Web 版就是一直跟的）
        s.in_idle = false;
        update_frame(&mut s);
    } else if !s.dragging && s.body.is_none() {
        // 鼠标贴到旋转中心附近时收势（低头），离开后恢复跟随。
        // 滞回：进 28px、出 48px，避免边界抖动导致反复切换。
        let mut wr = RECT::default();
        let _ = GetWindowRect(s.hwnd, &mut wr);
        let cx = (wr.left + wr.right) as f64 / 2.0;
        let cy = wr.top as f64 + (wr.bottom - wr.top) as f64 * focus_y();
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            let dx = pt.x as f64 - cx;
            let dy = pt.y as f64 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let r = s.idle_radius;
            if d < r {
                s.in_idle = true;
            } else if d > r * idle_exit_factor() {
                s.in_idle = false;
            }
        }

        if s.in_idle {
            // 回归路径要走环形最短方向，不能一直往下递减。
            // 主环是 32..360（32=上看，顺时针递增，360 与 32 相邻）；
            // 例如从左上(≈319)顺时针到 32 只有 41 帧，逆时针要 287 帧。
            let start = main_start();
            let last = frame_count() - 1;
            let step = idle_drift();
            if s.cur <= start {
                // 已到"上看"，开始低头（帧 31..0 是抬头动画的反向）
                s.cur = s.cur.saturating_sub(step);
            } else {
                let dist_cw = last - s.cur; // 顺时针：cur -> last(=start)
                let dist_ccw = s.cur - start; // 逆时针：cur -> start
                if dist_cw <= dist_ccw {
                    s.cur = if s.cur >= last {
                        start
                    } else {
                        (s.cur + step).min(last)
                    };
                } else {
                    s.cur = s.cur.saturating_sub(step).max(start);
                }
            }
        } else {
            update_frame(&mut s);
        }
    }
    // 按需重绘：帧号 / 挤压值 / 窗口位置 / 透明度 / 镜像 都没变就跳过。
    // 桌宠大部分时间静止，16ms 无条件 present 等于白烧 CPU。
    let pos_now = match body_pos {
        Some(p) => p,
        None => {
            let mut wr2 = RECT::default();
            let _ = GetWindowRect(s.hwnd, &mut wr2);
            (wr2.left, wr2.top)
        }
    };
    // 动画期间强制每帧重绘：回弹后半段每帧 scale 变化可能小于阈值，
    // 若走按需跳过会让动画提前静止，观感就是"后面突然结束"。
    let animating = !matches!(s.press, Press::Idle);
    let changed = animating
        || s.cur != s.last_cur
        || (scale.0 - s.last_sx).abs() > 0.002
        || (scale.1 - s.last_sy).abs() > 0.002
        || pos_now != s.last_pos
        || (s.opacity - s.last_op).abs() > 0.001
        || s.mirror != s.last_mirror;

    if changed {
        present_locked_at(&mut s, body_pos, scale);
        s.last_cur = s.cur;
        s.last_sx = scale.0;
        s.last_sy = scale.1;
        s.last_pos = pos_now;
        s.last_op = s.opacity;
        s.last_mirror = s.mirror;
    }

    // 不再用 SetWindowRgn 裁剪窗口：按压压扁时猫会横向撑开，
    // 超出旧轮廓就会被切掉。透明区的点击由 hit_test 的 alpha 判定处理。
}
