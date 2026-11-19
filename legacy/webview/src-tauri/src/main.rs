#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod physics;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewUrl,
    WebviewWindowBuilder,
};

const PAD: f64 = 14.0;
const POLL_MS: u64 = 16;
/// 旋转基准在窗口高度上的位置：猫鼻子约在画面 38% 处，
/// 换算到窗口（含 14px 透明边）= (14 + petSize*0.38) / (petSize + 28) ≈ 0.395。
/// 之前用 0.5（几何中心，落在胸口），抬头时角度会整体偏下。
const FOCUS_Y: f64 = 0.395;
const CENTER_DEAD: f64 = 20.0;

/// 角度极点：false = 以桌宠自身为中心（头始终面向鼠标，语义正确）
/// true = 以屏幕中心为中心（覆盖全 360°，但与"面向鼠标"不符，仅调试用）
const ORIGIN_SCREEN: bool = false;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Config {
    size: f64,
    opacity: f64,
    smoothing: f64,
    idle_radius: f64,
    locked: bool,
    visible: bool,
    mirror: bool,
    always_on_top: bool,
    autostart: bool,
    theme: String,
    x: Option<i32>,
    y: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            size: 160.0,
            opacity: 1.0,
            smoothing: 1.0,
            idle_radius: 28.0,
            locked: false,
            visible: true,
            mirror: false,
            always_on_top: true,
            autostart: false,
            theme: "fresh".into(),
            x: None,
            y: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CursorEvent {
    u: f64,
    /// 光标贴得太近（距中心 < idle_radius）时，角度噪声大，前端应保持当前朝向不动
    hold: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SizeEvent {
    pet_size: f64,
    win_size: f64,
    pad: f64,
    smoothing: f64,
}

/// 拖动轨迹与物理状态
#[derive(Default)]
struct DragState {
    /// 最近的光标轨迹 (x, y, t_ms)
    trail: Vec<(f64, f64, f64)>,
}

#[derive(Default)]
struct PhysState {
    body: Option<physics::Body>,
}

struct AppState(Mutex<Config>, Mutex<DragState>, Mutex<PhysState>);

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() {
        return lo;
    }
    v.max(lo).min(hi)
}

fn cfg_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    dir.join("config.json")
}

fn load_cfg(app: &AppHandle) -> Config {
    let mut c: Config = std::fs::read_to_string(cfg_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    c.size = clamp(c.size, 120.0, 360.0);
    c.opacity = clamp(c.opacity, 0.4, 1.0);
    c.smoothing = clamp(c.smoothing, 0.5, 2.0);
    c.idle_radius = clamp(c.idle_radius, 16.0, 120.0);
    c
}

fn save_cfg(app: &AppHandle, c: &Config) {
    let p = cfg_path(app);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(p, s);
    }
}

#[cfg(windows)]
fn work_area() -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            (mi.rcWork.left, mi.rcWork.top, mi.rcWork.right, mi.rcWork.bottom)
        } else {
            (0, 0, 1920, 1080)
        }
    }
}

#[cfg(windows)]
fn cursor_pos() -> (i32, i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        (pt.x, pt.y)
    }
}

fn win_size_px(cfg: &Config) -> f64 {
    cfg.size + PAD * 2.0
}

fn size_event(cfg: &Config) -> SizeEvent {
    SizeEvent {
        pet_size: cfg.size,
        win_size: win_size_px(cfg),
        pad: PAD,
        smoothing: cfg.smoothing,
    }
}

fn place_bottom_right(app: &AppHandle) {
    let cfg = app.state::<AppState>().0.lock().unwrap().clone();
    let Some(w) = app.get_webview_window("pet") else {
        return;
    };
    let sf = w.scale_factor().unwrap_or(1.0);
    let (_, _, r, b) = work_area();
    let ws = (win_size_px(&cfg) * sf).round() as i32;
    eprintln!(
        "[place] work_area=({},{}) sf={} ws_phys={} -> ({},{})",
        r,
        b,
        sf,
        ws,
        r - ws - 10,
        b - ws - 10
    );
    let _ = w.set_position(PhysicalPosition::new(r - ws - 10, b - ws - 10));
}

