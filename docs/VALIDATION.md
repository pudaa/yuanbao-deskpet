# 圆宝桌宠 · 验证文档

> 素材：`圆宝360转头视频V2-改进顺时针旋转表现.mp4`（720×720 / 24fps / 15.07s / 361 帧 / 绿幕）
> 项目：`D:\Codes\yuanbao-deskpet`
> 技术栈：**原生版纯 Win32 + GDI（主力，无 WebView）**；Tauri 2 + WebView2（对照，见 4.14 起的迁移记录）
> 产物：`dist\圆宝桌宠-单文件.exe`，**7.3 MB 单文件**（当前 v1.0.3；Web 版 `dist\YuanBaoDeskPet.exe` 保留对照）
>
> **目录变更（2026-11-19）**：WebView 对照版已从 `src-tauri/` 移至 `legacy/webview/src-tauri/`（前端仍复用根目录 `web/`）。下文 4.x 记录中的 `src-tauri/` 均指旧位置，作为历史保留。

---

## 1. 交付物

| 文件 | 说明 |
|---|---|
| `contact_sheet.png` | 46 格标定接触表（每 8 帧），带帧号与表观 yaw/pitch |
| `extracted_frames/0000–0360.png` | 361 张 384×384 RGBA 透明帧（源资产，PNG 无损） |
| `web/frames/0000–0360.webp` | 同上，WebP q94，约 16MB，供运行时加载 |
| `assets/frames.json` | 8 方位锚点 `angleAnchors` + 逐帧 `angleKeys` |
| `src-tauri/` | Rust 主程序（窗口 / 托盘 / 全局光标 / 配置） |
| `web/` | 前端（桌宠画布 + 设置面板） |
| `extracted_frames_v1/`、`assets/frames.v1.json` | V1 素材产物，仅作对比留存 |
| `legacy-electron/` | 首版 Electron 实现，回退备查 |

---

## 2. 素材处理：视频没有真实 alpha

源是 `yuv420p` H.264，**不含 alpha**，用本地绿幕色度键抠图，不上云。

关键设计（针对"不要把白毛/灰毛误删"）：

1. **绿度用 `G - max(R,B)`**，而不是 `G - (R+B)/2`。白毛被绿幕反射后仍 R≈B 且偏高，`G - max(R,B)` 很小，不会被误删。
2. 高亮中性保护：`min(R,G,B) > 0.72` 且绿度 < 0.30 强制不透明（白胸毛、胡须）。
3. 内部洞填充：猫眼偏黄绿易被判成背景，用 `binary_fill_holes` 补回。
4. 去溢色：绿边像素把 G 压到 `max(R,B)`。
5. **颜色去污染**：半透明像素的 RGB 混了绿幕的暗色，直接去绿会在胡须和毛发边缘留下**黑边**。这里用距离变换把每个半透明像素的 RGB 替换为最近实心像素的颜色，边缘在浅色背景上不再发脏（对比图 `build/edge_cmp_160.png`，左旧右新差异明显）。
6. **统一 bbox**：361 帧共用同一外接框与缩放系数（并集 `114,122,670,686`），保证转圈时身体不抖、不缩放。

验证：品红底合成无绿边、无白边、胡须完整、胸口白毛无破损。

---

## 3. 方向标定：V2 是真正的 360°

### 3.1 为什么换素材

V1 素材（241 帧）看起来是"转一圈"，逐帧标定后发现它其实是**环顾轨迹**——yaw 只覆盖 −35°~+20°，pitch −20°~+20°，没有后脑勺。当时的实现把"轨迹顺序"当成了角度，于是出现你测到的问题：**鼠标在正上方，猫头却在朝下看**。根因不是平滑参数，而是**帧序 ≠ 朝向角**。

V2 素材是「身体固定、头部按 上→右上→右→右下→下→左下→左→左上→上 顺时针转一整圈」，帧序本身就是朝向角，问题从根上消失。

### 3.2 规格与抬头段裁剪

视频共 361 帧，但**前 32 帧不属于朝向角**：帧 0 是"低头看向前方、眼睛向上瞟、正要抬头"，帧 32 才抬到位看向正上。这段是**抬头动画**。

如果让它参与角度索引，鼠标划过头顶时会重播这段抬头——表现就是"头突然一沉又抬起来"，非常突兀。所以：

- **帧 0–31**：不参与角度索引，**仅用作极点 idle**（正是你指出的"低头正要抬头"姿态）
- **帧 32–360**：主序列，覆盖 360°，共 328 帧，约 **1.098°/帧**
- **帧 32 与帧 360 是闭环最优点**：实测与末帧的像素差 0.042，是全程最小（其后开始右转，差异回升）
- 身体全程不动，只有头部旋转

### 3.3 方位锚点（`assets/frames.json → angleAnchors`）

yaw/pitch 为目测表观角度，仅用于文档与调试，**不参与索引**。

| 方位角 | 帧 | 方位 | yaw | pitch |
|---:|---:|---|---:|---:|
| 0° | 32 | 上 | 0 | +40 |
| 45° | 73 | 右上 | +30 | +28 |
| 90° | 114 | 右 | +45 | 0 |
| 135° | 155 | 右下 | +30 | −28 |
| 180° | 196 | 下 | 0 | −40 |
| 225° | 237 | 左下 | −30 | −28 |
| 270° | 278 | 左 | −45 | 0 |
| 315° | 319 | 左上 | −30 | +28 |
| 360° | 360 | 上（闭环） | 0 | +40 |

### 3.4 索引模型

```
鼠标相对猫中心的方位角 θ = atan2(dy, dx)      // 屏幕坐标，y 向下
u   = (θ + π) / 2π                            // 0..1
deg = ((u × 360) − 90 + 360) mod 360          // 0..360，0 = 正上方
pos = 32 + (deg / 360) × 328                  // 只映射到帧 32..360
帧号 = pos（相邻帧交叉淡化）
```

闭环处理：`pos > 357` 时，用最后 3° 把**帧 360 交叉淡化到帧 32**，避免硬切。因为这两帧姿态几乎一致，过渡几乎不可见。

校验（正上方附近）：

```
方位角  -10°   -5°   -2°    0°   +2°   +5°  +10°
帧号   350.9 355.4 358.2  32.0  33.8  36.6  41.1
```

顺时针 2–5° 落在帧 34–37，**抬头动画区间 0–31 完全不再被命中**。

**角度极点 = 猫窗口中心**（`ORIGIN_SCREEN = false`）。这是语义正确的选择：鼠标在猫的正上方，猫就看正上方。

---

## 4. 验证

### 4.1 映射数学（`node scripts/test_angles_v2.js`，PASS）

```
方向    鼠标(dx,dy)      角度deg   帧号   期望   结果
正上    (   0,-120)       0.0       0      0    OK
右上    (  85, -85)      45.0      45     45    OK
正右    ( 120,   0)      90.0      90     90    OK
右下    (  85,  85)     135.0     135    135    OK
正下    (   0, 120)     180.0     180    180    OK
左下    ( -85,  85)     225.0     225    225    OK
正左    ( -120,   0)    270.0     270    270    OK
左上    ( -85, -85)     315.0     315    315    OK

绕一圈采样 720 次：最大跨帧 0.500（理论 0.501）
闭环接缝：帧 360 -> 0  OK
结论：PASS
```

### 4.2 八方向真机（`python scripts/probe_directions.py`）

把猫临时移到屏幕中央，依次把鼠标放到 8 个方位（距中心 150px）截图。结果 `build/dirs_grid.png`：
**8 格全部与标签一致**——`up` 抬头看天、`right` 侧脸朝右、`down` 低头看地、`downleft` 低头看左下，无一错位。

### 4.3 左上 → 右上横扫（曾出现跳变，已修）

鼠标从猫左上沿水平线扫到右上（经过正上方），采样 24 帧（`build/sweep2_grid.png`）。

这一步暴露过两个不同的问题，都已修：

1. **抬头动画重播**（本轮修复）。正上方在旧映射里对应帧 0，而帧 0 是"低头正要抬头"，于是划过头顶会重播抬头。裁剪后正上方对应帧 32（已抬头到位），第 9–14 帧头部高度完全稳定，不再一沉一抬。
2. **边界抖动**（上一轮修复）。旧实现用单阈值 88px 判断"靠近极点"，鼠标在边界附近微动就反复翻转。现在改为 28px 进入 / 1.7 倍退出 的滞回。

结果：24 帧单向连续，中段无高度突变。

### 4.4 胡须黑边（已修）

半透明像素的 RGB 混有绿幕暗色，直接去绿会在胡须/毛发外缘留下黑边。加入颜色去污染后，160px 实显示尺寸下胡须为干净浅色细线（对比 `build/edge_cmp_160.png`）。

### 4.5 转圈连续性

绕猫顺时针采样 24 帧（`build/spin_grid.png`）：0 正上 → 6 右上 → 12 右下 → 15 正下 → 20 左侧 → 23 回左上，**单向连续，无跳变、无倒退、无卡顿**。

### 4.6 跟手参数

| 项 | 旧 | 现 |
|---|---|---|
| 光标采样 | 33ms（30Hz） | **16ms（60Hz）** |
| 平滑时间常数（常态） | 0.085s | **0.055s** |
| 平滑时间常数（大跨度） | 0.045s | **0.028s** |
| 主观延迟 | 明显滞后 | 约 30–50ms |

仍可在设置面板用「跟手速度」0.5–2.0× 继续调。调参逻辑：`tau = base / smoothing`，`smoothing` 越大越跟手。

### 4.7 中心保持：帧序列回归动画

鼠标距猫窗口中心 < `idleRadius`（默认 **28px**，退出阈值 **1.7 倍**做滞回）时，播放一段**帧序列回归动画**，而不是像素叠化。

**状态机**（`renderer.js`）：

