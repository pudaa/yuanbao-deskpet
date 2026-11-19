# 圆宝桌宠

把家里的猫做成桌面宠物：素材来自一段绿幕转头视频，抠成 361 帧，按鼠标方位角实时转头，可以拖、可以甩、可以摸。

- **主力：原生绘制版**（纯 Win32 + GDI，零 WebView）：`dist/圆宝桌宠-单文件.exe`，约 7.3 MB，双击即可用
- 备选：WebView 版 `dist/YuanBaoDeskPet.exe`（Tauri 2，约 9.8 MB），保留作对照
- 单文件：361 帧 WebP、3 段音效、图标全部作为 PE 资源嵌进 exe；MCI 音效首次播放时释放到 `%TEMP%`
- 常驻内存约 55 MB，静止 CPU 0%；默认尺寸 160 px，可调 120–360
- 设置与窗口位置持久化在 `%APPDATA%\YuanBaoDeskPet\config.json`，重启后保持

## 功能

| 功能 | 说明 |
|---|---|
| 方位跟随 | 猫头始终看向鼠标方向（8 方位实测准确） |
| 中心收势 | 鼠标贴到猫身上时平滑低头，非强制回正 |
| 拖动 | 左键按住拖动窗口 |
| 甩出 | 快速拖动后松手，猫会被甩出去在桌面上弹跳，带重力/阻尼/落地挤压 |
| 摸猫 | 单击有音效反馈 + 弹跳动画；音效可切换「圆宝叫声」/「鸭子叫声」，默认圆宝叫声。音效由「行为表」驱动，长按/循环等行为已预留接口 |
| 右键菜单 | 设置 ▸ / 锁定（穿透）/ 回到右下角 / 圆宝再见；自绘「日系清新」风格，带圆角与发丝线 |
| 托盘 | 显隐、设置；与右键菜单共用同一套自绘菜单 |
| 设置 | 纯原生自绘子菜单：尺寸 160/220/280/340、不透明度 100/80/60%、水平镜像、开机自启、音效（圆宝叫声 / 鸭子叫声）、恢复默认（当前值打勾，实时生效并落盘） |

## 目录结构

```
yuanbao-deskpet/
├── native/               原生绘制版（主力，零 WebView）
│   ├── build.rs          生成 PE .res：图标 + 361 帧 + 3 个 wav + 版本信息
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs       WS_EX_LAYERED + UpdateLayeredWindow、物理、资源加载
│       ├── settings.rs   内置配置解析（assets/settings.json）
│       ├── config.rs     用户配置持久化（%APPDATA%\YuanBaoDeskPet\config.json）
│       ├── audio/mod.rs  音效行为引擎（触发 × 播放 × 条件）
│       ├── audio/mci.rs  MCI 封装
│       ├── menu_ui.rs    owner-draw 自绘菜单（日系清新）
│       ├── physics.rs    甩出物理
│       ├── tray.rs       托盘
│       └── update.rs     GitHub Releases 自更新
├── legacy/
│   └── webview/          WebView 对照版（半搁置，见 VALIDATION 4.14 起的迁移记录）
│       └── src-tauri/
│           ├── src/      入口、窗口、命令、托盘、光标轮询、甩出物理
│           ├── capabilities/  窗口权限
│           └── tauri.conf.json
│   （其前端仍复用根目录 web/，frontendDist = ../../../web）
├── web/                  前端（打包时整体内嵌）
│   ├── pet/              桌宠画布：index.html / renderer.js / style.css
│   ├── settings/         设置面板
│   ├── bubble/           气泡页（当前未启用，留给后续碎碎念）
│   ├── frames/           361 帧 WebP（0000–0360）
│   └── sounds/           抚摸音效 pat_down / pat_up
├── assets/               图标、frames.json（角度锚点表）、settings.json（档位/几何）、sounds.json（音效行为）
├── docs/
│   ├── ARCHITECTURE.md   数据流、音效引擎、配置分层
│   ├── CONSTRAINTS.md    改代码前必读的硬约束
│   ├── VALIDATION.md     完整验证文档（含所有踩坑记录）
│   └── figures/          接触表等图
├── tools/                素材处理脚本（Python）
├── archive/              V1 素材产物（仅归档）
├── dist/                 发布产物：圆宝桌宠-单文件.exe / YuanBaoDeskPet.exe
└── build/                临时验证产物（可删）
```

