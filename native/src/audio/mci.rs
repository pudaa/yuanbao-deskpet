//! MCI 命令薄封装。
//!
//! MCI 只吃文件路径、不吃内存，所以资源要先落成临时文件（见 audio/mod.rs）。
//! 这里只做「命令字符串 -> 系统调用 -> 解析返回值」，不含任何业务判断。

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Media::Multimedia::mciSendStringW;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn command(cmd: &str) -> bool {
    let w = wide(cmd);
    unsafe { mciSendStringW(PCWSTR(w.as_ptr()), None, HWND::default()) == 0 }
}

/// 查状态（mode / length 等），失败返回空串
pub fn query(cmd: &str) -> String {
    let w = wide(cmd);
    let mut buf = [0u16; 128];
    let ok = unsafe { mciSendStringW(PCWSTR(w.as_ptr()), Some(&mut buf), HWND::default()) == 0 };
    if !ok {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

pub fn open(alias: &str, path: &std::path::Path) -> bool {
    let _ = command(&format!("close {alias}"));
    command(&format!(
        "open \"{}\" type waveaudio alias {alias}",
        path.to_string_lossy()
    ))
}

pub fn play(alias: &str) -> bool {
    let _ = command(&format!("seek {alias} to start"));
    command(&format!("play {alias}"))
}

pub fn stop(alias: &str) {
    let _ = command(&format!("stop {alias}"));
    let _ = command(&format!("seek {alias} to start"));
}

pub fn mode(alias: &str) -> String {
    query(&format!("status {alias} mode"))
}

/// 0.0–1.0 映射到 MCI 的 0–1000
pub fn set_volume(alias: &str, v: f32) {
    let n = (v.clamp(0.0, 1.0) * 1000.0).round() as i32;
    let _ = command(&format!("setaudio {alias} volume to {n}"));
}