| 模式 | 行为 |
|---|---|
| `FOLLOW` | 正常跟随鼠标 |
| `TO_IDLE` | 从当前帧沿环形最短路径转到帧 32（看上），再反向播到帧 0（低头） |
| `IDLE` | 停在帧 0 |
| `TO_FOLLOW` | 从帧 0 抬头到帧 32，然后交回 `FOLLOW` |

路径长度随起点变化，时长按长度在 0.25–0.9s 间自适应；全程**逐帧绘制**，不做 alpha 叠加，所以没有重影。

**为什么放弃叠化**

前面试过两版像素叠化，都不行：

1. `requestAnimationFrame` 驱动 + 单 canvas 叠加：WebView 节流时过渡卡在半透明中间态，出现两只半透明猫头。
2. 独立 canvas + CSS `opacity` transition：仍然可能停在半路。

**真正的根因其实不在节流**：`main.rs` 里 `hold` 的判断排在 `CENTER_DEAD` 之后，而 `CENTER_DEAD(20) < idle_radius(28)`。鼠标正好停在窗口中心时，代码走 `continue`，**一条事件都不发**——前端永远收不到 `hold=true`，画面就冻在最后一帧。表现出来像"节流/卡死"，实际是事件断流。

修复：`hold` 判断前置，命中时**继续发送事件**（`u` 填占位值），前端靠这条持续的事件流推进动画。同时 `main.rs` 去掉了光标事件的去重，保证停住不动时也有稳定的事件频率驱动。

**为什么不再依赖 rAF**：`step()` 由 rAF 和 IPC 事件共同调用，共享 `lastT`，`dt` 基于真实时间，不会重复推进。即使 rAF 被节流，事件流也能把动画走完。

### 4.8 设置面板：四个连环 bug

这一节记录的问题都是"设置页打不开/白屏"，但根因各不相同，前三个互相掩盖。

**① WebView2 环境参数冲突**（整页白屏）

```
CreateWebview(WebView2Error(HRESULT 0x8007139F, "组或资源的状态不是执行请求操作的正确状态"))
```

为排查"动画卡住"给主窗口单独加过 `additionalBrowserArgs`。WebView2 的浏览器参数在同一进程内必须全局一致，运行时新建的窗口用默认参数，环境对不上，第二个 WebView 直接建不起来。真因（事件断流）修好后该参数已无必要，删除。

**② 从 IPC 命令线程建窗口**（窗口在、内容空白）

`#[tauri::command] fn open_settings_cmd()` 里直接 `WebviewWindowBuilder::build()`，得到的窗口有标题栏但**内容全白**，且 `center()` 也失效（落在 Windows 层叠默认位 128,128 / 224,224）。

先改 `app.run_on_main_thread()` 包装——**依然白屏**。说明问题不只是线程，而是"运行期新建 WebView"这条路径本身不可靠。

**③ 已存在的坏窗口被反复复用**

`open_settings` 开头 `if let Some(w) = get_webview_window("settings") { show(); return; }`。
一旦有一次建窗失败留下空壳，后续每次点击都只是把那个白窗口 `show` 出来，看起来像"每次都没修好"。诊断文件的时间戳没更新才暴露这一点。

**④ 最终方案：启动时预创建**

在 `setup`（主线程、已验证可靠）里以 `visible(false)` 预创建设置窗口并隐藏；点击"设置"只做 `center() + show() + set_focus()`。关闭时 `CloseRequested → prevent_close + hide`，保证窗口永不销毁、始终可复用。

**附带修复：右键拖动冲突**

原来 `#stage` 用 `data-tauri-drag-region`，**右键也会触发拖动**——实测右键后窗口从 (2315,1195) 被拖到 (2307,1177)，菜单时有时无。改为 JS 监听 `mousedown` 且只对 `button === 0` 调 `startDragging()`。

**附带修复：托盘勾选不更新**

`refresh_tray` 函数根本不存在，托盘菜单只在启动时构建一次。抽出 `make_menu()`，新增 `refresh_tray(app)`（`tray_by_id("main")` + `set_menu`），在 `apply_lock` / `toggle_visible` / `apply_size` 三处调用。

**验证**：右键 → 设置，窗口居中于 (958,220)、643×1007、面板完整渲染、页脚 `361 帧 · 视口 500×768 · DPR 1.25`；右键不再移动窗口；锁定后 `config.locked=true` 且进程正常。

### 4.9 右键菜单

右键圆宝弹出**自定义菜单**（设置… / 锁定（穿透）/ 回到右下角 / 退出圆宝），并屏蔽 WebView 自带的浏览器菜单（返回/刷新/打印那一套）。

锁定时窗口不接收鼠标事件，右键会穿透到下层应用，这是预期行为——要解锁请点托盘图标。

### 4.10 甩出物理

参数直接对齐 `dsh-pet` 的 `shared-core.js`，保证手感一致：

| 参数 | 值 | 含义 |
|---|---:|---|
| gravity | 1400 px/s² | 重力 |
| restitution | 0.78 | 碰撞弹性 |
| groundFriction | 2.5 | 地面摩擦系数 |
| maxThrowSpeed | 3600 px/s | 甩出速度上限 |
| deadZone | 500 px/s | 低于此速度不算"甩"（轻拖不放飞） |
| restVy / restVx | 40 / 15 | 静止判定阈值 |

**甩出速度估算**（`physics::estimate_throw`）：取最近 150ms 的光标轨迹，`base = 首尾位移/时间`，再与分段峰值速度按 5:5 加权，上限 3600。低于 500 视为轻拖，不触发。

**驱动方式**：物理在 Rust 的轮询线程里跑（16 ms 一步），直接 `set_position` 移动窗口。不用 JS 做物理，避免 WebView 节流。

**点击 vs 甩出的区分**：左键按下时记录窗口位置，松开时若窗口位移 > 3 px 视为拖动/甩出，否则视为单击（触发抚摸）。这样单击和拖动共用同一个 mousedown，不需要额外手势。

**落地挤压**：`step()` 返回 `landed` + `impact`，前端收到 `pet:landed` 后播 220 ms 的 `scale(1.16, 0.84) → 1` 动画，impact < 280 时忽略（轻放不弹）。

**中途被抓住**：滑行过程中按下左键，主进程**立刻把 `body` 置空**，物理这一步就彻底退场。否则物理每 16 ms `set_position` 一次、系统拖拽又往鼠标位置拉，两个源抢同一个窗口坐标，表现为猫在原地高频抖动。停掉物理之后窗口位置只由拖拽一个源决定；松开时若仍有位移且速度够，会重新 `estimate_throw` 再甩一次。

### 4.11 抚摸反馈

**手势时序**（三段，与两个音效一一对应）：

| 阶段 | 视觉 | 音效 |
|---|---|---|
| 按下 | 70ms 内压到 `scale(1.17, 0.76)`，底部为轴，**保持住** | Ya1（`pat_down`） |
| 按住拖动 | **一直保持压扁** | — |
| 松开 | 340ms 回弹（过一次 `scale(0.96, 1.06)` 再复原） | Ya2（`pat_up`） |

第一版把压扁做成一整段动画在松手时才播，手感是"松开才弹一下"，不对。现在用 `body.pressing`（`animation: squeeze forwards` 保持终态）+ `body.bounce`（松开时的回弹 keyframes），两者衔接在压扁态，不会跳。

**命中判定**（两级，缺一不可）：

1. **前端**：按下瞬间 `getImageData` 读该点 alpha，> 24 才算命中；窗口是 188px 方形，猫只占中间一块，点四角不能算"摸到"。镜像模式下先把 x 翻成 `winSize - x`。取样异常时回退到 `petSize` 几何方块（**绝不退化成"永远命中"**——第一版 catch 分支返回 255 就是这个后果）。
2. **主进程**：松开时要求"按下与松开**都在窗口矩形内**、且窗口位移 ≤ 3px"才算抚摸（`is_pat`）。否则拖动结束后在桌面空白处松手，也会被误判成一次点击。

松手时**无论点击还是拖动都会发 `pet:release`**，前端据此解除压扁并回弹——拖动结束后不会"啪"地瞬间复原。

### 4.12 性能：内存从 453 MB 降到 181 MB

老大截图反馈任务管理器里 WebView2 那一组吃了 306 MB，明显过高。实测（Private 口径，与任务管理器一致）：

| 进程 | 优化前 | 优化后 |
|---|---:|---:|
| GPU 进程 | 282.6 MB | **16.6 MB** |
| browser（管理器） | 55.4 MB | 54.7 MB |
| renderer ×2（pet + 设置） | 64.1 MB | 65.3 MB |
| utility ×3 | 32.4 MB | 32.7 MB |
| crashpad | 4.3 MB | 3.5 MB |
| 主进程 | 14.8 MB | 7.7 MB |
| **合计** | **453.6 MB** | **180.6 MB** |

两处改动：

**① 帧分辨率 384 → 224**

361 帧 × 384×384 RGBA = **203 MB** 解码后常驻内存。但猫只显示 160 CSS px（125% DPI ≈ 200 物理像素），384 有 1.9 倍冗余。降到 224 后是 69 MB，省 134 MB；WebP 从 16 MB 降到 6.3 MB，exe 从 18 MB 降到 9.8 MB。

**② 关闭 GPU 加速**

真正的大头是 GPU 进程 282 MB——一个 185×185 的透明窗口不该吃这么多。透明窗口的离屏合成 + 361 张纹理上传，让 WebView2 的 GPU 进程异常膨胀。桌宠本质只画一张静态帧，软件光栅化完全够用。

```rust
std::env::set_var(
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "--disable-gpu --disable-gpu-compositing",
);
```

**必须用环境变量，不能用 `tauri.conf.json` 的 `additionalBrowserArgs`**：后者只作用于配置里的那个窗口，运行时创建/预创建的 settings 窗口参数不一致会直接建不起来（就是之前"设置页白屏"的根因）。环境变量是全局的，所有 WebView2 实例一致。