## 构建

```bash
cd D:\Codes\yuanbao-deskpet
export PATH="$HOME/.cargo/bin:$PATH"

# 主力：原生单文件版
cargo build --release --manifest-path native/Cargo.toml
# 产物：native/target/release/yuanbao-native.exe（复制成 dist/圆宝桌宠-单文件.exe 分发）

# 对照：WebView 版（半搁置）
cargo build --release --manifest-path legacy/webview/src-tauri/Cargo.toml
# 产物：legacy/webview/src-tauri/target/release/yuanbao-deskpet.exe
```

> 构建需要 `assets/icon.ico` 与 `web/frames/`、`web/sounds/` 就位（素材不入库，见下节）。

## 素材处理流水线

视频 → 抽帧 → 绿幕抠图 → WebP → 角度表：

```bash
# 1. 抽帧（24fps → build/raw）
ffmpeg -i 素材.mp4 -vsync 0 -start_number 0 build/raw/f_%04d.png

# 2. 抠图（颜色去污染，消除胡须黑边）
python tools/chroma_key.py --raw raw --all --size 384

# 3. 导出 WebP 到 web/frames
python tools/export_frames.py

# 4. 生成角度锚点表与接触表
python tools/build_sequence.py
```

## 角度索引

素材结构：帧 0–31 是"低头→抬头"的准备动作（只作极点 idle），帧 32–360 才是真正的 360° 转头。

```
θ   = atan2(dy, dx)                   鼠标相对猫中心的方位角
u   = (θ + π) / 2π
deg = ((u × 360) − 90 + 360) mod 360  0 = 正上方
pos = 32 + deg / 360 × 328            映射到帧 32..360
```

闭环处 `pos > 357` 时用 3° 把帧 360 交叉淡化到帧 32。

## 音效扩展

音效不是写死的枚举，而是 `assets/sounds.json` 里的一张**行为表**：

```json
{ "on": { "event": "press", "delayMs": 600 },
  "sound": "purr_loop",
  "play": { "mode": "loop", "until": "release", "fadeOutMs": 150 },
  "volume": 0.8, "instances": 2 }
```

- 触发：`press` / `release`，可带 `delayMs`（长按计时）或 `whileHeld`（按下即开始）
- 播放：`once` / `loop` + `until: release|forever`
- 条件：`when: { heldMsLt | heldMsGte | movedLt }`，用来表达"短按才响、长按不响"

加一段音效 = 把 `web/sounds/<sound>.wav` 放好 + 在 `sounds.json` 里加一条，**不用改 Rust**。
素材缺失时该行为静默、构建照常通过（只给一条 warning）。

## 素材说明

- 源视频：`D:\Picture\圆宝照片\圆宝抠图\圆宝360转头视频V2-改进顺时针旋转表现.mp4`
- 鸭子音效：来自 `dsh-whale-widget` 插件的 `Ya1.mp3` / `Ya2.mp3`（duck 音效组的 press / release）
- 圆宝叫声：原始 `圆宝叫声.mp3`（0.89s / 48kHz 立体声），归一化为 `web/sounds/meow.wav`（裁掉首尾静音，剩 0.65s）

## 源码与素材

**本仓库只放代码、文档与构建脚本，不含圆宝的影像与声音素材。**

`.gitignore` 排除了 `web/frames/`、`web/sounds/`、`assets/*.png|ico`、`extracted_frames/`、`archive/`、
`build/`、`dist/` 与两个 `target/`。克隆后自行准备素材才能构建出完整 exe；
`dist/` 里的成品通过 GitHub Releases 分发（exe 内嵌素材，属于产品交付物）。
