//! 圆宝桌宠 · 原生绘制实现
//!
//! 不依赖 WebView：一个 WS_EX_LAYERED 的分层窗口 + `UpdateLayeredWindow` 逐帧
//! 贴 BGRA 位图。目标是内存降到 40 MB 以内（HTML 版约 180 MB，其中 110 MB 是
//! WebView2 的固定开销）。
//!
//! 当前状态：窗口 / 帧加载 / 角度跟随 / 甩出物理 / 自绘菜单 / 托盘 / 开机自启 /
//! 自更新 / 设置持久化，全部装进一个 exe（帧与音效是 PE 资源）。

#![windows_subsystem = "windows"]

mod audio;
mod config;
mod menu_ui;
mod physics;
mod settings;
mod update;
mod tray;

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

// ---- 素材与几何参数：一律来自 assets/settings.json，代码不持有字面量 ----
fn frame_count() -> usize {
    settings::get().frames.count
}
fn main_start() -> usize {
    settings::get().frames.main_start
}
fn pad() -> i32 {
    settings::get().geometry.pad
}
fn focus_y() -> f64 {
    settings::get().geometry.focus_y
}
fn idle_drift() -> usize {
    settings::get().frames.idle_drift_frames
}
fn idle_enter() -> f64 {
    settings::get().geometry.idle_enter
}
fn idle_exit_factor() -> f64 {
    settings::get().geometry.idle_exit_factor
}
fn hit_alpha() -> u8 {
    settings::get().geometry.hit_alpha
}

/// 猫的逻辑显示边长（设置面板可改）
static DISPLAY_I: AtomicI32 = AtomicI32::new(0);

fn disp() -> i32 {
    DISPLAY_I.load(Ordering::Relaxed)
}

fn win() -> i32 {
    disp() + pad() * 2
}

struct Frame {
    /// 预乘 alpha 的 BGRA，尺寸 win()×win()
    bgra: Vec<u8>,
}

struct Dib {
    memdc: HDC,
    /// 只是持有句柄保证位图不被回收，代码里不直接读
    #[allow(dead_code)]
    bmp: HBITMAP,
    bits: *mut u32,
}

struct State {
    /// 按需填充的帧槽位：未加载为 None（省内存），远处帧会被淘汰
    frames: Vec<Option<Frame>>,
    cur: usize,
    dib: Option<Dib>,
    hwnd: HWND,

    // 拖动 / 甩出
    dragging: bool,
    grab_dx: i32,
    grab_dy: i32,
    trail: Vec<(f64, f64, f64)>, // 光标轨迹 (x, y, t_ms)
    body: Option<physics::Body>,
    last_tick: std::time::Instant,

    // 按压动画阶段（关键帧曲线对齐 Web 版）
    press: Press,
    pressed_hit: bool,
    press_win: (i32, i32),
    press_cursor: (i32, i32),
    in_idle: bool,
    locked: bool,

    // 可调参数（设置面板通过 IPC 改）
    opacity: f64,
    mirror: bool,
    idle_radius: f64,

    // 上一帧的"已绘制状态"快照，用于跳过无变化的重绘
    last_cur: usize,
    last_sx: f64,
    last_sy: f64,
    last_pos: (i32, i32),
    last_op: f64,
    last_mirror: bool,
    /// 最近一次成功绘制的帧号（目标帧未就绪时兜底）
    last_frame: Option<usize>,
}

unsafe impl Send for State {}

static ST: OnceLock<Mutex<State>> = OnceLock::new();

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

unsafe fn screen_size() -> (i32, i32) {
    let mut r = RECT::default();
    let _ = GetClientRect(GetDesktopWindow(), &mut r);
    (r.right, r.bottom)
}

/// 从 exe 位置逐级向上找 web/frames（开发期在 native/target/release，发布时可能并排）
fn frames_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..6 {
            let Some(d) = dir else { break };
            let cand = d.join("web").join("frames");
            if cand.is_dir() {
                return cand;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    std::path::PathBuf::from("web/frames")
}

/// 从自身 PE 资源读 RT_RCDATA。exe 被复制到任何目录都能拿到帧和音效，
/// 不再依赖旁边的 web/ 目录 —— 这是「单文件分发」的关键。
pub(crate) unsafe fn resource(id: u16) -> Option<&'static [u8]> {
    use windows::Win32::System::LibraryLoader::{
        FindResourceW, LoadResource, LockResource, SizeofResource,
    };
    let hinst = GetModuleHandleW(None).ok()?;
    let name = PCWSTR(id as usize as *const u16);
    let typ = PCWSTR(10usize as *const u16); // RT_RCDATA
    let h = FindResourceW(hinst, name, typ);
    if h.is_invalid() {
        return None;
    }
    let g = LoadResource(hinst, h).ok()?;
    let ptr = LockResource(g);
    let n = SizeofResource(hinst, h);
    if ptr.is_null() || n == 0 {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr as *const u8, n as usize))
}