/// 改尺寸时保持窗口右下角不动，视觉上像"从脚底缩放"
fn apply_size(app: &AppHandle, new_size: f64) {
    let new_size = clamp(new_size, 120.0, 360.0);
    {
        let st = app.state::<AppState>();
        let mut g = st.0.lock().unwrap();
        g.size = new_size;
        let c = g.clone();
        drop(g);
        save_cfg(app, &c);
    }
    if let Some(w) = app.get_webview_window("pet") {
        let sf = w.scale_factor().unwrap_or(1.0);
        let old_phys = w.outer_size().map(|s| s.width as f64).unwrap_or(0.0);
        let new_phys = win_size_px(&load_cfg(app)) * sf;
        let _ = w.set_size(tauri::Size::Logical(LogicalSize::new(
            win_size_px(&load_cfg(app)),
            win_size_px(&load_cfg(app)),
        )));
        if let Ok(pos) = w.outer_position() {
            let d = (old_phys - new_phys).round() as i32;
            let _ = w.set_position(PhysicalPosition::new(pos.x + d, pos.y + d));
        }
    }
    let cfg = load_cfg(app);
    let _ = app.emit("pet:size", size_event(&cfg));
    let _ = app.emit("settings:state", cfg);
    refresh_tray(app);
}

fn apply_lock(app: &AppHandle, locked: bool) {
    if let Some(w) = app.get_webview_window("pet") {
        let _ = w.set_ignore_cursor_events(locked);
    }
    let cfg = load_cfg(app);
    let _ = app.emit("pet:locked", locked);
    let _ = app.emit("settings:state", cfg);
    // 托盘菜单的勾选状态必须跟着刷新，否则右键锁定后托盘仍显示"未锁定"
    refresh_tray(app);
}