**验证**：透明背景正常（窗口角落取样是桌面颜色，非黑底），毛发无锯齿，胡须干净。

**③ 帧按需加载（环形窗口 + 远端淘汰）**

361 帧不必全量常驻。转头只需要当前角度附近的一小段，所以：

- 只缓存当前帧周围（向前 18°、向后 10°）
- 缓存上限 88 帧，超出时按**环形距离**淘汰最远的，并把 `img.src = ''` 断引用让 Blink 回收解码缓冲
- 目标帧尚未解码完时用上一帧兜底，避免快速甩鼠标闪白

实测：不转圈时 renderer 27.7 MB，转一圈缓存打满后 34.4 MB（全量版本是 41.8 MB）。**收益比预期小**——因为 Chrome 本身就不常驻全部解码图，只缓存显示过的帧，所以"全量常驻 69 MB"的估算并不成立。真正省下的是 7–14 MB。

> 踩坑：`step()` 里原有 `if (!ready) return`，而 `ready` 要靠帧加载成功才置真。改成按需加载后两者互相等待，启动直接白屏（只剩"圆宝加载中…"）。修法是初始化时主动 `refill(MAIN_START)` 拉一次。

**未采用的方案**：把设置窗口改成懒创建能再省 ~24 MB，但运行期创建 WebView 已验证会出现白屏，稳定性优先，保留预创建。若要继续下探，只能放弃 WebView 改原生绘制（可压到 40 MB 内，但设置面板需重写）。

### 4.13 旋转基准移到鼻子

原来算方位角用**窗口几何中心**（落在猫胸口），导致抬头时角度整体偏下。实测猫头部中心在画面 30% 处、鼻子约 38%，换算到含 14px 透明边的窗口：

```
(14 + 160×0.38) / (160 + 28) ≈ 0.395
```

所以改成 `FOCUS_Y = 0.395`，`cy = pos.y + size.height × 0.395`。这个比例在不同尺寸下基本恒定（petSize 340 时是 0.389）。

### 4.14 原生绘制路线（`native/`，实验）

按"双路并行"的要求新开目录，不动 `src-tauri/` 与 `web/`。目标是绕开 WebView2 那 110 MB 固定开销。

**技术方案**：不用 WebView，纯 Win32：

```
WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE
+ UpdateLayeredWindow(ULW_ALPHA)
```

帧在启动时缩到 160×160，转成**预乘 alpha 的 BGRA**（UpdateLayeredWindow 的硬要求），贴在 184×184 的透明画布中心，16 ms 定时器按鼠标方位角换帧。

**已跑通**：窗口创建、透明背景、帧渲染、四方向跟随全部正常。

| 指标 | WebView 版 | 原生版 |
|---|---:|---:|
| exe 体积 | 9.8 MB | **315 KB** |
| 内存 | ~180 MB | **62.7 MB** |
| 进程数 | 9 | **1** |

62.7 MB 里约 48 MB 是 361 帧的预乘位图（184×184×4），后续做按需加载可以降到 20 MB 左右。

**待补**：按需加载、拖动/甩出物理（可以直接搬 `src-tauri/src/physics.rs`）、托盘（`Shell_NotifyIcon`）、右键菜单、设置面板、抚摸音效。

**设置面板是原生路线唯一的结构性障碍**：要么用 egui 重画（风格难统一），要么在打开设置时临时起一个 WebView（用完销毁）。后者更省事，也不影响平时内存。

**踩坑**：`windows` 0.58 的 GDI/User32 函数普遍用 `Param<T>` 泛型，参数**直接传值**，不能包 `Some(...)`；`CreateWindowExW` 的 hinstance 也是 `HINSTANCE` 而非 `Option`。

### 4.15 设置面板：临时 WebView，关闭即归零

原生版没有 WebView，但设置面板必须用 HTML（视觉风格已定型，重画成本高）。方案是**打开时临时挂一个 WebView2，关闭时连进程一起销毁**。

实现（`native/src/settings.rs`）：`wry::WebViewBuilder::build_as_child` 挂到普通 Win32 窗口上，`WM_CLOSE` 时先 `webview.take()` 释放控制器，再 `DestroyWindow`。

**实测（核心验收点）**：

| 状态 | WebView2 进程 | 内存 |
|---|---:|---:|
| 打开设置 | 6 个 | 174.2 MB |
| **关闭设置** | **0 个** | **0 MB** |
| 桌宠本体常驻 | 0 | 54.9 MB |

关闭后 WebView2 进程全部退出，内存完全归零，符合"关闭即释放"的要求。

**踩坑**：`build_as_child` 默认把子 WebView 建成 **250×250**，不会自动铺满父窗口客户区。必须 `GetClientRect` 后通过 `.with_bounds()` 显式指定，否则窗口打开但内容只占左上角一小块（看起来像渲染失败）。

**待补**：设置页 JS 目前还是 Tauri 的 `invoke`，在纯 WebView2 里拿不到 `__TAURI__`，所以控件是静态的。需要改成 `chrome.webview.postMessage` + `addEventListener('message')` 双向通信。

### 4.16 原生版：拖动、甩出、托盘

**拖动改成自实现**（不再交给 `HTCAPTION`）：`WM_LBUTTONDOWN` 记录抓取偏移并 `SetCapture`，`WM_MOUSEMOVE` 直接 `SetWindowPos` 跟手，同时把光标位置压进最近 24 个采样点的环形缓冲。这样才能在松手时算出甩出速度——交给系统拖动的话拿不到轨迹。

**甩出**：`WM_LBUTTONUP` 时 `physics::estimate_throw(&trail)`，有速度就 `body = Some(...)`，之后每 16 ms 在 `on_tick` 里步进并 `SetWindowPos`。物理模块直接从 `src-tauri/src/physics.rs` 复制——纯计算、零依赖。

实测轨迹（从右下角往左上甩）：

```
起点 (2332,1152) → 松手 → (1881,878) → (1605,737) → (1343,626)
→ ... → (58,395) 撞左边界 → (78,406) 反弹 → (284,441)
```

**抓住滑行中的猫**：`WM_LBUTTONDOWN` 里直接把 `body` 置空，物理立即退场，避免和拖拽抢窗口位置（跟 HTML 版同一处坑）。

**托盘**：`Shell_NotifyIconW(NIM_ADD)` + `CreatePopupMenu` / `TrackPopupMenu`，菜单项：设置… / 显示-隐藏圆宝 / 退出。`WM_DESTROY` 时 `NIM_DELETE`。

日志确认 `tray_add=true`。图标被 Windows 默认折叠进隐藏区（新图标的系统行为），点任务栏的 `^` 或拖出来固定即可。

### 4.17 原生版：弹弓修复 + 按压反馈

**弹弓效应**（拖到新位置停住、松手却飞出去）

根因：轨迹只在 `WM_MOUSEMOVE` 里采样。手停住之后不再产生 mousemove，轨迹尾部还停在"快速拖动"那一段，而 `estimate_throw` 取最近 150 ms 窗口、还做峰值加权 —— 于是算出来仍是高速。

修法：把采样搬到 `on_tick`，**按 16 ms 定时记录**，不看是否在移动。停住时持续压入相同坐标，窗口内的位移自然归零。

实测：拖动中停 0.5 s 再松手，位置变化 **(0, 0)**；快速甩仍正常飞出（`2092,1002 → 2215,763 → 1956,653 → ... → 1052,538`）。

**按压反馈**（压扁 + 音效）

- 按下播 `pat_down.wav`，松开若是点击（位移 ≤ 3px）再播 `pat_up.wav`。用 `PlaySoundW`（只吃 wav，mp3 已转码）。
- 压扁用 `squeeze ∈ [0,1]` 驱动：按下时 60 ms 内趋近 1，松手用 `ease_out_back` 跑 340 ms 回弹（会过冲到负值，即轻微拉伸）。
- 命中判定读当前帧的 alpha（`> 24`），点窗口角落的透明区不算摸到。

**关键实现陷阱**：压扁**不能**通过改 `UpdateLayeredWindow` 的 `pptDst` / `size` 来做。`pptDst` 本身就是窗口位置，改它等于每帧把窗口往下挪，实测累积出 47 px 漂移。正确做法是在 CPU 侧把内容缩放到 DIB（底部对齐），窗口位置保持不变。

> 顺带踩了个崩溃：横向撑开后偏移 `ox` 为负，原写法直接按 `(y+oy)*WIN + (x+ox)` 索引，越界写缓冲区导致进程 abort（表现为窗口突然消失、`GetWindowRect` 返回 0,0）。必须逐点判断 `dx/dy` 是否落在 `0..WIN` 内。

### 4.18 设置面板 IPC 崩溃：根因与修复

**现象**：挂上 `with_ipc_handler` 后，打开设置面板，主进程无声退出（桌宠消失、`IsWindow` 返回 false），连日志都没有。

**根因**：`wry` 的 IPC 回调**不在主 UI 线程**。原来在 handler 里直接调 `apply_cmd`，而 `apply_cmd` 要拿 `ST` 的 `Mutex` 并操作窗口 —— 与主线程 `WM_TIMER` 里的 `on_tick` 竞争，触发跨线程的窗口/状态操作，进程直接没了。

`panic = "abort"` 让它连 backtrace 都不留，所以前几轮只能靠排除法猜。

**修复**：改成**队列式**。

```rust
// handler 只做一次 Vec push，不碰任何状态
.with_ipc_handler(|req| crate::ipc_push(req.body().to_string()))

// 主线程在 on_tick 开头消费（此时还没拿 ST 的锁）
for msg in ipc_drain() { apply_cmd(...) }
```

顺带把 `panic = "abort"` 去掉并挂了 `set_hook` 写临时文件，以后再崩能留下证据。

**验证**：打开设置 → 把不透明度从 100% 拖到 76%，猫同步变淡；主窗口存活、无 panic 日志。**写入链路已通**。

