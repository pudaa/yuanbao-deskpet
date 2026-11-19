// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::app::state::*;
use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::*;

/// 从 exe 位置逐级向上找 web/frames（开发期在 native/target/release，发布时可能并排）
pub(crate) fn frames_dir() -> std::path::PathBuf {
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
pub(crate) fn frame_bytes(i: usize) -> Option<&'static [u8]> {
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
pub(crate) unsafe fn load_one(i: usize) -> Option<Frame> {
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
    pub(crate) unsafe fn ensure_frame(&mut self, i: usize) {
        let i = i % frame_count();
        if self.frames[i].is_none() {
            self.frames[i] = load_one(i);
        }
    }
}