fn toggle_visible(app: &AppHandle) {
    let mut cfg = load_cfg(app);
    cfg.visible = !cfg.visible;
    save_cfg(app, &cfg);
    if let Some(w) = app.get_webview_window("pet") {
        if cfg.visible {
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
    let _ = app.emit("settings:state", cfg);
    refresh_tray(app);
}

/// 点击小猫时说的话
const WHISPERS: &[&str] = &[
    "喵？",
    "别戳啦…",
    "在忙呢",
    "今天也要加油哦",
    "有点想睡了",
    "摸鱼中…",
    "饿…",
    "陪你一会儿",
    "摸摸头",
    "要不要休息一下",
];

fn play_pat_hint(app: &AppHandle) {
    let _ = app.emit("pet:pat", ());
}

fn pt_in_window(px: i32, py: i32, pos: tauri::PhysicalPosition<i32>, size: tauri::PhysicalSize<u32>) -> bool {
    px >= pos.x
        && px < pos.x + size.width as i32
        && py >= pos.y
        && py < pos.y + size.height as i32
}

fn spawn_poller(app: AppHandle) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    std::thread::spawn(move || {
        let mut in_idle = false;
        let mut last_lb = false;
        let mut press_pos: Option<(i32, i32)> = None;
        let mut press_in_win = false;
        let mut last_tick = std::time::Instant::now();

        loop {
            std::thread::sleep(Duration::from_millis(POLL_MS));
            let now = std::time::Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f64();
            last_tick = now;

            let cfg = app.state::<AppState>().0.lock().unwrap().clone();
            if !cfg.visible {
                continue;
            }
            let Some(w) = app.get_webview_window("pet") else {
                continue;
            };
            let (Ok(mut pos), Ok(size), Ok(sf)) =
                (w.outer_position(), w.outer_size(), w.scale_factor())
            else {
                continue;
            };

            let (px, py) = cursor_pos();
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);

            // 光标轨迹：松手时用它估算甩出速度
            {
                let st = app.state::<AppState>();
                let mut d = st.1.lock().unwrap();
                d.trail.push((px as f64, py as f64, now_ms));
                if d.trail.len() > 24 {
                    d.trail.remove(0);
                }
            }

            // 左键松开：有位移=甩出，无位移=单击（碎碎念）
            let lb = unsafe { (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0 };
            if lb && !last_lb {
                press_pos = Some((pos.x, pos.y));
                press_in_win = pt_in_window(px, py, pos, size);

                // 抓住正在滑行的猫：立刻停掉物理。
                // 否则物理每帧 set_position 和系统拖拽会抢同一个窗口位置，表现为原地闪烁。
                {
                    let st = app.state::<AppState>();
                    let mut p = st.2.lock().unwrap();
                    p.body = None;
                }
            }
            if last_lb && !lb {
                let moved = press_pos
                    .map(|(ox, oy)| (pos.x - ox).abs() > 3 || (pos.y - oy).abs() > 3)
                    .unwrap_or(false);
                let release_in_win = pt_in_window(px, py, pos, size);
                // 只有"按下和松开都在窗口内、且窗口没位移"才算抚摸。
                // 否则拖动结束后在桌面空白处松手，也会被误判成一次点击。
                let is_pat = !moved && press_in_win && release_in_win;

                let st = app.state::<AppState>();
                let throw = {
                    let mut d = st.1.lock().unwrap();
                    let r = physics::estimate_throw(&d.trail);
                    d.trail.clear();
                    r
                };
                if moved {
                    if let Some((vx, vy)) = throw {
                        let mut p = st.2.lock().unwrap();
                        p.body = Some(physics::Body {
                            x: pos.x as f64,
                            y: pos.y as f64,
                            vx,
                            vy,
                        });
                    }
                }
                // 无论点击还是拖动，松手都要通知前端回弹
                let _ = app.emit("pet:release", is_pat);
                press_pos = None;
                press_in_win = false;
            }
            last_lb = lb;

            // 物理步进
            {
                let st = app.state::<AppState>();
                let mut p = st.2.lock().unwrap();
                if let Some(body) = p.body.as_mut() {
                    let (l, t, r, b) = work_area();
                    let bounds = physics::Bounds {
                        min_x: l as f64,
                        min_y: t as f64,
                        max_x: (r - size.width as i32) as f64,
                        max_y: (b - size.height as i32) as f64,
                    };
                    let res = body.step(dt, bounds);
                    let _ = w
                        .set_position(tauri::PhysicalPosition::new(body.x as i32, body.y as i32));
                    if res.landed {
                        let _ = app.emit("pet:landed", res.impact);
                    }
                    if res.at_rest {
                        let (fx, fy) = (body.x as i32, body.y as i32);
                        p.body = None;
                        drop(p);
                        {
                            let mut c = st.0.lock().unwrap();
                            c.x = Some(fx);
                            c.y = Some(fy);
                        }
                        let cfg2 = st.0.lock().unwrap().clone();
                        save_cfg(&app, &cfg2);
                        if let Ok(np) = w.outer_position() {
                            pos = np;
                        }
                        continue;
                    }
                }
            }

            let cx = pos.x + size.width as i32 / 2;
            let cy = pos.y + (size.height as f64 * FOCUS_Y) as i32;

            let ddx = (px - cx) as f64;
            let ddy = (py - cy) as f64;
            let dist = (ddx * ddx + ddy * ddy).sqrt();

            if !in_idle && dist < cfg.idle_radius * sf {
                in_idle = true;
            } else if in_idle && dist > cfg.idle_radius * 1.7 * sf {
                in_idle = false;
            }
            let hold = in_idle;

            if hold {
                let _ = app.emit("pet:cursor", CursorEvent { u: 0.25, hold: true });
                continue;
            }

            let (l, t, r, b) = work_area();
            let (ox, oy) = if ORIGIN_SCREEN {
                ((l + r) / 2, (t + b) / 2)
            } else {
                (cx, cy)
            };

            let dx = (px - ox) as f64;
            let dy = (py - oy) as f64;
            if (dx * dx + dy * dy).sqrt() < CENTER_DEAD {
                continue;
            }

            let theta = dy.atan2(dx);
            let u = (theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI);
            let _ = app.emit("pet:cursor", CursorEvent { u, hold });
        }
    });
}

/// 碎碎念气泡窗口（预创建，透明、无边框、置顶、不接收鼠标）
fn build_bubble_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window("bubble") {
        return Some(w);
    }
    WebviewWindowBuilder::new(app, "bubble", WebviewUrl::App("bubble/bubble.html".into()))
        .title("bubble")
        .inner_size(320.0, 110.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()
        .ok()
}

fn show_bubble(app: &AppHandle, text: &str) {
    let Some(w) = build_bubble_window(app) else {
        return;
    };

    // 贴在猫头顶；上方放不下就翻到下方
    if let Some(pet) = app.get_webview_window("pet") {
        if let (Ok(p), Ok(s), Ok(sf)) =
            (pet.outer_position(), pet.outer_size(), pet.scale_factor())
        {
            let bw = (320.0 * sf) as i32;
            let bh = (110.0 * sf) as i32;
            let (l, t, r, _b) = work_area();
            let mut x = p.x + s.width as i32 / 2 - bw / 2;
            x = x.clamp(l, (r - bw).max(l));
            let mut y = p.y - bh + (8.0 * sf) as i32;
            if y < t {
                y = p.y + s.height as i32 - (8.0 * sf) as i32;
            }
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }

    let _ = w.emit("bubble:say", text);
    let _ = w.show();

    let w2 = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2600));
        let _ = w2.emit("bubble:hide", ());
        std::thread::sleep(Duration::from_millis(320));
        let _ = w2.hide();
    });
}