> 另外发现 `main()` 里的 `--disable-gpu` 曾经在重构中丢失过（内存会回到 280MB 那档），已补回。

### 4.19 同类项目调研（GitHub）

搜到几个**技术路径和本项目几乎一致**（Rust + Win32 分层窗口 + 逐像素 alpha）的项目：

| 项目 | 技术栈 | 值得借鉴的点 |
|---|---|---|
| **SemaPet**（midea-ai） | Rust + `windows-sys`，分层窗口 | **Render/Hit 双窗口分离**；alpha-mask hit region；状态机 |
| **wunder**（ylsdamxssjxxdd） | Rust + Slint，独立窗口线程 | **有界、引用感知的帧缓存**；GDI 文本生成气泡遮罩；本地拖拽 + 系统右键菜单 |
| **workbuddy-whale-musume** | Rust + Win32 GDI，`UpdateLayeredWindow` | 与本项目**路径完全一致**；素材运行时读文件；呼吸/摇摆/微动作状态机 |
| **OHM Pet**（DrayChou） | Rust，macOS AppKit + Win 分层窗 | **不跑 60 FPS 常驻渲染**，按需重绘；加载 Shimeji/Ukagaka 素材 |
| **Mintu** | Tauri 2 + 30 Hz Win32 光标轮询 | 空闲 CPU < 2%；DOM hit-rect 同步 |
| **Ditto** | Tauri v2 | 重力/边界/抓取物理的组织方式 |

**可以落地的三点**：

1. **按需重绘**（OHM Pet 的思路）。我们现在 16 ms 定时器无条件 `present`，即使画面没变。改成"帧号 / 挤压值 / 窗口位置三者都没变才跳过"，静止时 CPU 应能明显下来。
2. **透明区真正穿透**（SemaPet 的 RenderWindow + HitWindow）。现在窗口的 14 px 透明 padding 仍会吃掉点击（只是不响应）。用独立 hit 窗口 + `CreateRectRgn` 组合出 alpha 轮廓，鼠标才能点在"猫旁边的空气"上落到下层窗口。
3. **帧缓存有界**（wunder 的引用感知缓存）。对应老大提过的"361 帧不必全量常驻"，我们 HTML 版做过环形窗口缓存，原生版还是全量 48 MB，可以搬同一套逻辑。

**暂不采纳**：SemaPet 用 HTTP 做 IPC、Ditto/Mintu 的 AI 行为层，都与当前范围无关。

### 4.20 三项性能优化（据调研落地）

**① 按需重绘**

`on_tick` 原本每 16 ms 无条件 `present`（memcpy 34 KB + `UpdateLayeredWindow`）。现在比对"帧号 / 挤压值 / 窗口位置 / 透明度 / 镜像"五项快照，全都没变就跳过。

**静止 CPU：从持续占用降到 0.000s / 6s（实测 0%）。** 桌宠 95% 的时间都是静止的，这是最大收益。

**② 透明区真正穿透**

用 `SetWindowRgn` 把窗口的可点击区域裁成当前帧的 alpha 轮廓，猫身边那圈透明 padding 不再"吃掉"点击。

实现要点：逐行扫描 alpha，把连续像素合并成矩形，用 **`ExtCreateRegion` 一次性提交**（比逐行 `CreateRectRgn` + `CombineRgn` 快一个数量级）。

验证：`GetWindowRgn` 返回 3（`COMPLEXREGION`），说明轮廓生效；渲染不受影响，毛发边缘依旧平滑。

**③ 帧缓存有界**

`frames: Vec<Frame>`（361 帧常驻 48 MB）改成 `Vec<Option<Frame>>`：只在需要时解码，环绕窗口 ±60 帧之外的槽位直接置 `None` 释放。初始只预载 25 帧。

目标帧未就绪时用 `last_frame` 兜底，避免白帧。

| 指标 | 优化前 | 优化后 |
|---|---:|---:|
| 内存（Private） | 54.9 MB | **5.9 MB** |
| 静止 CPU | 持续占用 | **0%** |
| 转动 CPU（单核） | — | 23% |
| 转动时白帧 | — | 0 / 20 帧 |
| 对比｜HTML 版 | ~180 MB | — |

**转动 CPU 偏高**（23% 单核）：每帧都要解码 webp + 重建 alpha 区域。缩小算法已从 Lanczos 换成 Triangle（差异不明显），瓶颈更可能在 `ExtCreateRegion`。进一步优化可以先做区域降采样（每 2 行合并）或隔帧更新轮廓，留待后续。

### 4.21 CPU 优化、set_size、镜像方向

**① 转动 CPU 23% → 0%（用内存换）**

23% 的根子是**每帧都在做一次 webp 解码 + 缩放**——按需加载把一次性成本摊到了每一次转头。

改回**启动时全量预载**（解码/缩放只在启动做一次）：

| 指标 | 优化前 | 优化后 |
|---|---:|---:|
| 转动 CPU（单核） | 23% | **< 0.3%**（长时间测试为 0.0%） |
| 静止 CPU | 0% | 0% |
| 内存 | 5.9 MB | 53.8 MB |

按老大"适当提高内存可以接受"的判断，这笔交换很值——53.8 MB 相比 HTML 版的 180 MB 仍然极低。

**② set_size 真正生效**

原本 `DISPLAY` / `WIN` 是编译期常量，改成运行时变量：

```rust
static DISPLAY_I: AtomicI32 = AtomicI32::new(160);
fn disp() -> i32 { DISPLAY_I.load(Ordering::Relaxed) }
fn win() -> i32 { disp() + PAD * 2 }
```

改尺寸时：清空帧缓存 → 按新尺寸重载 361 帧 → 重建 DIB → 重建 alpha region → `SetWindowPos` 改窗口大小（保持右下角锚点）。

实测 160 → 260：窗口 188 → 288，猫清晰无拉伸，进程存活。

**③ 镜像后朝向反转（HTML 版同样存在）**

画面用 `scaleX(-1)` 翻转，但角度还在用未翻转的坐标算，于是猫朝着反方向看。

修法：镜像开启时角度取反。

```js
// HTML 版 renderer.js
const u = document.body.classList.contains('mirror') ? (1 - cur + 1) % 1 : cur
```

```rust
// 原生版
if s.mirror { deg = (360.0 - deg) % 360.0; }
```

两版已同步修复。

### 4.22 回退窗口裁剪 + 修复悬停光标

**① 去掉 `SetWindowRgn` 裁剪**

上一轮为了"透明区真正穿透"给窗口设了 alpha 轮廓区域。但**按压压扁时猫会横向撑开**（scale_x 1.17），超出旧轮廓的部分直接被切掉，显示不全。

已回退：不再调用 `update_hit_region`。代价是窗口四周 14 px 的透明 padding 仍旧会"吃掉"点击（不响应，但下层窗口收不到）。这个取舍是值得的——显示完整性优先于 14 px 的穿透体验。

透明区的**抚摸判定**不受影响，`hit_test` 仍然读 alpha（阈值 24），点透明处不会触发压扁。

**② 悬停时显示"加载中"光标**

绕了很久的一个细节：鼠标放到猫身上，光标一直是箭头带沙漏（`IDC_APPSTARTING`），像在等什么。

根因：注册窗口类时 `WNDCLASSW { ..Default::default() }`，**`hCursor` 是空的**。系统拿不到光标就回退到一个默认值——正好是 AppStarting。

```rust
hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
```

验证：`GetCursorInfo` 读到的句柄 `65539` == `LoadCursor(IDC_ARROW)`，不再是 `65561`（AppStarting）。

### 4.23 极点回归走环形最短路径

**现象**：鼠标移到旋转中心时，猫头会**逆时针扫一整圈**才回到初始姿态。比如正在看左上，路径是"左上 → 左 → 左下 → 下 → 右下 → 右 → 右上 → 上 → 低头"，非常漫长。

**根因**：回归逻辑写成了无条件递减。

```rust
if s.cur > 0 { s.cur = s.cur.saturating_sub(4); }   // 一路减到 0
```

帧号 32 是"上看"、顺时针递增到 360，所以从左上（≈319）递减到 32 要走 287 帧——**绕了远路**。

**修复**：走环形最短方向。

```
dist_cw  = 360 - cur      // 顺时针：cur -> 360(≡32)
dist_ccw = cur - 32       // 逆时针：cur -> 32
```

取较短者。从左上顺时针只有 41 帧（≈0.16s），逆时针 287 帧。到达 32（上看）后，再走 32→0 的低头段。

实测（鼠标从左上移到中心，60ms 间隔采样）：**左上 → 略转正 → 正面 → 正面略低 → 稳定**，240ms 内收敛，没有任何绕圈。

### 4.24 甩出流畅度与音效延迟

**① 甩出时掉帧**

两个原因叠加：

1. **每帧两次系统调用**：物理步进先 `SetWindowPos` 移动窗口，`on_tick` 末尾又 `UpdateLayeredWindow` 重绘。
2. **`SetTimer` 受系统 15.6 ms tick 量化**：实测相邻位置变化间隔是 **11/21 ms 交替**，视觉上就是一顿一顿。

修法：

- 物理只算位置，**不再单独 SetWindowPos**；位置通过 `UpdateLayeredWindow` 的 `pptDst` 一并提交，一次调用完成"移动 + 绘制"。
- `SetTimer` 换成 **`timeSetEvent`**（多媒体定时器，1 ms 精度），回调里 `PostMessage(WM_TICK)` 交给 UI 线程；配合 `timeBeginPeriod(1)`，间隔 12 ms。
- 定时间隔 16 ms → **12 ms**（约 83 fps）。

实测相邻位置变化间隔：**平均 12.0 ms，中位 13 ms，最大 19 ms**（优化前平均 16 ms 且 11/21 交替）。

**② 快速点击时音效"吞掉"**

