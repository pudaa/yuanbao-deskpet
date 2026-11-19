//! 自更新：GitHub Releases 检查 + 下载 + 自替换。
//!
//! 设计要点：
//! - HTTP 用 **WinINet**，`INTERNET_OPEN_TYPE_PRECONFIG` 会自动继承系统/IE 代理，
//!   先代理后直连的两级尝试正好覆盖国内常见网络环境。
//! - 自替换不用 .bat：bat 里写中文路径会撞上 GBK/UTF-8 编码问题。
//!   改成把**自身**复制一份到临时目录当更新器，带 `--apply-update` 参数运行，
//!   它是原生 exe，路径怎么传都不会乱码。

use std::path::PathBuf;
use std::time::Duration;

/// 编译期常量，仅作读不到 PE 资源时的兜底
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "pudaa/yuanbao-deskpet";
const UA: &str = "yuanbao-deskpet";

/// 从自身 PE 的 VERSIONINFO 读版本（如 "1.0.1.0"）。
/// 优先用它：exe 被更新器换成新文件后，新文件自己就知道版本，
/// 不会出现"以为自己是旧版、反复提示更新到自己"。
pub fn current_version() -> String {
    pe_version().unwrap_or_else(|| VERSION.to_string())
}

fn pe_version() -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    unsafe {
        let path = std::env::current_exe().ok()?;
        let w: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(w.as_ptr()), 0, size, buf.as_mut_ptr() as *mut _).ok()?;

        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        let root: Vec<u16> = vec![u16::from(b'\\'), 0];
        if !VerQueryValueW(
            buf.as_ptr() as *const _,
            PCWSTR(root.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || ptr.is_null()
            || len < 16
        {
            return None;
        }
        // VS_FIXEDFILEINFO: offset 8 = dwFileVersionMS, 12 = dwFileVersionLS
        let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
        let ms = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        let ls = u32::from_le_bytes(bytes[12..16].try_into().ok()?);
        let full = format!(
            "{}.{}.{}.{}",
            ms >> 16,
            ms & 0xFFFF,
            ls >> 16,
            ls & 0xFFFF
        );
        // 四段太啰嗦，末尾的 .0 去掉（1.0.2.0 → 1.0.2）
        Some(full.strip_suffix(".0").map(str::to_string).unwrap_or(full))
    }
}

#[derive(Clone)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub asset_url: String,
    pub page_url: String,
}

/// 拉取 latest release；有更新返回 Some，没有返回 Ok(None)
pub fn check() -> Result<Option<Release>, String> {
    let body = http_get(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))?;
    let v: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("JSON 解析失败: {e}"))?;

    // GitHub 用 200 + {"message":"Not Found"} 表示没有 release
    if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
        return Err(if msg == "Not Found" {
            "该仓库还没有发布 release".to_string()
        } else {
            format!("GitHub 返回：{msg}")
        });
    }
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    if tag.is_empty() {
        return Err("release 缺少 tag_name".into());
    }
    if !is_newer(&tag, &current_version()) {
        return Ok(None);
    }

    // 取第一个 .exe 资产，不管它叫什么（上传时命名自由）
    let mut asset_url = String::new();
    if let Some(arr) = v["assets"].as_array() {
        for a in arr {
            let name = a["name"].as_str().unwrap_or("").to_lowercase();
            if name.ends_with(".exe") {
                asset_url = a["browser_download_url"].as_str().unwrap_or("").to_string();
                break;
            }
        }
    }
    if asset_url.is_empty() {
        return Err("该 release 里没有 .exe 资产".into());
    }

    Ok(Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        asset_url,
        page_url: v["html_url"].as_str().unwrap_or("").to_string(),
    }))
}

fn parse_ver(s: &str) -> Vec<u32> {
    s.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|x| x.trim().parse::<u32>().unwrap_or(0))
        .collect()
}