/// 创建（或取用）设置窗口。
/// 只在主线程调用——从 IPC 命令线程建 WebView 会得到空白页 + center() 失效。
fn build_settings_window(app: &AppHandle, show_it: bool) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window("settings") {
        if show_it {
            let _ = w.center();
            let _ = w.show();
            let _ = w.set_focus();
            if let Some(pet) = app.get_webview_window("pet") {
                let _ = pet.set_always_on_top(false);
            }
        }
        return Some(w);
    }

    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings/settings.html".into()))
        .title("圆宝桌宠 · 设置")
        .inner_size(500.0, 768.0)
        .resizable(false)
        .visible(false)
        .build();

    match built {
        Ok(w) => {
            // 关闭时只隐藏，保证下次点击复用同一个窗口（避免再次走建窗路径）
            let w2 = w.clone();
            w.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w2.hide();
                    let app2 = w2.app_handle();
                    let cfg = load_cfg(app2);
                    if let Some(pet) = app2.get_webview_window("pet") {
                        let _ = pet.set_always_on_top(cfg.always_on_top);
                    }
                }
            });

            if show_it {
                let _ = w.center();
                let _ = w.show();
                let _ = w.set_focus();
                if let Some(pet) = app.get_webview_window("pet") {
                    let _ = pet.set_always_on_top(false);
                }
            }
            Some(w)
        }
        Err(_) => None,
    }
}

fn open_settings(app: &AppHandle) {
    let _ = build_settings_window(app, true);
}

fn make_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let cfg = load_cfg(app);
    let show = CheckMenuItem::with_id(app, "show", "显示圆宝", true, cfg.visible, None::<&str>)?;
    let lock = CheckMenuItem::with_id(
        app,
        "lock",
        "锁定（鼠标穿透）",
        true,
        cfg.locked,
        None::<&str>,
    )?;
    let s1 = CheckMenuItem::with_id(app, "size160", "小 · 160", true, cfg.size == 160.0, None::<&str>)?;
    let s2 = CheckMenuItem::with_id(app, "size220", "中 · 220", true, cfg.size == 220.0, None::<&str>)?;
    let s3 = CheckMenuItem::with_id(app, "size280", "大 · 280", true, cfg.size == 280.0, None::<&str>)?;
    let s4 = CheckMenuItem::with_id(app, "size340", "特大 · 340", true, cfg.size == 340.0, None::<&str>)?;
    let size_menu = Submenu::with_items(app, "尺寸", true, &[&s1, &s2, &s3, &s4])?;

    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let corner = MenuItem::with_id(app, "corner", "回到右下角", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[&show, &lock, &sep1, &size_menu, &settings, &corner, &sep2, &quit],
    )
}

