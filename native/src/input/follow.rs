// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::app::state::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 按鼠标方位角换算帧号（调用方已持有锁）
pub(crate) unsafe fn update_frame(s: &mut State) {
    let mut wr = RECT::default();
    let _ = GetWindowRect(s.hwnd, &mut wr);
    let cx = (wr.left + wr.right) as f64 / 2.0;
    let cy = wr.top as f64 + (wr.bottom - wr.top) as f64 * focus_y();

    let mut pt = POINT::default();
    if GetCursorPos(&mut pt).is_err() {
        return;
    }

    let dx = pt.x as f64 - cx;
    let dy = pt.y as f64 - cy;
    if (dx * dx + dy * dy).sqrt() < 20.0 {
        return; // 贴太近时角度没意义，保持当前帧
    }

    let mut deg = ((dy.atan2(dx).to_degrees() + 90.0) % 360.0 + 360.0) % 360.0;
    // 水平镜像时画面是翻转的，角度要同步取反，否则猫会朝相反方向看
    if s.mirror {
        deg = (360.0 - deg) % 360.0;
    }
    let start = main_start();
    let last = frame_count() - 1;
    let pos = start as f64 + deg / 360.0 * (last - start) as f64;
    s.cur = (pos.round() as usize).clamp(0, last);
}