根因：每次播放都用 `SND_FILENAME`，要重新打开并读取 wav；加上 `PlaySound` 是单通道、新播放打断旧的，快速连点就只剩最后一声。

修法：启动时把两个 wav 读进内存，播放改用 **`SND_MEMORY`**，省掉文件 IO 与调度延迟。

> 注：`PlaySound` 本质仍是单通道，连续极快的点击依然会互相打断。若要真正的"吧唧吧唧"叠加，需要换 MCI（多 alias 轮转）或 XAudio2 —— 留待确认当前效果是否够用。

### 4.25 飞行中继续跟随 + MCI 多实例音效

**① 甩出飞行途中不跟随鼠标**

朝向更新被 `s.body.is_none()` 挡住了——物理飞行时干脆不更新角度，猫头固定在脱手瞬间的方向。Web 版是无论飞不飞都跟。

改成飞行中强制 `in_idle = false` 并照常 `update_frame`：

```rust
if !s.dragging && s.body.is_some() {
    s.in_idle = false;
    update_frame(&mut s);
}
```

验证：飞行途中把鼠标从右移到左，猫头随之从右转到左。

**② 快速点击仍然没声音**

`SND_MEMORY` 确实在播（日志 `len=20846 ok=true`），问题是 **`PlaySound` 全局单通道**：down 音约 0.5 s，连点间隔小于这个长度时，每次都会被下一次打断，听起来就是"没声音，最后响一下"。

换成 **MCI 多实例轮转**：启动时开 4 路 down + 2 路 up 的 `waveaudio` alias，每次播放 `seek ... to start` + `play ...` 轮换到下一个实例，互不打断，才能真正叠出"吧唧吧唧"。

> `windows` 0.58 里 `mciSendStringW` 是 3 参数 `(command, return_buf, hwnd)`，没有单独的 cc hReturn —— 返回值长度由切片长度推断。

### 4.26 设置不实时生效：IPC 前端入口用错

**现象**：设置面板里改尺寸、不透明度等，猫完全没反应。

**根因（两个叠加）**：

1. `settings.rs` 里的 `.with_ipc_handler(...)` 在之前修崩溃时被删掉，后来改队列式只补了**消费端**（`ipc_drain`），没补**接收端** → 消息根本没进队列。
2. 兼容层 shim 里用的是 `window.chrome.webview.postMessage`，但 **wry 的前端 IPC 入口是 `window.ipc.postMessage`**。wry 0.47 在 Windows 侧注入的是：

```js
Object.defineProperty(window, 'ipc', {
  value: Object.freeze({
    postMessage: s => window.chrome.webview.postMessage(s)
  })
});
```

所以之前的 postMessage 全部石沉大海，日志里一条都没有。

**修复**：接收端加回；shim 改优先 `window.ipc.postMessage`，保留 WebView2 原生入口作兜底。

**验证方式**：临时在 shim 末尾注入一条自检——打开设置 1.5 秒后自动 `set_size(240)`。若猫实时变大，说明「设置页 → wry → 队列 → apply_cmd」整条链路已通。验收后移除这行。

### 4.27 放弃 WebView 设置面板，改纯原生菜单

**决策**：设置页拖滑块会让进程闪退。根子在于链路太长：WebView2 临时进程 → 跨线程 IPC → 队列 → 主线程 `apply_cmd`，而 `set_size` 还要在主线程持锁重载 361 帧。老大提议"不用 WebView 的话信号通讯会不会更好做"，判断正确。

**改造**：

- 移除 `wry` / `raw-window-handle` 依赖、IPC 队列、shim、`settings.rs` 引用。exe 从 744 KB 降到 **453 KB**。
- 设置改为**原生弹出子菜单**（`CreatePopupMenu` + `AppendMenuW(MF_POPUP)`），右键菜单和托盘菜单共用：
  - 尺寸：160 / 220 / 280 / 340（当前值打勾）
  - 不透明度：100% / 80% / 60%（当前值打勾）
  - 水平镜像（开关，带勾选）
  - 恢复默认
- 点一下直接在**同一个 UI 线程**调 `apply_cmd`，零 IPC、零跨线程、零临时 WebView2 进程。
- 主题按老大要求只保留默认，不再提供主题切换。

**收益**：设置操作不再有闪退路径；平时内存也不再因开设置而临时涨 174 MB。

> `native/src/settings.rs` 保留作历史参考，但已不在编译图中。

### 4.28 叫停 egui，回落纯菜单并做自绘美化

**egui 方案实测否决**

按「视觉优先」试了 `eframe 0.31 + glow + Deng.ttf`：面板确实能复刻日系清新，中文也正常，但老大观察到**内存高达 200 MB**——字体 16 MB、glow/winit 渲染栈、字体图集，叠在 53 MB 帧缓存上。相对一个只负责几个滑块的设置窗口，这笔开销不合理，当场叫停。

回滚动作：移除 `eframe` / `winit` 依赖、删除 `settings_egui.rs`、`main.rs` 里全部 spawn/drain/publish 集成。exe **4.1 MB → 453 KB**，内存 **200 MB → 53.8 MB**。

**原生菜单自绘（owner-draw）**

不引入任何依赖，用 `MFT_OWNERDRAW` + `WM_MEASUREITEM` / `WM_DRAWITEM` 把日系清新落到菜单上：

| token | 落地 |
|---|---|
| `#fafaf8` 背景 | `FillRect` 铺底 |
| `#eef6fd` 悬停 | `ODS_SELECTED` 时换底色，只浅浅一层 |
| `#4a5568` 文字 | 等线细体（`CreateFontW` FW_LIGHT=300，禁粗体） |
| 发丝边框 | 1px、内缩 14px 的 `#e9e9e5` 分隔线 |
| `#64b5f6` 强调 | 当前值用实心蓝圆点，未选用空心圆环 |
| Ma 留白 | 行高 34px、左缩进 40px、分组标题独占 26px |

菜单项数据放全局池，`dwItemData` 只存 1-based 索引，菜单销毁无需释放任何东西。菜单项分 `Header / Radio / Check / Normal / Submenu / Separator` 六类。

**exe 只增加 5 KB**（453 → 458 KB，清理死代码后 452 KB）。

**功能复测**：点击「280 px」，窗口 188 → 308 实时变化，主窗口存活不闪退；内存 53.8 MB、静止 CPU 0%。

> 顺带清掉了几处调试期遗留的临时日志写入，其中 `play_bytes` 里那条**每次点击都写文件**的日志尤其要删，它会拖慢快速连点的响应。该文件本身在 MCI 方案后已是死代码，一并移除。

### 4.29 菜单收窄 + 圆角

老大反馈第一版自绘菜单「过宽、缺圆角」。

**收窄**：`MENU_WIDTH` 236 → **184**，行高 34 → 32、分组标题 26 → 24、分隔线 13 → 11。文字区仍留 40px 左缩进和 30px 右侧箭头位，`锁定（穿透）` 等最长标签不截断。

**圆角**：自绘只填内容区，外框是系统菜单窗口（类名 `#32768`）画的直角。在 `WM_DRAWITEM` 首次触发时（此时菜单窗口已创建）对它做两步：

```rust
// 1) Win11：DWM 圆角合成，保留系统阴影
DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND, 4);
// 2) 兜底：圆角 region，所有版本都生效
let rgn = CreateRoundRectRgn(0, 0, w, h, 12, 12);
SetWindowRgn(hwnd, rgn, true);
```

菜单窗口句柄优先取 `DRAWITEMSTRUCT.hwndItem`（若类名是 `#32768`），否则 `FindWindowW("#32768")`。用 `HashSet<HWND>` 按窗口去重，主菜单和子菜单各自只设一次；每次 `begin()` 清空。

**实测**：枚举到 2 个菜单窗口（主 208×169、子 208×388），两者四角均为圆角；用 `GetMenuItemRect` 精确点击「280 px」，窗口 188 → 308 实时变化，主窗口存活。

**两处收尾**：

- 无标记的菜单项原本也按「留 40px 给圆点」缩进，文字显得偏右。新增 `Kind::Plain`（左缩进 16px）用于主菜单和托盘菜单；子菜单的「恢复默认」仍用 `Kind::Normal`，与上方的单选圆点保持对齐。
- 退出项文案由「退出圆宝」改为 **「圆宝再见」**，带点人情味。

### 4.30 松手音与「是否点击」解耦

**现象**：拖动小猫后松开，只有按下音，没有第二段音。

**根因**：两版都把松手音绑在「是否算一次抚摸」上。

```rust
// native
if pressed_hit { if !moved { play("pat_up.wav"); } }
```
```js
// web
if (wasPat && lastPressHit) playUp()   // is_pat 要求 !moved
```

Web 版的 `is_pat` 原本是为了「拖动结束后在桌面空白处松手，别误判成点击」，这个判定对**回弹/弹跳**仍然必要，但不该顺带管音效。

**修复**：把音效从该判定里摘出来，只保留「按下时确实点在猫身上」这一个前提。

```rust
if s.pressed_hit { s.bounce_t = Some(0.0); play("pat_up.wav"); }
```
```js
if (lastPressHit) playUp()
```

现在点击、长按、拖动、甩出都有完整的「拿起 → 放下」两段反馈；`is_pat` / `moved` 仍各自管回弹与物理。

**验证**：模拟明显拖动（窗口位移 -154/-84）后松手，日志确认 `play(up)` 被调用；验证日志已移除。两版均重新编译，`dist/` 下两个 Web 版 exe 已同步。

### 4.31 画质 / 绿边 / 图标 / 按压曲线（四项一起）

**① 画质对齐 Web 版（内存不增）**

老大反馈原生版画质略低、左上朝向胡子边缘有「绿幕痕迹」，希望内存控制在 60 MB 内适当提升。

