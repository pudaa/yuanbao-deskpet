# 硬约束

改这个项目之前先读这一页。违反其中任一条都算回归。

## 1. 素材不入库

`web/frames/`、`web/sounds/`、`assets/*.png`、`assets/icon.ico`、`legacy/webview/src-tauri/icons/`、
`docs/figures/`、`extracted_frames*/`、`archive/`、`dist/`、`build/`、`**/target/` 全部在 `.gitignore` 里。

- 仓库只放代码、文档、配置与构建脚本。
- 缺失素材必须**静默降级**（构建给 warning，运行时该行为静默），绝不能阻断编译。
- 需要素材才能构建时，README 里已经写明"自备素材"。

## 2. 配置单一真源

- 档位、几何、帧参数 → `assets/settings.json`
- 音效组与行为 → `assets/sounds.json`
- 用户当前值 → `%APPDATA%\YuanBaoDeskPet\config.json`

**禁止**在 Rust 里写与上面同义的常量或数组字面量。需要"同时兼容两套参数"时，
在 JSON 里加一条，不要在代码里加分支或常量。

## 3. 菜单命令 ID

- 设置项 ID 由 `settings_submenu()` 现场分配并登记，`wndproc` 只查表。
- 不要恢复 `100..=160` 这类区间字面量；历史上它漏掉过新 ID，导致命令被静默丢弃。

## 3.5 音效行为 id

- `sounds.json` 里每条行为的 `id` 是**对外契约**（用户配置以 `组id.行为id` 引用它）。
- 改 id = 静默丢掉老用户的开关设置；只允许新增、不允许改名。
- 单测 `behavior_ids_unique_per_pack` 守着「组内唯一」。

## 4. 单文件分发

- 帧、音效、图标、版本信息全部是手写 PE 资源（`native/build.rs` 生成 `.res`）。
- 唯一例外：MCI 只接受文件路径，音频首次播放时释放到 `%TEMP%`。
- 不允许引入需要外部文件才能运行的依赖。

## 5. 自更新

- 版本号以 exe 自身的 VERSIONINFO 为准（`build.rs` 从 `CARGO_PKG_VERSION` 写入）。
- 发版 = 改 `native/Cargo.toml` 的 version → tag → 上传 exe 资产。
- 资产名用 ASCII（GitHub 会把中文文件名截断成 `-.exe`）。

## 6. 素材语义

- 帧 0–31 是"低头→抬头"预备动作，只作极点 idle，**不得**并入角度索引。
- 帧 32–360 是主角度环，映射公式见 README。
- 圆宝素材不可改名/改内容以外的用途；新增音效按 `<pack>_<slot>.wav` 命名。