/// 运行时重建托盘菜单。锁定/显隐/尺寸变化后必须调用，否则勾选状态永远停在启动时。
fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Ok(menu) = make_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let cfg = load_cfg(app);
    let _ = tray.set_tooltip(Some(if cfg.locked {
        "圆宝桌宠（已锁定）"
    } else {
        "圆宝桌宠"
    }));
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = make_menu(app)?;
    let icon = app.default_window_icon().cloned();
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("圆宝桌宠")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "show" => toggle_visible(app),
                "lock" => {
                    let c = load_cfg(app);
                    let mut nc = c.clone();
                    nc.locked = !c.locked;
                    save_cfg(app, &nc);
                    apply_lock(app, nc.locked);
                }
                "size160" => apply_size(app, 160.0),
                "size220" => apply_size(app, 220.0),
                "size280" => apply_size(app, 280.0),
                "size340" => apply_size(app, 340.0),
                "settings" => open_settings(app),
                "corner" => place_bottom_right(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_visible(tray.app_handle());
            }
        });

    if let Some(ic) = icon {
        builder = builder.icon(ic);
    }
    builder.build(app)?;
    Ok(())
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Config {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn set_size(app: AppHandle, size: f64) -> Config {
    apply_size(&app, size);
    load_cfg(&app)
}

#[tauri::command]
fn set_opacity(app: AppHandle, value: f64) -> Config {
    let mut c = load_cfg(&app);
    c.opacity = clamp(value, 0.4, 1.0);
    save_cfg(&app, &c);
    // 用前端 CSS 透明度实现，避免平台 API 差异
    let _ = app.emit("pet:opacity", c.opacity);
    let _ = app.emit("settings:state", c.clone());
    c
}

#[tauri::command]
fn set_smoothing(app: AppHandle, value: f64) -> Config {
    let mut c = load_cfg(&app);
    c.smoothing = clamp(value, 0.5, 2.0);
    save_cfg(&app, &c);
    let _ = app.emit("pet:size", size_event(&c));
    c
}

#[tauri::command]
fn set_idle_radius(app: AppHandle, value: f64) -> Config {
    let mut c = load_cfg(&app);
    c.idle_radius = clamp(value, 16.0, 120.0);
    save_cfg(&app, &c);
    c
}

#[tauri::command]
fn set_theme(app: AppHandle, value: String) -> Config {
    let mut c = load_cfg(&app);
    c.theme = value;
    save_cfg(&app, &c);
    let _ = app.emit("pet:theme", c.theme.clone());
    let _ = app.emit("settings:state", c.clone());
    c
}

#[tauri::command]
fn set_mirror(app: AppHandle, value: bool) -> Config {
    let mut c = load_cfg(&app);
    c.mirror = value;
    save_cfg(&app, &c);
    let _ = app.emit("pet:mirror", value);
    let _ = app.emit("settings:state", c.clone());
    c
}

#[tauri::command]
fn set_always_on_top(app: AppHandle, value: bool) -> Config {
    let mut c = load_cfg(&app);
    c.always_on_top = value;
    save_cfg(&app, &c);
    if let Some(w) = app.get_webview_window("pet") {
        let _ = w.set_always_on_top(value);
    }
    let _ = app.emit("settings:state", c.clone());
    c
}

#[cfg(windows)]
fn write_autostart(on: bool) {
    let exe = std::env::current_exe().unwrap_or_default();
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if on {
        let _ = std::process::Command::new("reg")
            .args([
                "add",
                key,
                "/v",
                "YuanBaoDeskPet",
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\"", exe.display()),
                "/f",
            ])
            .output();
    } else {
        let _ = std::process::Command::new("reg")
            .args(["delete", key, "/v", "YuanBaoDeskPet", "/f"])
            .output();
    }
}

#[cfg(not(windows))]
fn write_autostart(_on: bool) {}

#[tauri::command]
fn set_autostart(app: AppHandle, value: bool) -> Config {
    let mut c = load_cfg(&app);
    c.autostart = value;
    write_autostart(value);
    save_cfg(&app, &c);
    let _ = app.emit("settings:state", c.clone());
    c
}