fn is_newer(remote: &str, local: &str) -> bool {
    let a = parse_ver(remote);
    let b = parse_ver(local);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

// ---- HTTP（WinINet，两级回退）----

fn http_get(url: &str) -> Result<Vec<u8>, String> {
    // 第一次走系统代理（PRECONFIG 会读 IE/WinINet 配置），失败再强制直连
    match http_get_with(url, true) {
        Ok(v) => Ok(v),
        Err(e1) => match http_get_with(url, false) {
            Ok(v) => Ok(v),
            Err(e2) => Err(format!("代理失败({e1})；直连失败({e2})")),
        },
    }
}

fn http_get_with(url: &str, use_proxy: bool) -> Result<Vec<u8>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinInet::{
        InternetCloseHandle, InternetOpenUrlW, InternetOpenW, InternetReadFile, InternetSetOptionW,
        INTERNET_OPTION_CONNECT_TIMEOUT, INTERNET_OPTION_RECEIVE_TIMEOUT,
    };

    let agent: Vec<u16> = UA.encode_utf16().chain(std::iter::once(0)).collect();
    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    // 0 = PRECONFIG（继承系统代理），1 = DIRECT
    let accesstype: u32 = if use_proxy { 0 } else { 1 };

    unsafe {
        let session = InternetOpenW(
            PCWSTR(agent.as_ptr()),
            accesstype,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err("InternetOpen 失败".into());
        }

        // 默认超时是 60s，网络不通时会把 UI 卡死；收紧到 5s / 8s
        let conn_ms: u32 = 5000;
        let recv_ms: u32 = 8000;
        let _ = InternetSetOptionW(
            Some(session),
            INTERNET_OPTION_CONNECT_TIMEOUT,
            Some(&conn_ms as *const u32 as *const _),
            4,
        );
        let _ = InternetSetOptionW(
            Some(session),
            INTERNET_OPTION_RECEIVE_TIMEOUT,
            Some(&recv_ms as *const u32 as *const _),
            4,
        );

        let req = InternetOpenUrlW(
            session,
            PCWSTR(url_w.as_ptr()),
            None,
            0x8000_0000 | 0x0080_0000, // RELOAD | SECURE
            0,
        );
        if req.is_null() {
            let _ = InternetCloseHandle(session);
            return Err(
                "无法访问 GitHub（网络/代理不通，或该仓库还没有发布 release）".into(),
            );
        }

        let mut out: Vec<u8> = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            let mut read: u32 = 0;
            if InternetReadFile(
                req,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut read,
            )
            .is_err()
            {
                let _ = InternetCloseHandle(req);
                let _ = InternetCloseHandle(session);
                return Err("InternetReadFile 失败".into());
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
        }
        let _ = InternetCloseHandle(req);
        let _ = InternetCloseHandle(session);

        if out.is_empty() {
            return Err("响应为空".into());
        }
        Ok(out)
    }
}

// ---- 下载 + 自替换 ----

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// 下载新版本，启动更新器，返回后调用方应尽快退出
pub fn apply(release: &Release) -> Result<(), String> {
    let bytes = http_get(&release.asset_url)?;
    if bytes.len() < 1024 * 1024 {
        return Err("下载内容异常（体积过小，可能不是 exe）".into());
    }
    let new_exe = temp_path("yuanbao_new.exe");
    std::fs::write(&new_exe, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;

    let me = std::env::current_exe().map_err(|e| format!("取自身路径失败: {e}"))?;
    let updater = temp_path("yuanbao_updater.exe");
    std::fs::copy(&me, &updater).map_err(|e| format!("复制更新器失败: {e}"))?;

    std::process::Command::new(&updater)
        .arg("--apply-update")
        .arg(&new_exe)
        .arg(&me)
        .spawn()
        .map_err(|e| format!("启动更新器失败: {e}"))?;
    Ok(())
}

/// 更新器模式：等旧进程退出 → 覆盖 → 启动新版 → 清理
pub fn run_updater(args: &[String]) -> ! {
    if args.len() < 2 {
        std::process::exit(2);
    }
    let src = PathBuf::from(&args[0]);
    let dst = PathBuf::from(&args[1]);

    // 等到目标文件不再被占用（旧进程退出）
    for _ in 0..600 {
        match std::fs::OpenOptions::new().write(true).open(&dst) {
            Ok(_) => break,
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }

    for _ in 0..30 {
        if std::fs::copy(&src, &dst).is_ok() {
            let _ = std::process::Command::new(&dst).spawn();
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let _ = std::fs::remove_file(&src);
    std::process::exit(0);
}