/// 取第 i 帧的 webp 字节（长度表 + 拼接 blob）
fn frame_bytes(i: usize) -> Option<&'static [u8]> {
    unsafe {
        let lens = resource(200)?;
        let blob = resource(201)?;
        if i >= lens.len() / 4 {
            return None;
        }
        let mut off = 0usize;
        for k in 0..i {
            off += u32::from_le_bytes(lens[k * 4..k * 4 + 4].try_into().ok()?) as usize;
        }
        let len = u32::from_le_bytes(lens[i * 4..i * 4 + 4].try_into().ok()?) as usize;
        blob.get(off..off + len)
    }
}

/// 读单帧并缩放到 disp()，转成预乘 alpha 的 BGRA（win()×win()，居中）
///
/// 画质三要点（都不增加常驻内存，因为只在启动预载时做一次）：
/// 1. **先预乘再缩放**：在非预乘 RGBA 上插值，全透明像素的底色会渗进边缘，
///    形成暗绿描边（老大看到的「绿幕痕迹」）。预乘空间插值没有这个问题。
/// 2. **CatmullRom**：比 Triangle 锐利，又不像 Lanczos 那样在细线（胡子）旁产生振铃。
/// 3. **边缘去污染 + despill**：源素材的半透明边缘是抠图残留的暗色/微绿，
///    把它的 RGB 换成邻近不透明像素的颜色，再压掉残余绿偏。
unsafe fn load_one(i: usize) -> Option<Frame> {
    // 优先读内嵌资源；开发期若资源缺失再回退到磁盘文件
    let src = if let Some(bytes) = frame_bytes(i) {
        image::load_from_memory(bytes).ok()?.to_rgba8()
    } else {
        let path = frames_dir().join(format!("{i:04}.webp"));
        image::open(path.as_path()).ok()?.to_rgba8()
    };
    let (w, h) = src.dimensions();
    let snapshot = src.as_raw().clone();
    let mut pm = src;

    // 边缘去污染：只对半透明像素做，半径 2 内找最不透明的邻居取色
    for y in 0..h {
        for x in 0..w {
            let si = (((y * w) + x) * 4) as usize;
            let a = snapshot[si + 3];
            if a == 0 || a >= 250 {
                continue;
            }
            let mut best_a = 0u8;
            let mut best = [0u8; 3];
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ((((ny as u32) * w) + nx as u32) * 4) as usize;
                    if snapshot[j + 3] > best_a {
                        best_a = snapshot[j + 3];
                        best = [snapshot[j], snapshot[j + 1], snapshot[j + 2]];
                    }
                }
            }
            if best_a >= 200 {
                let px = pm.get_pixel_mut(x, y);
                px[0] = best[0];
                px[1] = best[1];
                px[2] = best[2];
            }
        }
    }

    // despill + 预乘（都在原始尺寸做）
    for px in pm.pixels_mut() {
        let (r, g0, b, a) = (
            px[0] as u32,
            px[1] as u32,
            px[2] as u32,
            px[3] as u32,
        );
        let mean_rb = (r + b) / 2;
        let g = if g0 > mean_rb + 6 {
            mean_rb + (g0 - mean_rb) / 4
        } else {
            g0
        };
        px[0] = ((r * a) / 255) as u8;
        px[1] = ((g * a) / 255) as u8;
        px[2] = ((b * a) / 255) as u8;
    }

    let dyn_img = image::DynamicImage::ImageRgba8(pm);
    let small = dyn_img
        .resize_exact(
            disp() as u32,
            disp() as u32,
            image::imageops::FilterType::CatmullRom,
        )
        .to_rgba8();

    let mut bgra = vec![0u8; (win() * win() * 4) as usize];
    for y in 0..disp() as u32 {
        for x in 0..disp() as u32 {
            let p = small.get_pixel(x, y).0;
            let idx = (((y as i32 + pad()) * win() + (x as i32 + pad())) * 4) as usize;
            bgra[idx] = p[2];
            bgra[idx + 1] = p[1];
            bgra[idx + 2] = p[0];
            bgra[idx + 3] = p[3];
        }
    }
    Some(Frame { bgra })
}

