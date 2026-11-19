# 架构说明

> 当前版本 v1.0.3 之后的开发线。Phase B（音效行为引擎）已落地，Phase A（模块拆分）待执行。

## 1. 运行时数据流

```
多媒体定时器 (12ms)
        │ WM_TICK
        ▼
   on_tick()
   ├─ 物理：甩出/弹跳/落地 → 新位置
   ├─ 跟随：光标方位角 → 帧号（环形最短路径）/ idle 收势
   ├─ 音效：audio::tick()（延迟触发、循环续播、淡出）
   ├─ 按压曲线：Down / Bounce / Land → 缩放
   └─ 绘制：BGRA 预乘位图 → UpdateLayeredWindow
```

- 帧缓存在启动时全量预载（361 帧；按 disp() 缩放后约 50 MB）。
- 重绘按需：帧号、缩放、位置、透明度、镜像都没变就跳过。
- 点击命中用当前帧 alpha 判定，透明区穿透。

## 2. 音效行为引擎（native/src/audio/）

**模型**：音效组 = 一组行为；行为 = 触发（on）× 播放（play）× 条件（when）。

| 维度 | 取值 |
|---|---|
| `on.event` | `press` / `release` |
| `on.delayMs` | 触发延迟；按下后松手会取消 |
| `on.whileHeld` | 按下即开始，松手才停（不判断按多久） |
| `play.mode` | `once` / `loop` |
| `play.until` | `release` / `forever` |
| `play.fadeOutMs` | 停止时淡出 |
| `when` | `heldMsLt` / `heldMsGte` / `movedLt` |

同一组里可以并存多条 `press` 行为（例如 600ms 呼噜 + 1200ms 另一段），互不干扰。

**播放**：MCI（`waveaudio`）多实例池，按音频文件 round-robin。MCI 只吃路径，
所以资源首次使用时释放到 `%TEMP%\yuanbao-deskpet-sounds\<key>.wav`（长度不符即重写）。
循环靠"播完再补一次"维持（waveaudio 不支持 repeat）。

**素材缺失**：`build.rs` 只嵌入存在的 `web/sounds/<key>.wav`；运行时不存在的 key
不进入资源表，对应行为静默，**不阻断构建、不影响其它行为**。

## 3. 配置三层

| 层 | 文件 | 入库 | 内容 |
|---|---|---|---|
| 内置配置 | `assets/settings.json` | ✅ | 尺寸/不透明度档位与范围、几何、帧参数、音效默认值 |
| 内置配置 | `assets/sounds.json` | ✅ | 音效组与行为（纯配置，无音频） |
| 用户配置 | `%APPDATA%\YuanBaoDeskPet\config.json` | ❌ | 用户当前值，覆盖内置默认 |

代码里不持有这些数值的同义常量。改档位 = 改 JSON；改行为 = 改 JSON。

## 4. 菜单命令 ID

设置菜单构建时由 `settings_submenu()` **现场分配** ID 并登记到 `MenuCmd` 表；
`wndproc` 只查表（`handle_menu_cmd`）。新增菜单项不需要改任何分发范围，
从机制上消除了"新 ID 落在区间外被静默丢弃"的历史问题。

## 5. 模块拆分（Phase A，已完成）

```
native/src/
├── main.rs              入口 + 消息循环 + wndproc 分发（449 行）
├── app/{state,tick,cmd}.rs      状态与几何 / 每帧驱动 / 命令与持久化
├── render/{present,frames,press}.rs   分层窗口合成 / 资源与帧 / 按压曲线
├── input/{follow,hit}.rs        角度跟随 / alpha 命中
├── audio/{mod,mci}.rs           行为引擎 / MCI 封装
├── sys/{win,autostart}.rs       屏幕与窗口 / 注册表自启
├── ui/{menu,menu_ui,tray}.rs    菜单与命令表 / 自绘 / 托盘
├── settings.rs / config.rs / update.rs / physics.rs
```

拆分是纯搬运（脚本按顶层条目行区间切分，逻辑未改），`main.rs` 从 1718 行降到 449 行。
每个模块只依赖 `crate` 内的同级模块，没有反向依赖。