排查结论：源帧本身没有绿幕（完全透明像素 RGB 是深灰 18/19/18），脏边来自**在非预乘 RGBA 上做插值**——透明底会渗进半透明边缘形成暗色描边；再叠加 `Triangle` 缩放偏软。

`load_one()` 三处改动，全部只在启动预载时执行一次，**常驻内存不变**：

1. **先预乘再缩放**：在预乘 alpha 空间做插值，透明底权重为 0，不再渗色。
2. `Triangle` → **`CatmullRom`**：明显锐利，又不像 Lanczos 在细线（胡子）旁产生振铃。
3. **边缘去污染 + despill**：源素材半透明边缘 RGB 均值仅 85/87/83（明显偏暗）。对 `0 < a < 250` 的像素，在半径 2 邻域里取最不透明像素的 RGB 替换；再把 G 超出 R/B 均值 6 以上的部分压到 1/4。

实测左上朝向放大图：暗边/绿边消失，猫身锐利。内存 **54.2 MB**（预算 60 MB），静止 CPU 0%。

**② exe 图标**

原生 exe 一直是通用图标。`winres` 靠 `reg.exe` 找 SDK 的 `rc.exe`，在受限环境失败（`cargo:warning=failed to embed icon resource`），`ExtractIconExW` 返回 0。

改为 `build.rs` **直接生成 PE `.res`**：解析 `assets/icon.ico` → 每帧写 `RT_ICON` 资源块 → 写 `RT_GROUP_ICON` → `cargo:rustc-link-arg-bins` 交给 link.exe。纯 Rust、零外部工具。

> 两个格式坑：文件开头必须有 32 字节 null 资源头；每个资源块的 `HeaderSize` 是 **32**（含 DataSize/HeaderSize 自身），写成 24 会 `LNK1136 无效或损坏的文件`。

验证：`ExtractIconExW` 从 0 → **1**；exe 458 KB → 576 KB（多的 116 KB 就是图标）。

**③ 按压曲线完整对齐 Web 版**

Web 版 `style.css` 是三段关键帧，原生版原来用 `ease_out_back` 单段近似，且**完全没实现落地挤压**（物理 `StepResult.landed/impact` 算了却丢了）。

改成 `Press` 阶段机 + 关键帧插值，精确复刻：

| 阶段 | 时长 | 曲线 / 关键帧 |
|---|---|---|
| `Down` 按下 | 0.07s | CSS `ease-out` → (1.17, 0.76) |
| `Bounce` 松手 | 0.34s | cubic-bezier(.3,.9,.4,1)：(1.17,.76)→(.96,1.06)→(1.02,.99)→(1,1) |
| `Land` 落地 | 0.22s | 同曲线：(1.16,.84)→(.97,1.03)→(1,1)，`impact > 280` 触发 |

CSS timing-function 是「按 x 反解参数 t 再取 y」，实现了牛顿迭代的 `cubic_bezier()`，不再拿 `ease_out_cubic` 近似。

**④ 老大反馈「起步慢、后面太快」**

- **后面太快**：按需重绘的 `changed` 阈值 0.002 把回弹尾巴吃掉了——后半段每帧 scale 变化小于阈值，被判定无变化跳过，动画提前静止。改成**动画期间强制每帧重绘**（`animating`），Idle 时仍走按需跳过。
- **起步慢**：mousedown 从 `Down(0.015)` 起，跳过 ease-out 最迟疑的开头，第一帧就有可见形变。
- 贴底时 `landed` 每帧为 true，加边沿保护避免 `Land` 被不断重置导致每帧重绘（静止 CPU 一度升到 1.17%，修后回 0%）。

验证：按下→回弹四阶段对比明显，回弹差异序列平滑收敛（36.9 → 12.8 → 6.3 → 5.2 → 0），不再一帧跳回。

> 待观察：`screen_size()` 用 `GetClientRect(GetDesktopWindow())`，返回的桌面尺寸略大于 `GetSystemMetrics(SM_CXSCREEN)`，快速甩出后猫可能停在主屏边缘外一点（如 2542,1272）。未改动，避免影响多显示器行为。

### 4.32 单文件 exe + 代码清理

**问题**：老大把 `native/target/release/yuanbao-native.exe` 单独复制到桌面，打不开。确实不是单文件——帧、音效、托盘图标都从 exe 相邻目录按 `../web/frames` 之类的路径找。

**方案：全部塞进 PE 资源**（沿用 4.31 里自己生成 `.res` 的机制，不引任何构建工具）：

| RCDATA ID | 内容 |
|---|---|
| 200 | 帧长度表（361 × u32） |
| 201 | 361 个 webp 顺序拼接 |
| 202 / 203 | pat_down.wav / pat_up.wav |

运行时 `FindResourceW` → `LoadResource` → `LockResource` 拿到只读字节，`frame_bytes(i)` 按长度表切片，`image::load_from_memory` 解码。图标 `RT_GROUP_ICON` 已嵌好，托盘改成 `LoadImageW(hinst, MAKEINTRESOURCE(1))` 从自身模块取。

**唯一例外**：MCI 的 `open` 只吃文件路径，不吃内存。首次播放时把两个 wav 释放到 `%TEMP%/yuanbao-deskpet-sounds/`（一次性，之后复用）。分发时仍然只需要一个 exe。

`frames_dir()` / `sounds_dir()` / `assets_dir()` 保留作**开发期兜底**（资源缺失时读磁盘），运行时优先走内嵌资源。

**验证**：把 exe 复制到只含它一个文件的空白目录，双击启动成功——窗口有效、帧渲染 std 73.6、图标数 1、两个 wav 正确释放；Private 内存 **55.5 MB**、静止 CPU ≈0%。exe **7.0 MB**。成品放在 `dist/圆宝桌宠-单文件.exe`。

**清理**：

- 删 `legacy-electron/`（含 package.json / package-lock.json / Electron 实现）
- 删 `native/src/settings.rs`（早期 wry/WebView2 设置面板，早已不在编译图里）
- 删 `main.rs` 里的 `settings_url()` 死函数
- README 同步到「原生单文件为主力 / WebView 版作对照」的当前架构

保留 `src-tauri/` 与 `web/`（WebView 对照版与素材源仍在用）、`archive/`（100 MB 历史素材）、`extracted_frames/`（26 MB 抠图中间产物）、`build/`（85 MB 验证截图）—— 这几项是素材与验证证据，未擅自删除。

### 4.33 穿透后无法自救（托盘补解锁）

**问题**：右键开启「锁定（穿透）」后，猫窗口带 `WS_EX_TRANSPARENT`，收不到任何鼠标消息 —— 也就再也没法右键打开菜单；而托盘菜单里只有「设置 / 显示隐藏 / 圆宝再见」，只能重启应用。

**修复**：托盘菜单加一项，文案随状态切换：

```rust
mui::add(menu, ID_LOCK, if locked { "解锁（可拖动）" } else { "锁定（穿透）" }, Kind::Plain);
```

`show_menu(hwnd, visible, locked)` 新增 `locked` 参数（从 `State.locked` 读），返回新的 `Action::ToggleLock`，主窗口收到后调 `set_locked(hwnd, !locked)`。

托盘图标由资源管理器绘制、不属于猫窗口，不受穿透影响，所以这条路径始终可用。

**闭环验证**（脚本模拟托盘右键 + `GetMenuItemRect` 精确点击）：

| 步骤 | 窗口 exstyle |
|---|---|
| 初始 | `normal` (0x08080088) |
| 托盘点「锁定（穿透）」 | **`WS_EX_TRANSPARENT`** (0x080800A8) |
| 穿透状态下再弹托盘菜单 → 点「解锁（可拖动）」 | **`normal`** (0x08080088) |

窗口全程存活，菜单文案随状态正确切换。

### 4.35 dsh-pet「宠物碰撞」的可行性评估 + 三项物理迁移

**问题**：dsh-pet 有可启用的 `petCollision`，能否翻译进原生版？

**结论：能翻译，但我们现在没有碰撞对象。** 看了 `dsh-pet/runtime/electron-helper/shared-core.js` 的 `collidePet`：

- 用 AABB 判断身体盒重叠（命中框按 640×360 归一化）
- 圆心连线作法线，**一维动量守恒 + e=0.995，质量 ∝ size²**
- 飞行方直接改速度；被撞方通过 `petBridge` IPC 收到 `hvx/hvy` 后 `startThrow`
- 一帧只处理一次，避免连锁抖动

数学部分只有 30 行纯函数，但它是**跨宠物**行为——需要多只宠物各有身份、位置广播、跨进程施力。我们的多开实例彼此完全无感（没有单实例锁，但也没有任何发现/同步机制）。

工作量分档：

| 方案 | 工作量 |
|---|---|
| 只翻译 `collidePet` | 1 小时（无人调用，死代码） |
| 真正多猫互撞（多开身份 + 发现 + 每帧广播 + 远程施力 + 多屏调试） | **1.5–2 天** |
| 单猫就能受益的三项 | 1–2 小时 |

**实际迁移了第三档**（同源物理里更成熟的部分）：

1. **落地挤压随冲击速度变化**（`impact_depth`）
   dsh-pet 的 `landingSquash`：冲速 300→1500 之间从压 20% 线性到压 45%，上限 0.8。
   我们原来固定压 16%，现在轻落浅压、猛砸深压。
2. **带回弹的挤压曲线**（`land_scale`）
   前 45% 二次加速压到最扁，后 55% 用 ease-out-back 弹回，允许过冲拉伸到 1.12；横向按 `1+(1-sy)*0.85` 补偿。
3. **高速子步进**（`physics::step`）
   按每 8px 一步拆分（最多 64 步），对齐 `throwSubsteps`。原来 `3600 px/s × 12ms = 43px` 单步积分可能直接穿过边界再弹回。摩擦改为整帧只应用一次，否则分子步累积会放大 n 倍。

**验证**：