impl State {
    /// 确保第 i 帧已加载（全量预载后通常直接命中）
    unsafe fn ensure_frame(&mut self, i: usize) {
        let i = i % frame_count();
        if self.frames[i].is_none() {
            self.frames[i] = load_one(i);
        }
    }
}

unsafe fn ensure_dib_locked(s: &mut State) {
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
unsafe fn present_locked_at(
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

/// 按鼠标方位角换算帧号（调用方已持有锁）
unsafe fn update_frame(s: &mut State) {
    let mut wr = RECT::default();
    let _ = GetWindowRect(s.hwnd, &mut wr);
    let cx = (wr.left + wr.right) as f64 / 2.0;
    let cy = wr.top as f64 + (wr.bottom - wr.top) as f64 * focus_y();

    let mut pt = POINT::default();
    if GetCursorPos(&mut pt).is_err() {
        return;
    }

    let dx = pt.x as f64 - cx;
    let dy = pt.y as f64 - cy;
    if (dx * dx + dy * dy).sqrt() < 20.0 {
        return; // 贴太近时角度没意义，保持当前帧
    }

    let mut deg = ((dy.atan2(dx).to_degrees() + 90.0) % 360.0 + 360.0) % 360.0;
    // 水平镜像时画面是翻转的，角度要同步取反，否则猫会朝相反方向看
    if s.mirror {
        deg = (360.0 - deg) % 360.0;
    }
    let start = main_start();
    let last = frame_count() - 1;
    let pos = start as f64 + deg / 360.0 * (last - start) as f64;
    s.cur = (pos.round() as usize).clamp(0, last);
}

/// 点是否落在猫的实体像素上（读当前帧的 alpha）
unsafe fn hit_test(s: &mut State, px: i32, py: i32) -> bool {
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

#[derive(Clone, Copy)]
enum Press {
    Idle,
    Down(f64),   // 按下，0→0.07s
    Bounce(f64), // 松手回弹，0→0.34s
    Land(f64, f64), // 落地挤压：(进度 0→0.22s, 压扁深度 0.2~0.45)
}

fn bezier_component(t: f64, p1: f64, p2: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

fn bezier_derivative(t: f64, p1: f64, p2: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// 精确求 CSS cubic-bezier(x1,y1,x2,y2) 在给定时间进度 x 上的输出。
/// CSS 的 timing-function 是先按 x 反解参数 t，再取 y，不能拿 ease-out 之类的近似糊弄。
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let mut t = x;
    for _ in 0..8 {
        let err = bezier_component(t, x1, x2) - x;
        if err.abs() < 1e-6 {
            break;
        }
        let d = bezier_derivative(t, x1, x2);
        if d.abs() < 1e-6 {
            break;
        }
        t = (t - err / d).clamp(0.0, 1.0);
    }
    bezier_component(t, y1, y2)
}

/// CSS `ease-out` = cubic-bezier(0, 0, .58, 1) —— 用于 squeeze
fn css_ease_out(k: f64) -> f64 {
    cubic_bezier(0.0, 0.0, 0.58, 1.0, k)
}

/// Web 版 bounce / squash 的 cubic-bezier(.3, .9, .4, 1)
/// 前段冲得快、尾巴拖得长，所以观感「慢而丝滑」
fn css_bounce(k: f64) -> f64 {
    cubic_bezier(0.3, 0.9, 0.4, 1.0, k)
}

/// 按 Web 版 CSS 关键帧插值；每个区间都套用 animation-timing-function
fn keys(t: f64, pts: &[(f64, f64, f64)], ease: fn(f64) -> f64) -> (f64, f64) {
    for w in pts.windows(2) {
        if t <= w[1].0 {
            let k = ((t - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0);
            let e = ease(k);
            return (
                w[0].1 + (w[1].1 - w[0].1) * e,
                w[0].2 + (w[1].2 - w[0].2) * e,
            );
        }
    }
    let last = pts[pts.len() - 1];
    (last.1, last.2)
}

/// 落地挤压深度随冲击速度变化（对齐 dsh-pet 的 landingSquash）：
/// 轻落压 20%，猛砸压到 45%；再叠加 ease-out-back 的过冲回弹，最多拉伸到 1.12。
fn impact_depth(impact: f64) -> f64 {
    let t = ((impact.abs() - 300.0) / (1500.0 - 300.0)).clamp(0.0, 1.0);
    let squash = (1.0 - 0.45 * t).min(0.8);
    1.0 - squash
}

fn land_scale(u: f64, depth: f64) -> (f64, f64) {
    let d = depth.clamp(0.0, 0.45);
    let sy = if u < 0.45 {
        // 前 45%：二次加速压到最扁
        let p = (u / 0.45).clamp(0.0, 1.0);
        1.0 - d * p * p
    } else {
        // 后 55%：ease-out-back 回到 1，允许过冲
        let p = ((u - 0.45) / 0.55).clamp(0.0, 1.0);
        let c1 = 1.70158;
        let c3 = c1 + 1.0;
        let f = 1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2);
        ((1.0 - d) + d * f.max(0.0)).min(1.12)
    };
    // 纵向压扁时横向补偿变宽，体积感更自然
    (1.0 + (1.0 - sy) * 0.85, sy)
}

/// 返回 (scaleX, scaleY)，完全对应 web/pet/style.css 的 squeeze / bounce / squash
fn press_scale(p: Press) -> (f64, f64) {
    match p {
        Press::Idle => (1.0, 1.0),
        Press::Down(t) => {
            let e = css_ease_out(t / 0.07);
            (1.0 + 0.17 * e, 1.0 - 0.24 * e) // → (1.17, 0.76)
        }
        Press::Bounce(t) => keys(
            t / 0.34,
            &[
                (0.0, 1.17, 0.76),
                (0.45, 0.96, 1.06),
                (0.75, 1.02, 0.99),
                (1.0, 1.0, 1.0),
            ],
            css_bounce,
        ),
        Press::Land(t, d) => land_scale(t / 0.22, d),
    }
}

impl State {
    fn tick_press(&mut self, dt: f64) {
        match self.press {
            Press::Down(t) => self.press = Press::Down((t + dt).min(0.07)),
            Press::Bounce(t) => {
                let n = t + dt;
                self.press = if n >= 0.34 { Press::Idle } else { Press::Bounce(n) };
            }
            Press::Land(t, d) => {
                let n = t + dt;
                self.press = if n >= 0.22 { Press::Idle } else { Press::Land(n, d) };
            }
            Press::Idle => {}
        }
    }
}

// ---- 开机自启（HKCU\...\Run，不需要管理员）----

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "YuanBaoDeskPet";

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn autostart_on() -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };
    let mut k = HKEY::default();
    let key_w = to_wide(RUN_KEY);
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        PCWSTR(key_w.as_ptr()),
        0,
        KEY_QUERY_VALUE,
        &mut k,
    )
    .is_err()
    {
        return false;
    }
    let name = to_wide(RUN_VALUE);
    let mut cb: u32 = 0;
    let ok = RegQueryValueExW(
        k,
        PCWSTR(name.as_ptr()),
        None,
        None,
        None,
        Some(&mut cb),
    )
    .is_ok()
        && cb > 0;
    let _ = RegCloseKey(k);
    ok
}

unsafe fn set_autostart(on: bool) -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };
    let mut k = HKEY::default();
    let key_w = to_wide(RUN_KEY);
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        PCWSTR(key_w.as_ptr()),
        0,
        KEY_SET_VALUE,
        &mut k,
    )
    .is_err()
    {
        return false;
    }
    let name = to_wide(RUN_VALUE);
    let ok = if on {
        let exe = std::env::current_exe().unwrap_or_default();
        // 用引号包住，路径带空格也能正确启动
        let val = format!("\"{}\"", exe.display());
        let w: Vec<u16> = val.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes =
            std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * std::mem::size_of::<u16>());
        RegSetValueExW(k, PCWSTR(name.as_ptr()), 0, REG_SZ, Some(bytes)).is_ok()
    } else {
        RegDeleteValueW(k, PCWSTR(name.as_ptr())).is_ok()
    };
    let _ = RegCloseKey(k);
    ok
}

