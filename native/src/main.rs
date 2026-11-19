//! 圆宝桌宠 · 原生绘制实现
//!
//! 不依赖 WebView：分层窗口 + UpdateLayeredWindow 逐帧贴 BGRA 位图。
//! 模块划分见 docs/ARCHITECTURE.md：app / render / input / audio / ui / sys。

#![windows_subsystem = "windows"]

mod app;
mod audio;
mod config;
mod input;
mod physics;
mod render;
mod settings;
mod sys;
mod ui;
mod update;

use app::cmd::save_config;
use app::state::*;
use app::tick::on_tick;
use input::hit::hit_test;
use render::frames::load_one;
use render::present::ensure_dib_locked;
use render::press::*;
use sys::win::{now_ms, screen_size, set_locked, to_bottom_right};
use ui::menu::{handle_menu_cmd, pet_menu, prompt_update, settings_submenu};
use ui::{menu_ui, tray};

use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

/// 多媒体定时器回调里投递的自定义消息
const WM_TICK: u32 = WM_APP + 2;

/// 后台检查到新版本，通知 UI 线程弹框
const WM_UPDATE_READY: u32 = WM_APP + 3;

static PENDING_UPDATE: OnceLock<Mutex<Option<update::Release>>> = OnceLock::new();

static mut TIMER_HWND: HWND = HWND(std::ptr::null_mut());

unsafe extern "system" fn timer_proc(_id: u32, _msg: u32, _user: usize, _d1: usize, _d2: usize) {
    let _ = PostMessageW(TIMER_HWND, WM_TICK, WPARAM(0), LPARAM(0));
}