- 临时日志实测落地：`impact=1377 → depth=0.404`（压到 0.596 倍高），`blocked=false`，确认按冲速变形且未被回弹动画挡住。
- 新增 2 个单元测试并纳入 `cargo test`：
  - `fast_down_throw_lands_with_impact`：2500px/s 下落能落地且 `impact > 280`
  - `substep_does_not_tunnel_at_max_speed`：3600px/s × 0.05s（单步 180px）连续 60 帧不越界
- 实测极端甩出 40 次采样 0 越界。

> 排查记录：第一次测 `land_hit` 一直不触发，`estimate_throw` 返回 `None`。原因是测试脚本向下拖到了屏幕底部之外，后半段光标被 clamp、位移为 0、速度估不出来——不是代码问题。换成向上甩立刻正常。

### 4.37 开机自启

**做法**：写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值名 `YuanBaoDeskPet`，值为带引号的 exe 完整路径。选 HKCU 而不是 HKLM：不需要管理员权限，也不污染系统全局。

用 `windows` crate 的 `RegOpenKeyExW` / `RegSetValueExW` / `RegDeleteValueW` 直接调 advapi32，**不启动 reg.exe**（沙箱黑名单拦的是 exe 进程，进程内 API 调用不受影响）。

**入口**：设置子菜单新增「开机自启」勾选项，`autostart_on()` 读当前状态决定勾选，点击后取反写入。

**踩坑**：新命令 ID 用了 140，但 `WM_RBUTTONUP` 的分发是 `100..=130`，140 被静默丢弃。日志显示命令没到 `handle_settings_cmd`，把范围放宽到 `100..=140` 后正常。

**验证**（`winreg` 读注册表，非 reg.exe）：

| 步骤 | 注册表值 |
|---|---|
| 初始 | 不存在 |
| 点「开机自启」 | `"D:\Codes\yuanbao-deskpet\native\target\release\yuanbao-native.exe"` |
| 再点一次 | 已删除 |

日志 `hit140 on=true target=false ok=true` 确认开关双向生效。

### 4.39 自更新（GitHub Releases）

**仓库**：`pudaa/yuanbao-deskpet`（只借 Release 分发，源码不必上传）。

**链路**：启动 5 秒后后台静默检查 → `GET /repos/{repo}/releases/latest` → 版本比对 → 有新版则弹原生 MessageBox 询问 → 下载 → 自替换 → 重启。

**几个关键决策**：

1. **HTTP 用 WinINet 而非 WinHTTP**：`INTERNET_OPEN_TYPE_PRECONFIG` 会自动继承系统/IE 代理，先代理后直连两级尝试正好覆盖国内网络；WinHTTP 得手动读注册表代理配置。虽然 WinHTTP 更"现代"，但这里省事更重要。
2. **默认超时是 60 秒**，网络不通会把 UI 卡死。用 `InternetSetOptionW` 把连接/接收收紧到 5s / 8s。
3. **自替换不用 .bat**：bat 里写中文路径会撞 GBK/UTF-8 编码问题。改成把**自身**复制到 `%TEMP%\yuanbao_updater.exe`，带 `--apply-update <新exe> <目标exe>` 运行——原生 exe 传参不会乱码。更新器轮询目标文件是否可写（等旧进程退出）→ 覆盖 → 启动新版 → 自删。
4. **资产名不写死**：遍历 `assets[]` 取第一个 `.exe`，仓库里叫什么都能认。

**版本**：`Cargo.toml` 提到 `1.0.0`，菜单项显示当前版本「检查更新（v1.0.0）」。

**实测**（仓库还没有 release）：弹框正确显示「检查更新失败：该仓库还没有发布 release」，说明 HTTP、JSON、错误分支、对话框全部打通。

> 注意：GitHub 对"没有 release"返回的是 **200 + `{"message":"Not Found"}`**，不是 404 状态码。`InternetOpenUrlW` 也只给句柄不给状态码，所以只能靠解析 body 里的 `message` 判断。

**发版方法**（每次更新只需三步）：
1. 新建 release，tag 形如 `v1.0.1`（必须大于代码里的 `CARGO_PKG_VERSION`）
2. 上传任意 `.exe` 资产（建议 `圆宝桌宠.exe`，7 MB 左右）
3. 填 Release notes —— 会原样显示在更新对话框里（截断到 400 字）

### 4.40 修「自己更新自己」：版本号必须内嵌在 exe 里

**现象**：老大发到 v1.0.1 之后，每次开机自启都提示更新，而"新版"就是它自己。

**根因**：版本号只有编译期常量 `env!("CARGO_PKG_VERSION")`，**exe 文件本身没有任何版本元数据**。Cargo.toml 里还停在 1.0.0，仓库 latest 是 v1.0.1 → `is_newer("v1.0.1", "1.0.0")` 恒为真 → 永远提示。

更深一层：就算把常量改成 1.0.1 也只是"编译时对上了"。exe 作为可分发的二进制，应该**自己知道自己是几版**，否则任何不经重编译的替换（手动复制、改名、回滚）都会让判断失真。

**修复**：

1. **`build.rs` 手写 VERSIONINFO 资源**（`RT_VERSION`）
   `VS_VERSION_INFO` 块 + `VS_FIXEDFILEINFO`(52 字节) + `StringFileInfo`(040904B0) + `VarFileInfo`(Translation)。
   用通用 `vs_block(key, value_len, wtype, value, children)` 递归拼装，每段按 DWORD 对齐、`wLength` 收尾回填。
   版本号从 `CARGO_PKG_VERSION` 读，发版只需改 Cargo.toml 一处。

2. **运行时优先读自身 PE 版本**
   `GetFileVersionInfoSizeW` → `GetFileVersionInfoW` → `VerQueryValueW("\\")` →
   按 `VS_FIXEDFILEINFO` 偏移 8/12 取 `dwFileVersionMS/LS`，拼成 `1.0.2`。
   读不到才回退到编译常量。四段号末尾的 `.0` 去掉再显示。

3. **版本提到 1.0.2**（1.0.1 已被旧代码占用，那个包更新过去仍然会误报）

**验证**：

| 检查项 | 结果 |
|---|---|
| 资源管理器/`VersionInfo` | FileVersion `1.0.1` → 改成 1.0.2 后正确 |
| ProductName / FileDescription | 圆宝桌宠 / 圆宝桌宠 · 原生版 |
| 手动「检查更新」（仓库 latest=v1.0.1） | 弹框「已经是最新版本 v1.0.2」，不再提示更新 |

> 关键认知：GitHub 上已发布的 v1.0.1 是**旧代码**打的包，没有 VERSIONINFO。所以修复版必须发成 **v1.0.2**；旧版（认为自己是 1.0.0）会提示更新到 1.0.2，更新完成后即自愈，之后不再误报。

### 4.41 人工验收

1. 双击 `dist\YuanBaoDeskPet.exe`。
2. **方向**：鼠标放到猫的正上方 → 猫应抬头看天；放到左侧 → 侧脸朝左；放到左下 → 低头看左下。
3. **跟手**：快速移动鼠标，猫头应紧跟，无明显拖尾。觉得慢就把设置里"跟手速度"拉到 1.5–2.0。
4. **绕圈**：鼠标贴着猫周围画圆 → 猫头转一圈，接缝处无跳变。
5. **左上→右上横扫**：从左上缓慢滑到右上，头应连续转过正上方，不出现来回跳或卡顿。
6. **中心保持**：鼠标移到猫身上（28px 内）→ 头停在当前朝向，不会跳回上看。
6. **拖动 / 锁定 / 置顶 / 设置**：同前。

### 4.42 音效可切换：圆宝叫声 / 鸭子叫声（v1.0.3）

**需求**：原来点猫只有 duck 音效组的「jijiji」两段（`pat_down.wav` / `pat_up.wav`），老大给了自家猫的一声 `圆宝叫声.mp3`，要求可切换、且默认换成它。

**素材处理**：mp3 是 0.89s / 48kHz 立体声 320kbps，首 94ms、尾 170ms 是静音。先转成 PCM s16le 48kHz 立体声（与既有 wav 参数一致），再裁掉首尾静音并加 20ms 淡出 → **0.65s / 124,878 B**。裁头的意义是点击后不再有近 100ms 的空等。

**结构设计**：音效不再按文件名判断，而是 `SoundPack { Meow, Duck }` 枚举：

| 音效组 | 按下 | 松手 |
|---|---|---|
| `Meow`（默认） | `meow.wav` | 静默 |
| `Duck` | `pat_down.wav` | `pat_up.wav` |

圆宝叫声只有一声，松手再响一次会变成「喵喵」，所以松手保持静默；鸭子组仍是两段。MCI 实例：down 4 路、up 2 路、meow 2 路（0.65s 的两路足够，快速连点会自然打断而不是叠成一团）。

**资源**：新增 RCDATA **204** = `meow.wav`，`build.rs` 一并嵌入。临时文件释放逻辑改成「长度对不上就重写」：换版本后 `%TEMP%` 里的旧缓存不会继续顶替新声音。

**设置入口**：设置子菜单新增「音效」分组，两项单选（`Radio`），命令 ID 150/151。顺手把 `WM_RBUTTONUP` 的分发范围从 `100..=140` 放宽到 `100..=160` —— 这正是 4.37 记过的「新命令 ID 越界被静默丢弃」坑，这次提前修掉。

**验证**：

| 项 | 结果 |
|---|---|
| 默认启动（无配置） | 窗口 188px、`config.json` 写入 `"sound": "meow"` |
| 真实光标点猫 | `%TEMP%\yuanbao-deskpet-sounds\meow.wav` 释放 124,878 B（说明 init_mci → 播放链路真的走到了） |
| 点菜单「圆宝叫声」 | 配置由 `duck` 变 `meow`，其余字段不动 |
| 菜单截图 | 「音效」分组、`圆宝叫声` / `鸭子叫声` 单选、勾选跟随配置（`build/check_settings_menu.png`） |

