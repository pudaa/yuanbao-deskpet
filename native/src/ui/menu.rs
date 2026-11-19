// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::PENDING_UPDATE;
use crate::app::cmd::*;
use crate::app::state::*;
use crate::sys::autostart::*;
use crate::ui::menu_ui;
use crate::{audio, settings, update};
use std::sync::{Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 设置菜单里的一项动作。
///
/// 命令 ID 不再手写：构建菜单时按顺序分配并登记，wndproc 只查表。
/// 这样「加一个尺寸档/加一个音效组」不会再把命令 ID 写到分发范围之外。
#[derive(Clone)]
pub(crate) enum MenuCmd {
    Size(i32),
    Opacity(i32),
    Mirror,
    Autostart,
    Reset,
    Sound(String),
    /// 总静音
    Mute,
    /// 单条行为开关，值形如 "meow.purr"
    Behavior(String),
}

/// 本轮菜单的命令表。菜单是同步弹出的，重开时整体重建。
pub(crate) static MENU_CMDS: OnceLock<Mutex<Vec<(u16, MenuCmd)>>> = OnceLock::new();

/// 设置项的 ID 命名空间（主菜单用 11/12/13/20/30，不冲突即可）
pub(crate) const MENU_ID_BASE: u16 = 100;

pub(crate) fn menu_cmds() -> &'static Mutex<Vec<(u16, MenuCmd)>> {
    MENU_CMDS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 设置子菜单：自绘，带分组标题与主题色单选圆点。
/// 档位与音效组都来自配置，命令 ID 现场分配并登记。
pub(crate) unsafe fn settings_submenu() -> HMENU {
    let m = CreatePopupMenu().unwrap_or_default();
    let cfg = settings::get();
    let cur_size = disp();
    let cur_op = (s_opacity() * 100.0).round() as i32;
    let mirror = s_mirror();
    let current_sound = audio::current_pack_id().to_string();

    let mut reg = match menu_cmds().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    reg.clear();
    let mut next = MENU_ID_BASE;
    let mut push = |cmd: MenuCmd| -> u16 {
        let id = next;
        next += 1;
        reg.push((id, cmd));
        id
    };

    menu_ui::add(m, 0, "尺寸", menu_ui::Kind::Header);
    for &v in &cfg.size.options {
        let id = push(MenuCmd::Size(v));
        menu_ui::add(m, id as u32, &format!("{v} px"), menu_ui::Kind::Radio(v == cur_size));
    }
    menu_ui::add(m, 0, "", menu_ui::Kind::Separator);
    menu_ui::add(m, 0, "不透明度", menu_ui::Kind::Header);
    for &v in &cfg.opacity.options {
        let id = push(MenuCmd::Opacity(v));
        menu_ui::add(m, id as u32, &format!("{v}%"), menu_ui::Kind::Radio(v == cur_op));
    }
    menu_ui::add(m, 0, "", menu_ui::Kind::Separator);
    menu_ui::add(m, 0, "显示", menu_ui::Kind::Header);
    let id = push(MenuCmd::Mirror);
    menu_ui::add(m, id as u32, "水平镜像", menu_ui::Kind::Check(mirror));
    let id = push(MenuCmd::Autostart);
    menu_ui::add(m, id as u32, "开机自启", menu_ui::Kind::Check(autostart_on()));
    menu_ui::add(m, 0, "", menu_ui::Kind::Separator);
    menu_ui::add(m, 0, "音效", menu_ui::Kind::Header);
    for pack in audio::packs() {
        let id = push(MenuCmd::Sound(pack.id.clone()));
        let checked = pack.id == current_sound;
        menu_ui::add(m, id as u32, &pack.label, menu_ui::Kind::Radio(checked));
    }
    // 当前组的行为开关：关掉「长按呼噜」不影响「叫一声」
    menu_ui::add(m, 0, "", menu_ui::Kind::Separator);
    let id = push(MenuCmd::Mute);
    menu_ui::add(m, id as u32, "全部静音", menu_ui::Kind::Check(audio::muted()));
    for (key, label, on) in audio::current_behaviors() {
        let id = push(MenuCmd::Behavior(key));
        menu_ui::add(m, id as u32, label, menu_ui::Kind::Check(on));
    }
    menu_ui::add(m, 0, "", menu_ui::Kind::Separator);
    let id = push(MenuCmd::Reset);
    menu_ui::add(m, id as u32, "恢复默认", menu_ui::Kind::Normal);
    m
}

/// 设置项分发：只查表，不认识就返回 false。
/// 命令 ID 由 settings_submenu() 现场分配，这里不再依赖任何区间字面量。
pub(crate) unsafe fn handle_menu_cmd(cmd: u16) -> bool {
    let action = menu_cmds()
        .lock()
        .ok()
        .and_then(|reg| reg.iter().find(|(id, _)| *id == cmd).map(|(_, a)| a.clone()));
    let Some(action) = action else {
        return false;
    };
    match action {
        MenuCmd::Size(v) => {
            let _ = apply_cmd("set_size", &serde_json::json!({ "size": v as f64 }));
        }
        MenuCmd::Opacity(v) => {
            let _ = apply_cmd("set_opacity", &serde_json::json!({ "value": v as f64 }));
        }
        MenuCmd::Mirror => {
            let cur = s_mirror();
            let _ = apply_cmd("set_mirror", &serde_json::json!({ "value": !cur }));
        }
        MenuCmd::Autostart => {
            let on = autostart_on();
            set_autostart(!on);
        }
        MenuCmd::Reset => {
            let _ = apply_cmd("reset_all", &serde_json::Value::Null);
        }
        MenuCmd::Sound(id) => {
            let _ = apply_cmd("set_sound", &serde_json::json!({ "value": id }));
        }
        MenuCmd::Mute => {
            audio::set_muted(!audio::muted());
        }
        MenuCmd::Behavior(key) => {
            let on = audio::is_behavior_enabled(&key);
            audio::set_behavior_enabled(&key, !on);
        }
    }
    // 每次经由菜单改动都立刻落盘：桌宠没有「保存」按钮，
    // 用户也不该为了留住设置而正常退出一次。
    save_config();
    true
}

/// 发现新版本时的确认框；确认后下载并重启
pub(crate) unsafe fn prompt_update(hwnd: HWND) {
    let rel = PENDING_UPDATE
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()));
    let Some(rel) = rel else { return };

    let notes: String = rel.notes.chars().take(400).collect();
    let msg = format!(
        "发现新版本 v{}（当前 v{}）\n\n{}\n\n现在更新吗？更新时小猫会重启一下。",
        rel.version,
        update::current_version(),
        if notes.is_empty() { "（无更新说明）".to_string() } else { notes }
    );
    let mw: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let tw: Vec<u16> = "圆宝桌宠 · 发现新版本"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let r = MessageBoxW(
        hwnd,
        PCWSTR(mw.as_ptr()),
        PCWSTR(tw.as_ptr()),
        MB_YESNO | MB_ICONINFORMATION,
    );
    if r != IDYES {
        return;
    }

    match update::apply(&rel) {
        Ok(()) => {
            // 更新器已拉起，本体退出让位
            let _ = DestroyWindow(hwnd);
        }
        Err(e) => {
            let emsg = format!(
                "更新失败：{e}\n\n可以到发布页手动下载：\n{}",
                rel.page_url
            );
            let ew: Vec<u16> = emsg.encode_utf16().chain(std::iter::once(0)).collect();
            let etw: Vec<u16> = "圆宝桌宠 · 更新失败"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            MessageBoxW(
                hwnd,
                PCWSTR(ew.as_ptr()),
                PCWSTR(etw.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

/// 右键菜单：自绘，日系清新风格
pub(crate) unsafe fn pet_menu(hwnd: HWND) -> u16 {
    let Ok(menu) = CreatePopupMenu() else { return 0 };
    let locked = ST
        .get()
        .and_then(|st| st.lock().ok().map(|g| g.locked))
        .unwrap_or(false);

    menu_ui::begin();
    menu_ui::add(menu, 0, "圆宝桌宠", menu_ui::Kind::Header);
    let sub = settings_submenu();
    menu_ui::add_submenu(menu, 20, "设置", sub);
    menu_ui::add(
        menu,
        11,
        if locked { "解锁（可拖动）" } else { "锁定（穿透）" },
        menu_ui::Kind::Plain,
    );
    menu_ui::add(menu, 12, "回到右下角", menu_ui::Kind::Plain);
    menu_ui::add(
        menu,
        30,
        &format!("检查更新（v{}）", update::current_version()),
        menu_ui::Kind::Plain,
    );
    menu_ui::add(menu, 0, "", menu_ui::Kind::Separator);
    menu_ui::add(menu, 13, "圆宝再见", menu_ui::Kind::Plain);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, None);
    let _ = DestroyMenu(menu);
    cmd.0 as u16
}