fn s_opacity() -> f64 {
    ST.get()
        .and_then(|st| st.lock().ok().map(|g| g.opacity))
        .unwrap_or(1.0)
}
fn s_mirror() -> bool {
    ST.get()
        .and_then(|st| st.lock().ok().map(|g| g.mirror))
        .unwrap_or(false)
}

/// 设置菜单里的一项动作。
///
/// 命令 ID 不再手写：构建菜单时按顺序分配并登记，wndproc 只查表。
/// 这样「加一个尺寸档/加一个音效组」不会再把命令 ID 写到分发范围之外。
#[derive(Clone)]
enum MenuCmd {
    Size(i32),
    Opacity(i32),
    Mirror,
    Autostart,
    Reset,
    Sound(String),
}

/// 本轮菜单的命令表。菜单是同步弹出的，重开时整体重建。
static MENU_CMDS: OnceLock<Mutex<Vec<(u16, MenuCmd)>>> = OnceLock::new();

/// 设置项的 ID 命名空间（主菜单用 11/12/13/20/30，不冲突即可）
const MENU_ID_BASE: u16 = 100;

fn menu_cmds() -> &'static Mutex<Vec<(u16, MenuCmd)>> {
    MENU_CMDS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 把当前设置与窗口位置写回 config.json（调用方不能已持有 ST 锁）