> 未能自动化验证的：声音是否真的从扬声器出来。MCI 播放无返回值可查，需人工点一次。

### 4.43 设置持久化 + 「恢复默认」其实一直是坏的

**问题**：原生版从一开始就没有任何配置持久化 —— 尺寸、不透明度、镜像、音效、窗口位置，重启全部回默认。老大原先没注意到，是全局 code review 时扫出来的。

**新增 `native/src/config.rs`**：读写 `%APPDATA%\YuanBaoDeskPet\config.json`，字段 `size / opacity / mirror / sound / x / y`。三个设计点：

1. **尺寸必须在预载帧之前读**：`load_one()` 按 `disp()` 缩放，所以 `config::load()` 放在 `main()` 进入 `unsafe` 块之前，而不是窗口创建之后。
2. **越界收敛**：手改坏的 JSON（size=9999、opacity 非数）一律 clamp；`sound` 不是 `duck` 就回落 `meow`。
3. **原子写**：临时文件 + rename，避免崩溃留半截 JSON。写失败静默 —— 桌宠不能因为写不了配置弹框。

**保存时机**：每次经菜单改动后立刻 `save_config()`；退出（`WM_CLOSE`）再记一次当前位置。桌宠没有「保存」按钮，用户也不该为了留住设置而正常退出一次。

**顺带修的真 bug**：`apply_cmd("reset_all")` 原来**只返回一份默认值 JSON、不真正改状态**，所以菜单点「恢复默认」等于没反应。现在先放锁再逐项下发 `set_size / set_opacity / set_mirror / set_sound / set_locked / place_corner`，复用各条命令自己的重载与重排逻辑。

**验证**：

| 步 | 结果 |
|---|---|
| 无配置启动 → 关闭 | 生成 `{"size":160,"opacity":1.0,"mirror":false,"sound":"meow","x":2332,"y":1152}` |
| 改 220px / 80% / duck / 300,200 → 重启 | 窗口 248×248 @ 300,200（220+14×2 正确） |
| 点「恢复默认」 | 窗口 248 → **188**，配置回到 160 / 1.0 / meow（`build/check_pet_220.png`） |

位置校验：保存的坐标必须与屏幕有交集，换分辨率/拔副屏后回落到右下角，避免猫跑到看不见的地方。

### 4.44 源码开源与素材隔离

仓库 `pudaa/yuanbao-deskpet` 从「只放 README + Release 资产」升级为**源码仓库**，但圆宝的影像与声音素材不入库。

`.gitignore` 排除：`web/frames/`（361 帧）、`web/sounds/`（3 段音效）、`assets/*.png|ico`、`extracted_frames/`、`archive/`、`build/`、`dist/`、两个 `target/`。

**取舍说明**：

- 克隆者拿不到素材，必须自备 `assets/icon.ico` 与 `web/frames/` 才能构建出完整 exe —— 这是刻意的。
- Release 的 exe 仍内嵌素材且公开可下载，理论上可被解包。要彻底封闭只能转私有仓库，但私有后 `GET /repos/.../releases/latest` 的自更新要带 token，会断掉静默更新。权衡后维持公开仓库。
- 原始 `圆宝叫声.mp3` 归档在 `archive/sounds/`（同样不入库）。

### 4.45 v1.0.3 人工验收

1. 双击 `dist\圆宝桌宠-单文件.exe`（首次会以默认 160px 出现在右下角）。
2. 点一下猫 → 应该听到一声「喵」；右键 ▸ 设置 ▸ 音效 ▸ 鸭子叫声 后再点 → 应回到 jijiji 两段音。
3. 再点 ▸ 圆宝叫声，退出（右键 ▸ 圆宝再见），重新打开 → 应仍是圆宝叫声（配置持久化）。
4. 改尺寸 280px、不透明度 80%，拖动到屏幕中间，退出重开 → 尺寸、透明度、位置都应保持。
5. 设置 ▸ 恢复默认 → 回到 160px / 100% / 圆宝叫声 / 右下角。
6. 检查 `%APPDATA%\YuanBaoDeskPet\config.json` 内容与界面一致。

---

## 5. 高 DPI 适配

1. canvas 位图 = CSS 尺寸 × `devicePixelRatio`，`setTransform` 把绘制坐标缩回 CSS px，125%/150%/200% 均按物理像素 1:1 绘制。
2. `matchMedia('(resolution: Ndppx)')` 监听跨屏 DPI 变化（纯 DPI 变化时 `window.resize` 不触发）。
3. 帧源 384px 对默认 160px 显示，**DPR 2.4 以内无损**；放大到 360px 时用 `imageSmoothingQuality: 'high'`。

窗口尺寸与位置按物理像素计算（`scale_factor` 参与），避免高 DPI 落位偏移。

---

## 6. 设置面板

托盘右键 →「设置…」。配置持久化在 `%APPDATA%\com.yuanbao.deskpet\config.json`。

| 分组 | 项目 | 范围 | 默认 |
|---|---|---|---|
| 外观 | 尺寸 | 120–360 px | 160 |
| 外观 | 不透明度 | 40–100% | 100% |
| 外观 | 水平镜像 | 开关 | 关 |
| 外观 | 界面主题 | 日系清新 / 少女漫画 | 日系清新 |
| 行为 | 跟手速度 | 0.5–2.0× | 1.0 |
| 行为 | 亲近半径 | 40–180 px | 48 |
| 行为 | 锁定位置（鼠标穿透） | 开关 | 关 |
| 行为 | 始终置顶 | 开关 | 开 |
| 行为 | 开机自动启动 | 开关 | 关 |

**主题设计**：日系清新为骨架（0.5px 发丝边框、米白 `#fafaf8`、禁粗体），少女漫画的樱花粉 `#ffb7c5` 作强调 + 6% 网点纹理。正文统一 `#4a5568`（浅底 7.2:1），规避两个原始风格对比度不达标（D 级）的问题。

**实测**：两套主题均截图验证（`build/win_settings4.png`、`build/win_shoujo.png`）；把 `config.json` 的 `idleRadius` 改成 120 后重启，面板显示 `120 px` 且滑块在 2/3 处，确认读数来自真实配置而非 HTML 默认值。

---

## 7. 启动

```bash
cd D:\Codes\yuanbao-deskpet
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release --manifest-path native\Cargo.toml      # 主力：原生单文件版
cargo build --release --manifest-path src-tauri\Cargo.toml   # 对照：WebView 版
cargo build --manifest-path src-tauri\Cargo.toml             # debug

node tools/test_angles_v2.js            # 映射数学自测
python tools/chroma_key.py --raw v2raw --all --size 384   # 抠图
python tools/export_frames.py           # 导出 WebP 到 web/frames
python tools/build_sequence.py          # 重建锚点表与 contact sheet
python tools/probe_directions.py        # 八方向真机验证（猫需在屏幕中央）
```

> 素材不动产出：抠图与导出后的 `web/frames/`、`web/sounds/` 已 `.gitignore`，见 4.44。

网页预览：浏览器打开 `web/index.html`，无 Tauri 环境时自动切预览模式。

---

## 8. 回退

| 场景 | 操作 |
|---|---|
| 找不到猫 | 托盘图标单击 = 显示/隐藏 |
| 位置错乱 | 托盘「回到右下角」，或删 `config.json` 重启 |
| 退回 V1 素材 | `extracted_frames_v1/` + `assets/frames.v1.json` 已保留，需同时改 `renderer.js` 的 `FC`=241 与映射公式 |
| 退回 Electron | `legacy-electron/`，`npm i && npm start` |
| 换帧数 | 必须同步改 `web/renderer.js` 的 `FC` 与 `assets/frames.json` 的 `frameCount` |

---

## 9. 可调参数

| 位置 | 参数 | 默认 | 作用 |
|---|---|---|---|
| `main.rs` | `POLL_MS` | 16 | 全局光标采样周期（60Hz） |
| `main.rs` | `ORIGIN_SCREEN` | false | 极点：猫中心（语义正确）/ 屏幕中心 |
| `main.rs` | `CENTER_DEAD` | 20 | 极点附近死区，防角度噪声 |
| `main.rs` | `idle_radius` | 48 | 距中心多近算 idle |
| `renderer.js` | `FC` | 361 | 帧数 |
| `renderer.js` | `DEG_OFFSET` | −90 | 帧 0 对应的屏幕方位角 |
| `renderer.js` | `tau` | 0.028/0.055/0.14 | 大跨度 / 常态 / 回 idle |

---

## 10. 已知限制

1. **猫在屏幕角落时，可视角有限**。以猫为极点、猫在右下角时，鼠标可达方位角约 −180°~−45°（左上方），猫头只在这个扇区里转。这是"始终面向鼠标"的**正确行为**，不是 bug。想体验完整 360°，把猫拖到屏幕中央再绕圈。
2. **素材转的是头，不是身子**。头转 360° 在解剖上不可能，视觉上接近"贴着脖子绕一圈"，是素材本身的取舍。
3. **没有"正脸平视"姿态**。V2 素材全程在做转头，极点改用"低头正要抬头"的帧 0 叠化呈现。
4. **抬头段被裁掉了**。帧 0–31 只在极点使用；若将来想让它当独立动作（比如被点击时抬一次头），可以直接播放这 32 帧。
5. 361 帧 WebP 约 16MB，运行时解码后内存约 90MB；若需更小可降到 180 帧（每 2°）配插值。
6. 托盘菜单交互与开机自启写注册表未做自动化验证，需人工点一次。
7. **音效切换只能人工听验证**：MCI 播放无返回值，自动化只能确认音频文件释放与命令链路。
8. **源码仓库不含素材**（见 4.44）：clone 后缺 `web/frames/`、`web/sounds/`、`assets/icon.ico` 无法构建完整 exe。
9. 配置只写在 `config.json`，没有「导出/导入」；损坏时的恢复手段就是删掉该文件重启。
