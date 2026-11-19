//! 把 assets/icon.ico 嵌进 exe，让资源管理器显示应用图标。
//!
//! 不用 winres：它要靠 reg.exe 找 Windows SDK 的 rc.exe，在受限环境里会失败。
//! 这里直接按 PE 资源文件格式生成 .res（RT_ICON + RT_GROUP_ICON），
//! 再交给链接器，纯 Rust、零外部工具。

use std::fs;
use std::path::PathBuf;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_RCDATA: u16 = 10;
const RT_VERSION: u16 = 16;

// RCDATA 资源 ID（运行时用 FindResourceW 按序号取）
const ID_FRAME_LENS: u16 = 200; // 每帧长度表：361 × u32
const ID_FRAME_BLOB: u16 = 201; // 所有 webp 顺序拼接
const ID_SND_DOWN: u16 = 202;
const ID_SND_UP: u16 = 203;
const ID_SND_MEOW: u16 = 204;
const FRAME_COUNT: usize = 361;

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn push_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// 一个资源块：DataSize/HeaderSize + ordinal type/name + 版本信息 + 数据
fn resource_block(out: &mut Vec<u8>, type_id: u16, name_id: u16, data: &[u8]) {
    push_u32(out, data.len() as u32);
    push_u32(out, 32); // HeaderSize：含 DataSize/HeaderSize 自身
    push_u16(out, 0xFFFF); // type 用序号
    push_u16(out, type_id);
    push_u16(out, 0xFFFF); // name 用序号
    push_u16(out, name_id);
    push_u32(out, 0); // DataVersion
    push_u16(out, 0x1030); // MemoryFlags
    push_u16(out, 0x0409); // LanguageId
    push_u32(out, 0); // Version
    push_u32(out, 0); // Characteristics
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}


fn align4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

/// VS_VERSIONINFO 的通用块：wLength 先占位，收尾回填
fn vs_block(key: &str, value_len: u16, wtype: u16, value: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0u16.to_le_bytes()); // wLength 占位
    v.extend_from_slice(&value_len.to_le_bytes());
    v.extend_from_slice(&wtype.to_le_bytes());
    for c in key.encode_utf16() {
        v.extend_from_slice(&c.to_le_bytes());
    }
    v.extend_from_slice(&0u16.to_le_bytes());
    align4(&mut v);
    if !value.is_empty() {
        v.extend_from_slice(value);
        align4(&mut v);
    }
    for ch in children {
        v.extend_from_slice(ch);
        align4(&mut v);
    }
    let len = (v.len() as u16).to_le_bytes();
    v[0..2].copy_from_slice(&len);
    v
}

fn vs_string(key: &str, val: &str) -> Vec<u8> {
    let w: Vec<u16> = val.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    vs_block(key, w.len() as u16, 1, &bytes, &[])
}

/// 手写 VERSIONINFO（RT_VERSION），让 exe 在资源管理器里可见、运行时也能读到自身版本
fn version_info(ver: &str) -> Vec<u8> {
    let p: Vec<u32> = ver.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let g = |i: usize| *p.get(i).unwrap_or(&0);
    let (a, b, c, d) = (g(0), g(1), g(2), g(3));

    let mut fixed: Vec<u8> = Vec::with_capacity(52);
    for x in [
        0xFEEF04BDu32,      // dwSignature
        0x00010000,         // dwStrucVersion
        (a << 16) | b,      // dwFileVersionMS
        (c << 16) | d,      // dwFileVersionLS
        (a << 16) | b,      // dwProductVersionMS
        (c << 16) | d,      // dwProductVersionLS
        0x3F,               // dwFileFlagsMask
        0,                  // dwFileFlags
        0x40004,            // dwFileOS = VOS_NT_WINDOWS32
        1,                  // dwFileType = VFT_APP
        0,                  // dwFileSubtype
        0,                  // dwFileDateMS
        0,                  // dwFileDateLS
    ] {
        fixed.extend_from_slice(&x.to_le_bytes());
    }

    let strings = vs_block(
        "StringFileInfo",
        0,
        1,
        &[],
        &[vs_block(
            "040904B0",
            0,
            1,
            &[],
            &[
                vs_string("FileVersion", ver),
                vs_string("ProductVersion", ver),
                vs_string("ProductName", "圆宝桌宠"),
                vs_string("FileDescription", "圆宝桌宠 · 原生版"),
            ],
        )],
    );
    let vars = vs_block(
        "VarFileInfo",
        4,
        1,
        &[],
        &[vs_block("Translation", 4, 0, &[0x09, 0x04, 0xB0, 0x04], &[])],
    );

    vs_block("VS_VERSION_INFO", 52, 0, &fixed, &[strings, vars])
}

