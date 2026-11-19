// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::app::state::*;
use windows::Win32::Foundation::*;
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
