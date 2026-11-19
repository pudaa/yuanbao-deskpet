// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::app::state::*;
use crate::render::frames::*;
use crate::render::present::*;
use crate::sys::win::*;
use crate::{audio, config, settings};
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 把当前设置与窗口位置写回 config.json（调用方不能已持有 ST 锁）
pub(crate) unsafe fn save_config() {
    let Some(st) = ST.get() else { return };
    let Ok(s) = st.lock() else { return };
    let mut wr = RECT::default();
    let _ = GetWindowRect(s.hwnd, &mut wr);
    config::save(&config::Config {
        size: disp(),
        opacity: s.opacity,
        mirror: s.mirror,
        sound: audio::current_pack_id().to_string(),
        x: Some(wr.left),
        y: Some(wr.top),
    });
}

/// 设置面板通过 IPC 调用的命令分发
pub unsafe fn apply_cmd(cmd: &str, args: &serde_json::Value) -> serde_json::Value {
    let Some(st) = ST.get() else {
        return serde_json::Value::Null;
    };
    let Ok(mut s) = st.lock() else {
        return serde_json::Value::Null;
    };
    let num = |k: &str| args.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let flag = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);

    match cmd {
        "get_config" => serde_json::json!({
            "size": disp() as f64,
            "opacity": s.opacity,
            "smoothing": 1.0,
            "idleRadius": s.idle_radius,
            "locked": s.locked,
            "mirror": s.mirror,
            "sound": audio::current_pack_id(),
            "alwaysOnTop": true,
            "autostart": false,
            "theme": "fresh",
            "visible": true,
            "x": 0,
            "y": 0,
        }),
        "set_opacity" => {
            let oc = &settings::get().opacity;
            s.opacity = (num("value") / 100.0).clamp(oc.min as f64 / 100.0, oc.max as f64 / 100.0);
            serde_json::Value::Null
        }
        "set_mirror" => {
            s.mirror = flag("value");
            serde_json::Value::Null
        }
        "set_idle_radius" => {
            s.idle_radius = num("value").clamp(8.0, 120.0);
            serde_json::Value::Null
        }
        "set_smoothing" => serde_json::Value::Null,
        "set_sound" => {
            let id = args.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let _ = audio::set_pack(id);
            serde_json::Value::Null
        }
        "set_size" => {
            let sc = &settings::get().size;
            let v = num("size").clamp(sc.min as f64, sc.max as f64) as i32;
            if v != disp() {
                DISPLAY_I.store(v, Ordering::Relaxed);
                let hwnd = s.hwnd;
                drop(s); // 重新加载很慢，先让出锁

                if let Some(st) = ST.get() {
                    if let Ok(mut s2) = st.lock() {
                        // 帧的缩放尺寸变了，全部重载
                        for f in s2.frames.iter_mut() {
                            *f = None;
                        }
                        for i in 0..frame_count() {
                            s2.frames[i] = load_one(i);
                        }
                        s2.dib = None;
                        ensure_dib_locked(&mut s2);
                        s2.last_cur = usize::MAX;
                        s2.last_frame = None;
                    }
                }

                // 保持右下角锚点，按新尺寸重新摆放
                let w = win();
                let (sw, sh) = screen_size();
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    sw - w - 40,
                    sh - w - 100,
                    w,
                    w,
                    SWP_NOACTIVATE,
                );
            }
            serde_json::Value::Null
        }
        "set_locked" => {
            let v = flag("value");
            let hwnd = s.hwnd;
            s.locked = v;
            drop(s);
            set_locked(hwnd, v);
            serde_json::Value::Null
        }
        "place_corner" => {
            let hwnd = s.hwnd;
            drop(s);
            to_bottom_right(hwnd);
            serde_json::Value::Null
        }
        "reset_all" => {
            // 原来这里只回一份默认值 JSON、不真正改状态，菜单点「恢复默认」等于没反应。
            // 现在先放锁再逐项下发，复用各条命令自己的重载/重排逻辑。
            drop(s);
            let d = settings::get();
            let _ = apply_cmd("set_size", &serde_json::json!({ "size": d.size.default_value }));
            let _ = apply_cmd("set_opacity", &serde_json::json!({ "value": d.opacity.default_value }));
            let _ = apply_cmd("set_mirror", &serde_json::json!({ "value": false }));
            let _ = apply_cmd("set_sound", &serde_json::json!({ "value": d.audio.default_pack }));
            let _ = apply_cmd("set_locked", &serde_json::json!({ "value": false }));
            let _ = apply_cmd("place_corner", &serde_json::Value::Null);
            serde_json::Value::Null
        }
        "quit_app" => {
            let hwnd = s.hwnd;
            drop(s);
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            serde_json::Value::Null
        }
        // 未识别的命令直接返回成功，避免旧调用方报错
        _ => serde_json::Value::Null,
    }
}