fn main() {
    // 更新器模式：由 update::apply 拉起，负责覆盖文件并重启，不启动桌宠
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() >= 3 && argv[1] == "--apply-update" {
        update::run_updater(&argv[2..]);
    }

    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(
            std::env::temp_dir().join("yuanbao_panic.log"),
            format!("{info}\n"),
        );
    }));

    // 读持久化配置：尺寸必须在预载帧之前生效（load_one 按 disp() 缩放），
    // 所以放在这里，而不是等窗口创建完再套。
    let cfg = config::load();
    DISPLAY_I.store(cfg.size, Ordering::Relaxed);
    // 用户上次选的音效组覆盖内置默认（引擎按需初始化，这里只是记住选择）
    audio::set_disabled(cfg.disabled_behaviors.clone());
    audio::set_muted(cfg.muted);
    let _ = audio::set_pack(&cfg.sound);

    unsafe {
        // 默认定时器精度约 15.6ms，物理步进会抖；提到 1ms 让甩出更连续
        let _ = windows::Win32::Media::timeBeginPeriod(1);
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // 全量预载：把解码/缩放的一次性成本从"每帧"挪到"启动"。
        // 代价是内存从 ~6MB 升到 ~50MB，但转动 CPU 会明显下降。
        let mut frames: Vec<Option<Frame>> = (0..frame_count()).map(|_| None).collect();
        for i in 0..frame_count() {
            frames[i] = load_one(i);
        }
        if frames.iter().all(|f| f.is_none()) {
            return;
        }

        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: w!("YuanBaoNative"),
            // 不指定光标时系统会回退到 AppStarting（箭头带沙漏），
            // 表现为"鼠标放在猫上一直像在加载"
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("YuanBaoNative"),
            w!("圆宝桌宠"),
            WS_POPUP,
            0,
            0,
            win(),
            win(),
            None,
            None,
            HINSTANCE(hinst.0),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        ST.get_or_init(|| {
            Mutex::new(State {
                frames,
                cur: main_start(),
                dib: None,
                hwnd,
                dragging: false,
                grab_dx: 0,
                grab_dy: 0,
                trail: Vec::with_capacity(32),
                body: None,
                last_tick: std::time::Instant::now(),
                press: Press::Idle,
                pressed_hit: false,
                press_win: (0, 0),
                press_cursor: (0, 0),
                in_idle: false,
                locked: false,
                opacity: cfg.opacity,
                mirror: cfg.mirror,
                idle_radius: idle_enter(),
                last_cur: usize::MAX,
                last_sx: 1.0,
                last_sy: 1.0,
                last_pos: (i32::MIN, i32::MIN),
                last_op: -1.0,
                last_mirror: false,
                last_frame: None,
            })
        });

        // 初始位置：优先用上次保存的坐标；越界（换分辨率、拔掉副屏）
        // 或没保存过就回落到右下角，避免猫跑到看不见的地方。
        let (sw, sh) = screen_size();
        let saved = match (cfg.x, cfg.y) {
            (Some(x), Some(y))
                if x > -win() && y > -win() && x + win() < sw + 40 && y + win() < sh + 40 =>
            {
                Some((x, y))
            }
            _ => None,
        };
        let (px, py) = saved.unwrap_or((sw - win() - 40, sh - win() - 100));
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            px,
            py,
            win(),
            win(),
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );

        if let Some(st) = ST.get() {
            if let Ok(mut s) = st.lock() {
                ensure_dib_locked(&mut s);
            }
        }
        on_tick();
        let _ = tray::add(hwnd);

        // 启动 5 秒后静默检查更新：不占用启动时间，失败也不打扰用户
        {
            let hw = hwnd.0 as isize;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if let Ok(Some(rel)) = update::check() {
                    if let Ok(mut m) =
                        PENDING_UPDATE.get_or_init(|| Mutex::new(None)).lock()
                    {
                        *m = Some(rel);
                    }
                    let _ = PostMessageW(
                        HWND(hw as *mut _),
                        WM_UPDATE_READY,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
        }
        // SetTimer 受系统 15.6ms tick 量化（实测 11/21ms 交替，甩出明显不连续）；
        // 多媒体定时器精度 1ms，回调里 PostMessage 给 UI 线程处理。
        TIMER_HWND = hwnd;
        let _ = windows::Win32::Media::timeSetEvent(
            12,
            1,
            Some(timer_proc),
            0,
            windows::Win32::Media::TIME_PERIODIC,
        );

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_UPDATE_READY => {
            prompt_update(hwnd);
            LRESULT(0)
        }
        WM_MEASUREITEM => {
            menu_ui::measure(lparam.0);
            LRESULT(1)
        }
        WM_DRAWITEM => {
            menu_ui::draw(lparam.0);
            LRESULT(1)
        }

        WM_TIMER | WM_TICK => {
            on_tick();
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut wr = RECT::default();
            let _ = GetWindowRect(hwnd, &mut wr);
            let mut capture = false;
            if let Some(st) = ST.get() {
                if let Ok(mut s) = st.lock() {
                    let hit = hit_test(&mut s, pt.x, pt.y);
                    s.dragging = true;
                    s.pressed_hit = hit;
                    s.press_win = (wr.left, wr.top);
                    s.press_cursor = (pt.x, pt.y);
                    s.grab_dx = pt.x - wr.left;
                    s.grab_dy = pt.y - wr.top;
                    s.body = None; // 抓住正在滑行的猫就停物理
                    s.trail.clear();
                    s.trail.push((pt.x as f64, pt.y as f64, now_ms()));
                    if hit {
                        // 跳过 ease-out 最迟疑的开头，让第一帧就有可见形变
                        s.press = Press::Down(0.015);
                        audio::press();
                    } else {
                        s.press = Press::Idle;
                    }
                    capture = true;
                }
            }
            if capture {
                SetCapture(hwnd);
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            if let Some(st) = ST.get() {
                if let Ok(s) = st.lock() {
                    if s.dragging {
                        let mut pt = POINT::default();
                        if GetCursorPos(&mut pt).is_ok() {
                            let x = pt.x - s.grab_dx;
                            let y = pt.y - s.grab_dy;
                            let h = s.hwnd;
                            let _ = SetWindowPos(
                                h,
                                HWND_TOPMOST,
                                x,
                                y,
                                0,
                                0,
                                SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            if let Some(st) = ST.get() {
                if let Ok(mut s) = st.lock() {
                    if s.dragging {
                        s.dragging = false;
                        let _ = ReleaseCapture();

                        let mut wr = RECT::default();
                        let _ = GetWindowRect(s.hwnd, &mut wr);
                        // 用光标位移判点击，而不是窗口位移：
                        // 长按时鼠标有 1~2px 抖动会让窗口跟着挪，用窗口位移会被误判成拖动，
                        // 结果按下音播了、松开音不播。
                        let mut cpt = POINT::default();
                        let _ = GetCursorPos(&mut cpt);
                        // 位移按像素传给音效引擎：when.movedLt 需要的是距离，不是布尔
                        let drag = (cpt.x - s.press_cursor.0)
                            .abs()
                            .max((cpt.y - s.press_cursor.1).abs());
                        let moved = drag > 4;

                        if s.pressed_hit {
                            s.press = Press::Bounce(0.0); // 松手回弹
                            // 松手音与「是否点击」解耦：点击、长按、拖动、甩出
                            // 都应该有完整的「拿起 → 放下」反馈（圆宝组只有按下那一声）。
                            audio::release(drag);
                        } else {
                            s.press = Press::Idle;
                        }

                        if moved {
                            if let Some((vx, vy)) = physics::estimate_throw(&s.trail) {
                                s.body = Some(physics::Body {
                                    x: wr.left as f64,
                                    y: wr.top as f64,
                                    vx,
                                    vy,
                                });
                            }
                        }
                        s.trail.clear();
                        s.pressed_hit = false;
                    }
                }
            }
            LRESULT(0)
        }

        WM_RBUTTONUP => {
            let cmd = pet_menu(hwnd);
            match cmd {
                30 => {
                    match update::check() {
                        Ok(Some(rel)) => {
                            if let Ok(mut m) = PENDING_UPDATE.get_or_init(|| Mutex::new(None)).lock() {
                                *m = Some(rel);
                            }
                            prompt_update(hwnd);
                        }
                        Ok(None) => {
                            let msg = format!("已经是最新版本 v{}", update::current_version());
                            let mw: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                            let tw: Vec<u16> = "圆宝桌宠".encode_utf16().chain(std::iter::once(0)).collect();
                            MessageBoxW(hwnd, PCWSTR(mw.as_ptr()), PCWSTR(tw.as_ptr()), MB_OK | MB_ICONINFORMATION);
                        }
                        Err(e) => {
                            let msg = format!("检查更新失败：{e}\n\n网络不通时可以先开代理再试。");
                            let mw: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                            let tw: Vec<u16> = "圆宝桌宠".encode_utf16().chain(std::iter::once(0)).collect();
                            MessageBoxW(hwnd, PCWSTR(mw.as_ptr()), PCWSTR(tw.as_ptr()), MB_OK | MB_ICONWARNING);
                        }
                    }
                }
                // 范围必须覆盖全部设置项：4.37 那个「命令 ID 越界被静默丢弃」的坑
                // 就是漏了新增项。加菜单项时记得同步这里。

                11 => {
                    let locked = ST
                        .get()
                        .and_then(|st| st.lock().ok().map(|g| g.locked))
                        .unwrap_or(false);
                    set_locked(hwnd, !locked);
                }
                12 => to_bottom_right(hwnd),
                13 => {
                    let _ = DestroyWindow(hwnd);
                }
                // 设置项的命令 ID 是动态分配的，这里统一查表
                _ => {
                    let _ = handle_menu_cmd(cmd);
                }
            }
            LRESULT(0)
        }

        tray::WM_TRAY => {
            let ev = lparam.0 as u32;
            if ev == WM_RBUTTONUP || ev == WM_LBUTTONUP {
                let visible = IsWindowVisible(hwnd).as_bool();
                let locked = ST
                    .get()
                    .and_then(|st| st.lock().ok().map(|g| g.locked))
                    .unwrap_or(false);
                match tray::show_menu(hwnd, visible, locked) {
                    tray::Action::Settings => {
                        let sub = settings_submenu();
                        let mut pt = POINT::default();
                        let _ = GetCursorPos(&mut pt);
                        let cmd = TrackPopupMenu(
                            sub, TPM_RETURNCMD | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, None,
                        );
                        let _ = DestroyMenu(sub);
                        let _ = handle_menu_cmd(cmd.0 as u16);
                    }
                    tray::Action::ToggleLock => set_locked(hwnd, !locked),
                    tray::Action::ToggleVisible => {
                        if visible {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        } else {
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        }
                    }
                    tray::Action::Quit => {
                        let _ = DestroyWindow(hwnd);
                    }
                    tray::Action::None => {}
                }
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            // 退出前记一次位置：用户拖到哪、下次就在哪起来。
            save_config();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            tray::remove();
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