fn build_res(ico: &[u8]) -> Option<Vec<u8>> {
    if ico.len() < 22 || ico[0..4] != [0, 0, 1, 0] {
        return None;
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    // .res 文件必须以一个 32 字节的 null 资源头开头
    let mut out = Vec::new();
    push_u32(&mut out, 0); // DataSize = 0
    push_u32(&mut out, 32); // HeaderSize
    push_u16(&mut out, 0xFFFF);
    push_u16(&mut out, 0); // type ordinal 0
    push_u16(&mut out, 0xFFFF);
    push_u16(&mut out, 0); // name ordinal 0
    push_u32(&mut out, 0); // DataVersion
    push_u16(&mut out, 0); // MemoryFlags
    push_u16(&mut out, 0); // LanguageId
    push_u32(&mut out, 0); // Version
    push_u32(&mut out, 0); // Characteristics

    let mut group = Vec::new();
    push_u16(&mut group, 0); // idReserved
    push_u16(&mut group, 1); // idType = icon
    push_u16(&mut group, count as u16);

    for i in 0..count {
        let e = 6 + i * 16;
        if e + 16 > ico.len() {
            return None;
        }
        let size = u32::from_le_bytes(ico[e + 8..e + 12].try_into().ok()?) as usize;
        let off = u32::from_le_bytes(ico[e + 12..e + 16].try_into().ok()?) as usize;
        if off + size > ico.len() {
            return None;
        }
        let id = (i + 1) as u16;
        resource_block(&mut out, RT_ICON, id, &ico[off..off + size]);

        group.push(ico[e]); // bWidth
        group.push(ico[e + 1]); // bHeight
        group.push(ico[e + 2]); // bColorCount
        group.push(ico[e + 3]); // bReserved
        group.extend_from_slice(&ico[e + 4..e + 8]); // wPlanes, wBitCount
        push_u32(&mut group, size as u32); // dwBytesInRes
        push_u16(&mut group, id); // nID
    }

    resource_block(&mut out, RT_GROUP_ICON, 1, &group);

    // 把帧序列与音效也塞进 RCDATA，让 exe 真正单文件可分发
    let frames = PathBuf::from("../web/frames");
    let mut lens = Vec::with_capacity(FRAME_COUNT * 4);
    let mut blob = Vec::new();
    for i in 0..FRAME_COUNT {
        let p = frames.join(format!("{i:04}.webp"));
        let Ok(d) = fs::read(&p) else {
            println!("cargo:warning=missing frame: {}", p.display());
            return None;
        };
        lens.extend_from_slice(&(d.len() as u32).to_le_bytes());
        blob.extend_from_slice(&d);
    }
    resource_block(&mut out, RT_RCDATA, ID_FRAME_LENS, &lens);
    resource_block(&mut out, RT_RCDATA, ID_FRAME_BLOB, &blob);

    let sounds = PathBuf::from("../web/sounds");
    let down = fs::read(sounds.join("pat_down.wav")).unwrap_or_default();
    let up = fs::read(sounds.join("pat_up.wav")).unwrap_or_default();
    let meow = fs::read(sounds.join("meow.wav")).unwrap_or_default();
    resource_block(&mut out, RT_RCDATA, ID_SND_DOWN, &down);
    resource_block(&mut out, RT_RCDATA, ID_SND_UP, &up);
    resource_block(&mut out, RT_RCDATA, ID_SND_MEOW, &meow);

    // 版本资源：让 exe 自带版本标识，避免"自己更新自己"
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".into());
    resource_block(&mut out, RT_VERSION, 1, &version_info(&ver));

    Some(out)
}

fn main() {
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    println!("cargo:rerun-if-changed=../web/frames");
    println!("cargo:rerun-if-changed=../web/sounds");
    let icon_path = PathBuf::from("../assets/icon.ico");
    let Ok(ico) = fs::read(&icon_path) else {
        println!("cargo:warning=icon not found: {}", icon_path.display());
        return;
    };
    let Some(res) = build_res(&ico) else {
        println!("cargo:warning=invalid .ico file");
        return;
    };

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let res_path = out_dir.join("yuanbao_icon.res");
    if fs::write(&res_path, res).is_err() {
        println!("cargo:warning=failed to write .res");
        return;
    }
    // link.exe 直接接受 .res 作为输入
    println!("cargo:rustc-link-arg-bins={}", res_path.display());
}