unsafe fn save_config() {
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

/// 设置子菜单：自绘，带分组标题与主题色单选圆点。
/// 档位与音效组都来自配置，命令 ID 现场分配并登记。
unsafe fn settings_submenu() -> HMENU {
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
    menu_ui::add(m, 0, "", menu_ui::Kind::Separator);
    let id = push(MenuCmd::Reset);
    menu_ui::add(m, id as u32, "恢复默认", menu_ui::Kind::Normal);
    m
}

/// 设置项分发：只查表，不认识就返回 false。
/// 命令 ID 由 settings_submenu() 现场分配，这里不再依赖任何区间字面量。
unsafe fn handle_menu_cmd(cmd: u16) -> bool {
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
    }
    // 每次经由菜单改动都立刻落盘：桌宠没有「保存」按钮，
    // 用户也不该为了留住设置而正常退出一次。
    save_config();
    true
}

/// 发现新版本时的确认框；确认后下载并重启
unsafe fn prompt_update(hwnd: HWND) {
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
unsafe fn pet_menu(hwnd: HWND) -> u16 {
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

unsafe fn set_locked(hwnd: HWND, locked: bool) {
    let mut style = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
    if locked {
        style |= WS_EX_TRANSPARENT;
    } else {
        style &= !WS_EX_TRANSPARENT;
    }
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style.0 as isize);
    if let Some(st) = ST.get() {
        if let Ok(mut s) = st.lock() {
            s.locked = locked;
        }
    }
}

unsafe fn to_bottom_right(hwnd: HWND) {
    let (sw, sh) = screen_size();
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        sw - win() - 40,
        sh - win() - 100,
        0,
        0,
        SWP_NOSIZE | SWP_NOACTIVATE,
    );
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

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// 每 16ms 一次：物理 → 角度 → 绘制（全程只锁一次，避免重入死锁）
unsafe fn on_tick() {
    let Some(st) = ST.get() else { return };
    let Ok(mut s) = st.lock() else { return };

    let now = std::time::Instant::now();
    let dt = now.duration_since(s.last_tick).as_secs_f64();
    s.last_tick = now;

    // 物理只算位置，不单独 SetWindowPos —— 位置通过 UpdateLayeredWindow 的 pptDst
    // 一并提交，省掉一半的系统调用（之前甩出时每帧两次调用，明显掉帧）。
    let mut body_pos: Option<(i32, i32)> = None;
    let mut land_hit = false;
    let mut land_impact = 0.0;
    if s.body.is_some() {
        let (sw, sh) = screen_size();
        let bounds = physics::Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: (sw - win()) as f64,
            max_y: (sh - win()) as f64,
        };
        if let Some(body) = s.body.as_mut() {
            let r = body.step(dt, bounds);
            body_pos = Some((body.x as i32, body.y as i32));
            // Web 版 pet:landed 用 280 做阈值，这里保持一致
            land_hit = r.landed && r.impact > 280.0;
            land_impact = r.impact;
            if r.at_rest {
                s.body = None;
            }
        }
    }
    // 贴底时 landed 每帧都为 true，必须只在动画未进行时触发，
    // 否则 Land 会被不断重置、动画永不结束、每帧强制重绘。
    if land_hit && !matches!(s.press, Press::Land(..) | Press::Bounce(_)) {
        s.press = Press::Land(0.0, impact_depth(land_impact));
    }

    // 轨迹改为按时间采样（而不是只在 mousemove 时记）：
    // 否则停住手之后轨迹里还留着"快速拖动"那一段，松手会被判成高速甩出（弹弓效应）。
    if s.dragging {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            let t = now_ms();
            s.trail.push((pt.x as f64, pt.y as f64, t));
            if s.trail.len() > 32 {
                s.trail.remove(0);
            }
        }
    }

    // 音效行为的时间推进：延迟触发、循环续播、淡出
    audio::tick();

    // 按压动画按 Web 版关键帧推进
    s.tick_press(dt);
    let scale = press_scale(s.press);

    if !s.dragging && s.body.is_some() {
        // 甩出飞行途中也要继续看着鼠标（Web 版就是一直跟的）
        s.in_idle = false;
        update_frame(&mut s);
    } else if !s.dragging && s.body.is_none() {
        // 鼠标贴到旋转中心附近时收势（低头），离开后恢复跟随。
        // 滞回：进 28px、出 48px，避免边界抖动导致反复切换。
        let mut wr = RECT::default();
        let _ = GetWindowRect(s.hwnd, &mut wr);
        let cx = (wr.left + wr.right) as f64 / 2.0;
        let cy = wr.top as f64 + (wr.bottom - wr.top) as f64 * focus_y();
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            let dx = pt.x as f64 - cx;
            let dy = pt.y as f64 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let r = s.idle_radius;
            if d < r {
                s.in_idle = true;
            } else if d > r * idle_exit_factor() {
                s.in_idle = false;
            }
        }

        if s.in_idle {
            // 回归路径要走环形最短方向，不能一直往下递减。
            // 主环是 32..360（32=上看，顺时针递增，360 与 32 相邻）；
            // 例如从左上(≈319)顺时针到 32 只有 41 帧，逆时针要 287 帧。
            let start = main_start();
            let last = frame_count() - 1;
            let step = idle_drift();
            if s.cur <= start {
                // 已到"上看"，开始低头（帧 31..0 是抬头动画的反向）
                s.cur = s.cur.saturating_sub(step);
            } else {
                let dist_cw = last - s.cur; // 顺时针：cur -> last(=start)
                let dist_ccw = s.cur - start; // 逆时针：cur -> start
                if dist_cw <= dist_ccw {
                    s.cur = if s.cur >= last {
                        start
                    } else {
                        (s.cur + step).min(last)
                    };
                } else {
                    s.cur = s.cur.saturating_sub(step).max(start);
                }
            }
        } else {
            update_frame(&mut s);
        }
    }
    // 按需重绘：帧号 / 挤压值 / 窗口位置 / 透明度 / 镜像 都没变就跳过。
    // 桌宠大部分时间静止，16ms 无条件 present 等于白烧 CPU。
    let pos_now = match body_pos {
        Some(p) => p,
        None => {
            let mut wr2 = RECT::default();
            let _ = GetWindowRect(s.hwnd, &mut wr2);
            (wr2.left, wr2.top)
        }
    };
    // 动画期间强制每帧重绘：回弹后半段每帧 scale 变化可能小于阈值，
    // 若走按需跳过会让动画提前静止，观感就是"后面突然结束"。
    let animating = !matches!(s.press, Press::Idle);
    let changed = animating
        || s.cur != s.last_cur
        || (scale.0 - s.last_sx).abs() > 0.002
        || (scale.1 - s.last_sy).abs() > 0.002
        || pos_now != s.last_pos
        || (s.opacity - s.last_op).abs() > 0.001
        || s.mirror != s.last_mirror;

    if changed {
        present_locked_at(&mut s, body_pos, scale);
        s.last_cur = s.cur;
        s.last_sx = scale.0;
        s.last_sy = scale.1;
        s.last_pos = pos_now;
        s.last_op = s.opacity;
        s.last_mirror = s.mirror;
    }

    // 不再用 SetWindowRgn 裁剪窗口：按压压扁时猫会横向撑开，
    // 超出旧轮廓就会被切掉。透明区的点击由 hit_test 的 alpha 判定处理。
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
