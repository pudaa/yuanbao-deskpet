//! 自绘菜单（owner-draw）：把「日系清新」的视觉落到原生 Win32 菜单上。
//!
//! 纯 GDI，零额外依赖、零常驻内存。菜单项数据放在全局池里，
//! `dwItemData` 只存 1-based 索引，菜单销毁时不需要释放任何东西。

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED, ODT_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::*;

// COLORREF 是 0x00BBGGRR
const BG: COLORREF = COLORREF(0x00F8_FAFA); // #fafaf8
const SOFT: COLORREF = COLORREF(0x00FD_F6EE); // #eef6fd 悬停
const TEXT: COLORREF = COLORREF(0x0068_554A); // #4a5568
const MUTED: COLORREF = COLORREF(0x00AD_A098); // #98a0ad 分组标题
const LINE: COLORREF = COLORREF(0x00E5_E9E9); // #e9e9e5 发丝分隔线
const ACCENT: COLORREF = COLORREF(0x00F6_B564); // #64b5f6
const RING: COLORREF = COLORREF(0x00CF_D4D4); // #d4d4cf 未选中圆环

const MENU_WIDTH: u32 = 184;
const H_ITEM: u32 = 32;
const H_HEADER: u32 = 24;
const H_SEP: u32 = 11;

#[derive(Clone, Copy)]
pub enum Kind {
    Header,
    Radio(bool), // 是否选中
    Check(bool),
    Normal,  // 左侧留 40px（与带圆点的项对齐）
    Plain,   // 左侧只留 16px（无标记的项，避免文字显得偏右）
    Submenu,
    Separator,
}

#[derive(Clone)]
struct Item {
    label: String,
    kind: Kind,
}

static POOL: OnceLock<Mutex<Vec<Item>>> = OnceLock::new();
/// 本轮已经设过圆角的菜单窗口（主菜单和子菜单各一个）
static ROUNDED: OnceLock<Mutex<HashSet<isize>>> = OnceLock::new();

fn pool() -> &'static Mutex<Vec<Item>> {
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

/// 每次弹出菜单前调用，清空上一轮的项。菜单是同步显示的，池不会被并发重建。
pub fn begin() {
    if let Ok(mut p) = pool().lock() {
        p.clear();
    }
    if let Ok(mut r) = ROUNDED.get_or_init(|| Mutex::new(HashSet::new())).lock() {
        r.clear();
    }
}

fn get(idx: usize) -> Option<Item> {
    if idx == 0 {
        return None;
    }
    pool().lock().ok().and_then(|p| p.get(idx - 1).cloned())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn insert(menu: HMENU, id: u32, label: &str, kind: Kind, sub: Option<HMENU>) {
    let idx = {
        let Ok(mut p) = pool().lock() else { return };
        p.push(Item {
            label: label.to_owned(),
            kind,
        });
        p.len() // 1-based
    };

    let mut mask = MIIM_FTYPE | MIIM_ID | MIIM_DATA | MIIM_STATE;
    if sub.is_some() {
        mask |= MIIM_SUBMENU;
    }
    let state = if matches!(kind, Kind::Header | Kind::Separator) {
        MFS_DISABLED | MFS_GRAYED
    } else {
        MENU_ITEM_STATE(0)
    };
    let mii = MENUITEMINFOW {
        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
        fMask: mask,
        fType: MFT_OWNERDRAW,
        fState: state,
        wID: id,
        hSubMenu: sub.unwrap_or_default(),
        dwItemData: idx,
        ..Default::default()
    };
    let pos = GetMenuItemCount(menu).max(0) as u32;
    let _ = InsertMenuItemW(menu, pos, BOOL(1), &mii);
}

pub unsafe fn add(menu: HMENU, id: u32, label: &str, kind: Kind) {
    insert(menu, id, label, kind, None);
}

pub unsafe fn add_submenu(menu: HMENU, id: u32, label: &str, sub: HMENU) {
    insert(menu, id, label, Kind::Submenu, Some(sub));
}

// ---- 尺寸测量 ----

pub unsafe fn measure(lparam: isize) -> bool {
    if lparam == 0 {
        return false;
    }
    let m = &mut *(lparam as *mut MEASUREITEMSTRUCT);
    if m.CtlType != ODT_MENU {
        return false;
    }
    let Some(item) = get(m.itemData) else {
        return false;
    };
    m.itemWidth = MENU_WIDTH;
    m.itemHeight = match item.kind {
        Kind::Separator => H_SEP,
        Kind::Header => H_HEADER,
        _ => H_ITEM,
    };
    true
}

// ---- 绘制 ----

fn fill_rect(hdc: HDC, l: i32, t: i32, r: i32, b: i32, color: COLORREF) {
    unsafe {
        let br = CreateSolidBrush(color);
        let rc = RECT { left: l, top: t, right: r, bottom: b };
        let _ = FillRect(hdc, &rc, br);
        let _ = DeleteObject(HGDIOBJ(br.0));
    }
}

fn circle(hdc: HDC, cx: i32, cy: i32, radius: i32, color: COLORREF) {
    unsafe {
        let br = CreateSolidBrush(color);
        let old = SelectObject(hdc, HGDIOBJ(br.0));
        let _ = Ellipse(hdc, cx - radius, cy - radius, cx + radius, cy + radius);
        SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(br.0));
    }
}