#[tauri::command]
fn set_locked(app: AppHandle, value: bool) -> Config {
    let mut c = load_cfg(&app);
    c.locked = value;
    save_cfg(&app, &c);
    apply_lock(&app, c.locked);
    c
}

#[tauri::command]
fn reset_all(app: AppHandle) -> Config {
    let c = Config::default();
    save_cfg(&app, &c);
    if let Some(w) = app.get_webview_window("pet") {
        let _ = w.set_size(tauri::Size::Logical(LogicalSize::new(
            win_size_px(&c),
            win_size_px(&c),
        )));
    }
    place_bottom_right(&app);
    apply_lock(&app, c.locked);
    let _ = app.emit("pet:opacity", c.opacity);
    let _ = app.emit("pet:mirror", c.mirror);
    let _ = app.emit("pet:size", size_event(&c));
    let _ = app.emit("pet:theme", c.theme.clone());
    let _ = app.emit("settings:state", c.clone());
    c
}

#[tauri::command]
fn place_corner(app: AppHandle) {
    place_bottom_right(&app);
}

#[tauri::command]
fn open_settings_cmd(app: AppHandle) {
    // 必须回到 UI 线程建窗口：IPC 命令线程创建 WebView 会得到空白页面
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || open_settings(&app2));
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn main() {
    // 关闭 GPU 加速：桌宠只画一张静态帧，软件光栅化完全够用，
    // 而 WebView2 的 GPU 进程在透明窗口上会吃掉 250MB+。
    // 必须用环境变量而非 tauri.conf 的 additionalBrowserArgs —— 后者只作用于
    // 配置里的那个窗口，运行时新建的 settings 窗口参数不一致会直接建不起来。
    #[cfg(target_os = "windows")]
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-gpu --disable-gpu-compositing",
    );

    tauri::Builder::default()
        .manage(AppState(
        Mutex::new(Config::default()),
        Mutex::new(DragState::default()),
        Mutex::new(PhysState::default()),
    ))
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_size,
            set_opacity,
            set_smoothing,
            set_idle_radius,
            set_theme,
            set_locked,
            set_mirror,
            set_always_on_top,
            set_autostart,
            reset_all,
            place_corner,
            open_settings_cmd,
            quit_app
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = load_cfg(&handle);
            {
                let st = handle.state::<AppState>();
                *st.0.lock().unwrap() = cfg.clone();
            }

            if let Some(w) = handle.get_webview_window("pet") {
                let _ = w.set_size(tauri::Size::Logical(LogicalSize::new(
                    win_size_px(&cfg),
                    win_size_px(&cfg),
                )));
                let _ = w.set_ignore_cursor_events(cfg.locked);
                let _ = w.set_always_on_top(cfg.always_on_top);
                // 必须先显示窗口，set_position 才不会被忽略
                let _ = w.show();
                std::thread::sleep(Duration::from_millis(120));
                if let (Some(x), Some(y)) = (cfg.x, cfg.y) {
                    let _ = w.set_position(PhysicalPosition::new(x, y));
                } else {
                    place_bottom_right(&handle);
                }
            }

            let _ = handle.emit("pet:opacity", cfg.opacity);

            build_tray(&handle)?;
            spawn_poller(handle.clone());

            // 预创建设置窗口与气泡窗口（隐藏）。在 setup 主线程里建是唯一可靠的路径，
            // 运行时从 IPC 命令建会得到空白页。
            let _ = build_settings_window(&handle, false);

            // 调试入口：环境变量或标记文件均可触发
            let marker = std::path::Path::new("D:/Codes/yuanbao-deskpet/build/.open_settings");
            if std::env::var("YB_OPEN_SETTINGS").is_ok() || marker.exists() {
                open_settings(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Moved(pos) = event {
                if window.label() == "pet" {
                    let app = window.app_handle();
                    let mut c = load_cfg(app);
                    c.x = Some(pos.x);
                    c.y = Some(pos.y);
                    save_cfg(app, &c);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
