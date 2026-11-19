//! 系统托盘：图标 + 右键菜单（设置 / 显示隐藏 / 退出）

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::ui::menu_ui::{self as mui, Kind};

pub const WM_TRAY: u32 = WM_APP + 1;

const ID_SETTINGS: usize = 1;
const ID_TOGGLE: usize = 2;
const ID_QUIT: usize = 3;
const ID_LOCK: usize = 4;
const TRAY_ID: u32 = 1;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn assets_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..6 {
            let Some(d) = dir else { break };
            let cand = d.join("assets");
            if cand.is_dir() {
                return cand;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    std::path::PathBuf::from("assets")
}

unsafe fn load_icon() -> HICON {
    // 图标已嵌进 exe 的 RT_GROUP_ICON（name=1），直接从自身模块加载，
    // 这样把 exe 单独复制到桌面也能显示托盘图标。
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    if let Ok(h) = LoadImageW(
        hinst,
        PCWSTR(1usize as *const u16),
        IMAGE_ICON,
        0,
        0,
        LR_DEFAULTSIZE,
    ) {
        return HICON(h.0);
    }
    // 开发期兜底：资源缺失时读 assets/
    let p = assets_dir().join("icon.ico");
    let wpath = wide(&p.to_string_lossy());
    match LoadImageW(
        None,
        PCWSTR(wpath.as_ptr()),
        IMAGE_ICON,
        0,
        0,
        LR_LOADFROMFILE | LR_DEFAULTSIZE,
    ) {
        Ok(h) => HICON(h.0),
        Err(_) => HICON::default(),
    }
}

pub unsafe fn add(hwnd: HWND) -> bool {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: load_icon(),
        ..Default::default()
    };
    let tip: Vec<u16> = "圆宝桌宠".encode_utf16().collect();
    for (i, c) in tip.iter().take(127).enumerate() {
        nid.szTip[i] = *c;
    }
    Shell_NotifyIconW(NIM_ADD, &nid).as_bool()
}

pub unsafe fn remove() {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND::default(),
        uID: TRAY_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

pub enum Action {
    Settings,
    ToggleVisible,
    /// 穿透（锁定）后猫窗口收不到鼠标，右键菜单就打不开了，
    /// 托盘是唯一的救援入口，必须能在这里解开。
    ToggleLock,
    Quit,
    None,
}

/// 弹出托盘菜单并返回用户选择
pub unsafe fn show_menu(hwnd: HWND, visible: bool, locked: bool) -> Action {
    let Ok(menu) = CreatePopupMenu() else {
        return Action::None;
    };

    let toggle_label = if visible { "隐藏圆宝" } else { "显示圆宝" };
    mui::begin();
    mui::add(menu, 0, "圆宝桌宠", Kind::Header);
    mui::add(menu, ID_SETTINGS as u32, "设置", Kind::Plain);
    mui::add(menu, ID_TOGGLE as u32, toggle_label, Kind::Plain);
    mui::add(
        menu,
        ID_LOCK as u32,
        if locked { "解锁（可拖动）" } else { "锁定（穿透）" },
        Kind::Plain,
    );
    mui::add(menu, 0, "", Kind::Separator);
    mui::add(menu, ID_QUIT as u32, "圆宝再见", Kind::Plain);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    // TrackPopupMenu 需要窗口在前台，否则点别处菜单不消失
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);

    match cmd.0 as usize {
        ID_SETTINGS => Action::Settings,
        ID_TOGGLE => Action::ToggleVisible,
        ID_LOCK => Action::ToggleLock,
        ID_QUIT => Action::Quit,
        _ => Action::None,
    }
}