fn text(
    hdc: HDC,
    rect: &mut RECT,
    s: &str,
    color: COLORREF,
    size: i32,
    right: bool,
) {
    unsafe {
        let face = wide("等线");
        // FW_LIGHT(300) —— 日系清新明确禁止粗体
        let font = CreateFontW(-size, 0, 0, 0, 300, 0, 0, 0, 1, 0, 0, 5, 0, PCWSTR(face.as_ptr()));
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        let _ = SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT
        let _ = SetTextColor(hdc, color);
        let mut buf: Vec<u16> = s.encode_utf16().collect();
        let fmt = DT_SINGLELINE | DT_VCENTER | if right { DT_RIGHT } else { DT_LEFT };
        let _ = DrawTextW(hdc, &mut buf, rect, fmt);
        SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(font.0));
    }
}

/// 给弹出菜单窗口加圆角：DWM 属性优先（保留系统阴影），
/// 再用圆角 region 兜底——这台机器上系统菜单本身是直角，光靠 DWM 不够。
unsafe fn apply_rounding(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    let key = hwnd.0 as isize;
    {
        let Ok(mut done) = ROUNDED.get_or_init(|| Mutex::new(HashSet::new())).lock() else {
            return;
        };
        if !done.insert(key) {
            return;
        }
    }

    // Windows 11：让 DWM 按圆角合成窗口（保留阴影）
    let pref: u32 = 2; // DWMWCP_ROUND
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &pref as *const u32 as *const core::ffi::c_void,
        4,
    );

    // 兜底：直接把窗口裁成圆角矩形，所有 Windows 版本都生效
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_ok() {
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        if w > 0 && h > 0 {
            let rgn = CreateRoundRectRgn(0, 0, w, h, 12, 12);
            if !rgn.is_invalid() {
                // 成功后 region 归系统所有，不要 DeleteObject
                let _ = SetWindowRgn(hwnd, rgn, true);
            }
        }
    }
}

/// 找到正在显示的菜单窗口（类名 #32768）
unsafe fn menu_window_of(d: &DRAWITEMSTRUCT) -> HWND {
    let mut cls = [0u16; 32];
    let n = GetClassNameW(d.hwndItem, &mut cls);
    if n > 0 {
        let name = String::from_utf16_lossy(&cls[..n as usize]);
        if name == "#32768" {
            return d.hwndItem;
        }
    }
    let cls = wide("#32768");
    FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()).unwrap_or_default()
}

pub unsafe fn draw(lparam: isize) -> bool {
    if lparam == 0 {
        return false;
    }
    let d = &*(lparam as *const DRAWITEMSTRUCT);
    if d.CtlType != ODT_MENU || d.hDC.is_invalid() {
        return false;
    }
    let Some(item) = get(d.itemData) else {
        return false;
    };
    apply_rounding(menu_window_of(d));
    let hdc = d.hDC;
    let rc = d.rcItem;
    let selected = (d.itemState.0 & ODS_SELECTED.0) != 0;

    // 背景：留白为主，悬停只用极浅的一层蓝
    fill_rect(
        hdc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        if selected && !matches!(item.kind, Kind::Header | Kind::Separator) {
            SOFT
        } else {
            BG
        },
    );

    match item.kind {
        Kind::Separator => {
            // 发丝线：1px、内缩 14px
            let y = (rc.top + rc.bottom) / 2;
            fill_rect(hdc, rc.left + 14, y, rc.right - 14, y + 1, LINE);
        }
        Kind::Header => {
            let mut r = RECT { left: rc.left + 16, top: rc.top, right: rc.right - 12, bottom: rc.bottom };
            text(hdc, &mut r, &item.label, MUTED, 12, false);
        }
        _ => {
            let cy = (rc.top + rc.bottom) / 2;
            // 左侧状态标记
            match item.kind {
                Kind::Radio(true) => circle(hdc, rc.left + 20, cy, 4, ACCENT),
                Kind::Radio(false) => {
                    circle(hdc, rc.left + 20, cy, 5, RING);
                    circle(hdc, rc.left + 20, cy, 3, BG);
                }
                Kind::Check(true) => circle(hdc, rc.left + 20, cy, 4, ACCENT),
                Kind::Check(false) => {
                    circle(hdc, rc.left + 20, cy, 5, RING);
                    circle(hdc, rc.left + 20, cy, 3, BG);
                }
                _ => {}
            }

            let left = match item.kind {
                Kind::Radio(_) | Kind::Check(_) | Kind::Normal => 40,
                _ => 16,
            };
            let mut r = RECT {
                left: rc.left + left,
                top: rc.top,
                right: rc.right - 30,
                bottom: rc.bottom,
            };
            text(hdc, &mut r, &item.label, TEXT, 15, false);

            if matches!(item.kind, Kind::Submenu) {
                let mut ar = RECT {
                    left: rc.right - 34,
                    top: rc.top,
                    right: rc.right - 14,
                    bottom: rc.bottom,
                };
                text(hdc, &mut ar, "›", MUTED, 16, true);
            }
        }
    }
    true
}
