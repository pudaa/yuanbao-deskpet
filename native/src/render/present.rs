// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use std::ffi::c_void;
use crate::app::state::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub(crate) unsafe fn ensure_dib_locked(s: &mut State) {
    if s.dib.is_some() {
        return;
    }
    let screen = GetDC(HWND::default());
    let memdc = CreateCompatibleDC(screen);
    let _ = ReleaseDC(HWND::default(), screen);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: win(),
            biHeight: -win(), // 负值 = 自上而下
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut c_void = std::ptr::null_mut();
    let Ok(bmp) = CreateDIBSection(memdc, &bmi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0) else {
        return;
    };
    SelectObject(memdc, HGDIOBJ(bmp.0));
    s.dib = Some(Dib {
        memdc,
        bmp,
        bits: bits as *mut u32,
    });
}

/// 把当前帧贴到分层窗口（调用方已持有锁）
pub(crate) unsafe fn present_locked_at(
    s: &mut State,
    pos_override: Option<(i32, i32)>,
    scale: (f64, f64),
) {
    let cur = s.cur;
    s.ensure_frame(cur);
    let idx = if s.frames[cur].is_some() {
        cur
    } else {
        s.last_frame.unwrap_or(cur)
    };

    let Some(dib) = s.dib.as_ref() else { return };
    let (sx, sy) = scale;
    let op_set = s.opacity;
    let mirror_set = s.mirror;
    let Some(f) = s.frames.get(idx).and_then(|x| x.as_ref()) else {
        return;
    };

    if (sx - 1.0).abs() < 0.002 && (sy - 1.0).abs() < 0.002 {
        let op = op_set.clamp(0.2, 1.0);
        if (op - 1.0).abs() < 0.004 && !mirror_set {
            std::ptr::copy_nonoverlapping(
                f.bgra.as_ptr() as *const u32,
                dib.bits,
                (win() * win()) as usize,
            );
        } else {
            unsafe {
                let dst = std::slice::from_raw_parts_mut(dib.bits as *mut u8, (win() * win() * 4) as usize);
                dst.copy_from_slice(&f.bgra);
                if mirror_set {
                    for y in 0..win() {
                        let row = (y * win() * 4) as usize;
                        for x in 0..(win() / 2) {
                            let a = row + (x * 4) as usize;
                            let b = row + ((win() - 1 - x) * 4) as usize;
                            for k in 0..4 {
                                dst.swap(a + k, b + k);
                            }
                        }
                    }
                }
                if (op - 1.0).abs() >= 0.004 {
                    for px in dst.chunks_exact_mut(4) {
                        px[0] = (px[0] as f64 * op) as u8;
                        px[1] = (px[1] as f64 * op) as u8;
                        px[2] = (px[2] as f64 * op) as u8;
                        px[3] = (px[3] as f64 * op) as u8;
                    }
                }
            }
        }
    } else {
        // 在 CPU 侧把内容压扁（底部对齐），窗口位置保持不动。
        // 直接改 UpdateLayeredWindow 的目标矩形等于每帧把窗口往下挪，会累积漂移。
        let sx = sx.max(0.2);
        let sy = sy.max(0.2);
        let dw = (win() as f64 * sx).round() as i32;
        let dh = (win() as f64 * sy).round() as i32;
        let ox = (win() - dw) / 2;
        let oy = win() - dh;
        let src = &f.bgra;
        unsafe {
            let dst = std::slice::from_raw_parts_mut(dib.bits as *mut u8, (win() * win() * 4) as usize);
            dst.fill(0);
            for y in 0..dh {
                // 横向撑开后 ox 可能为负、纵向也可能越界，必须逐点裁剪，
                // 否则会写出画布导致进程直接崩掉。
                let dy = y + oy;
                if dy < 0 || dy >= win() {
                    continue;
                }
                let cy = ((y as f64 + 0.5) / sy - 0.5).clamp(0.0, (win() - 1) as f64) as i32;
                for x in 0..dw {
                    let dx = x + ox;
                    if dx < 0 || dx >= win() {
                        continue;
                    }
                    let cx = ((x as f64 + 0.5) / sx - 0.5).clamp(0.0, (win() - 1) as f64) as i32;
                    let si = (((cy * win()) + cx) * 4) as usize;
                    let di = (((dy * win()) + dx) * 4) as usize;
                    dst[di] = src[si];
                    dst[di + 1] = src[si + 1];
                    dst[di + 2] = src[si + 2];
                    dst[di + 3] = src[si + 3];
                }
            }
        }
    }

    let pos = match pos_override {
        Some((x, y)) => POINT { x, y },
        None => {
            let mut wr = RECT::default();
            let _ = GetWindowRect(s.hwnd, &mut wr);
            POINT {
                x: wr.left,
                y: wr.top,
            }
        }
    };
    let size = SIZE {
        cx: win(),
        cy: win(),
    };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: 0, // AC_SRC_OVER
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: 1, // AC_SRC_ALPHA
    };

    let screen = GetDC(HWND::default());
    let _ = UpdateLayeredWindow(
        s.hwnd,
        screen,
        Some(&pos),
        Some(&size),
        dib.memdc,
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    let _ = ReleaseDC(HWND::default(), screen);
    s.last_frame = Some(idx);
}
