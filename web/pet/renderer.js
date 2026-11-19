(() => {
  'use strict'

  const T = window.__TAURI__
  const isTauri = !!(T && T.core)
  const invoke = (cmd, args) => (isTauri ? T.core.invoke(cmd, args || {}) : Promise.resolve())

  // V2 素材结构：
  //   帧 0..31  = 从"低头看前方"抬起到"看正上方"的抬头动画（不属于朝向角）
  //   帧 32..360 = 真正的 360° 转头；帧 32 与帧 360 姿态基本重合（闭环最优点）
  const FC = 361
  const DEG_OFFSET = -90
  const MAIN_START = 32
  const MAIN_SPAN = 360 - MAIN_START // 328
  const SEAM = 3
  const PAD = 14

  const RING_LO = MAIN_START
  const RING_HI = 360
  const RING_LEN = RING_HI - RING_LO

  // 动画模式：不做像素叠化，全部走帧序列
  const M_FOLLOW = 0
  const M_TO_IDLE = 1 // 当前朝向 -> 帧 32 -> 帧 0（低头收势）
  const M_IDLE = 2 // 停在帧 0
  const M_TO_FOLLOW = 3 // 帧 0 -> 帧 32（抬头），再交回跟随

  const cv = document.getElementById('cv')
  const ctx = cv.getContext('2d')
  const hint = document.getElementById('hint')
  const ctxMenu = document.getElementById('ctx')

  let petSize = 160
  let winSize = petSize + PAD * 2
  let dpr = window.devicePixelRatio || 1
  let smoothing = 1

  function layout() {
    dpr = window.devicePixelRatio || 1
    winSize = window.innerWidth || petSize + PAD * 2
    cv.width = Math.max(1, Math.round(winSize * dpr))
    cv.height = Math.max(1, Math.round(winSize * dpr))
    cv.style.width = winSize + 'px'
    cv.style.height = winSize + 'px'
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.imageSmoothingEnabled = true
    ctx.imageSmoothingQuality = 'high'
  }

  let dprMedia = null
  function watchDpr() {
    if (dprMedia) dprMedia.removeEventListener('change', onDprChange)
    dprMedia = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`)
    dprMedia.addEventListener('change', onDprChange)
  }
  function onDprChange() {
    layout()
    watchDpr()
  }

  watchDpr()
  window.addEventListener('resize', layout)

  // ---- 帧按需加载（环形窗口 + 远端淘汰）----
  // 361 帧全量常驻需 69MB（224² RGBA）。转头只需当前角度附近一小段，
  // 所以只保留当前帧周围的窗口，远端帧主动断引用让 Blink 回收解码缓冲。
  const base = '../frames'
  const CACHE_MAX = 88
  const PREFETCH_AHEAD = 18
  const PREFETCH_BACK = 10

  const cache = new Map()
  const loading = new Set()
  let ready = false
  let lastGood = null
  let lastRefill = -1

  const pad4 = (i) => String(i).padStart(4, '0')
  const usable = (im) => !!(im && im.complete && im.naturalWidth > 0)

  function ringDist(a, b) {
    const d = Math.abs((((a % FC) + FC) % FC) - (((b % FC) + FC) % FC))
    return Math.min(d, FC - d)
  }

  function frameAt(i) {
    const idx = ((Math.round(i) % FC) + FC) % FC
    let img = cache.get(idx)
    if (img) {
      cache.delete(idx)
      cache.set(idx, img)
      return img
    }
    if (loading.has(idx)) return null
    loading.add(idx)
    img = new Image()
    img.decoding = 'async'
    img.onload = () => {
      loading.delete(idx)
      if (!ready) {
        ready = true
        hint.classList.remove('show')
        hint.textContent = ''
      }
    }
    img.onerror = () => {
      loading.delete(idx)
      console.warn('[yuanbao] frame failed:', idx)
    }
    img.src = `${base}/${pad4(idx)}.webp`
    cache.set(idx, img)
    return img
  }

  function refill(cur) {
    for (let d = 0; d <= PREFETCH_AHEAD; d++) frameAt(cur + d)
    for (let d = 1; d <= PREFETCH_BACK; d++) frameAt(cur - d)

    if (cache.size <= CACHE_MAX) return
    const byDist = [...cache.keys()].sort((a, b) => ringDist(a, cur) - ringDist(b, cur))
    for (let i = CACHE_MAX; i < byDist.length; i++) {
      const k = byDist[i]
      const im = cache.get(k)
      if (im && im !== lastGood) {
        im.src = ''
      }
      cache.delete(k)
    }
  }

  hint.classList.add('show')
  hint.textContent = '圆宝加载中…'

  /**
   * 命中判定：判断按下点是否落在猫身上。
   * 窗口是方形、猫只占中间一块，点四角的透明区不该算"摸到"。
   * 优先读像素 alpha；取样不可用时回退到几何方块（绝不再退化成"永远命中"）。
   */
  let lastPressHit = false

  function alphaAt(clientX, clientY) {
    try {
      let x = clientX
      if (document.body.classList.contains('mirror')) x = winSize - x
      const px = Math.round(x * dpr)
      const py = Math.round(clientY * dpr)
      if (px < 0 || py < 0 || px >= cv.width || py >= cv.height) return 0
      const d = ctx.getImageData(px, py, 1, 1).data
      if (!d || d.length < 4) return -1
      return d[3]
    } catch (_) {
      return -1
    }
  }

  function withinPetBox(clientX, clientY) {
    const off = (winSize - petSize) / 2
    return (
      clientX >= off &&
      clientX <= off + petSize &&
      clientY >= off &&
      clientY <= off + petSize
    )
  }

  function hitTest(clientX, clientY) {
    const a = alphaAt(clientX, clientY)
    if (a >= 0) return a > 24
    return withinPetBox(clientX, clientY)
  }

  // ---- 抚摸音效（复用 dsh-whale-widget 的 duck 音效组）----
  const sndDown = new Audio('../sounds/pat_down.mp3')
  const sndUp = new Audio('../sounds/pat_up.mp3')
  sndDown.volume = 0.55
  sndUp.volume = 0.55
  sndDown.addEventListener('error', () => console.warn('[yuanbao] pat_down load failed'))
  sndUp.addEventListener('error', () => console.warn('[yuanbao] pat_up load failed'))

  function playDown() {
    try {
      sndDown.currentTime = 0
      sndDown.play().catch(() => {})
    } catch (_) {}
  }

  function playUp() {
    try {
      sndUp.currentTime = 0
      sndUp.play().catch(() => {})
    } catch (_) {}
  }

  // ---- 状态 ----
  let mode = M_FOLLOW
  let cur = 0.25
  let targetU = 0.25
  let held = false
  let firstFrame = true
  let lastT = performance.now()

  // 回归动画参数
  let animT = 0
  let animDur = 0.5
  let animFrom = RING_LO // 段 A 起点帧
  let animDir = 1
  let animLenA = 0
  let animStart = RING_LO // M_TO_FOLLOW 起点

  function wrapDelta(d) {
    return d - Math.round(d)
  }

  function ringWrap(f) {
    let x = f
    while (x > RING_HI) x -= RING_LEN
    while (x < RING_LO) x += RING_LEN
    return x
  }

  /** 从 from 沿环走到 to 的最短带符号距离 */
  function ringShortest(from, to) {
    let d = to - from
    while (d > RING_LEN / 2) d -= RING_LEN
    while (d < -RING_LEN / 2) d += RING_LEN
    return d
  }

  function framePos() {
    // 水平镜像时画面被 scaleX(-1) 翻转，角度要同步取反，否则猫会朝相反方向看
    const u = document.body.classList.contains('mirror') ? (1 - cur + 1) % 1 : cur
    const deg = (((u * 360 + DEG_OFFSET) % 360) + 360) % 360
    return MAIN_START + (deg / 360) * MAIN_SPAN
  }

  function uFromFrame(f) {
    const deg = ((f - MAIN_START) / MAIN_SPAN) * 360
    return ((((deg + 90) / 360) % 1) + 1) % 1
  }

  /** 当前应显示的帧号（含所有模式） */
  function displayFrame() {
    if (mode === M_FOLLOW) return framePos()

    if (mode === M_TO_IDLE) {
      const total = animLenA + MAIN_START
      const s = animT * total
      if (s <= animLenA) return ringWrap(animFrom + animDir * s)
      return Math.max(0, MAIN_START - (s - animLenA))
    }

    if (mode === M_IDLE) return 0

    // M_TO_FOLLOW：从 animStart 抬头到帧 32
    return animStart + (MAIN_START - animStart) * animT
  }

  function draw() {
    const off = (winSize - petSize) / 2
    let f = displayFrame()
    if (f < 0) f = 0
    if (f > RING_HI) f = RING_HI

    let i0 = Math.floor(f)
    const frac = f - i0
    let i1 = Math.min(i0 + 1, RING_HI)
    if (i0 >= RING_HI) {
      i0 = RING_HI
      i1 = RING_HI
    }

    const a = frameAt(i0)
    const src = usable(a) ? a : lastGood
    if (!usable(src)) return
    if (usable(a)) lastGood = a

    ctx.clearRect(0, 0, winSize, winSize)
    ctx.globalAlpha = 1
    ctx.drawImage(src, off, off, petSize, petSize)

    if (i1 !== i0 && frac > 0.004) {
      const b = frameAt(i1)
      if (usable(b)) {
        ctx.globalAlpha = frac
        ctx.drawImage(b, off, off, petSize, petSize)
        ctx.globalAlpha = 1
      }
    }
  }

  function enterIdle() {
    const from = framePos()
    const d = ringShortest(from, RING_LO)
    animFrom = from
    animDir = d >= 0 ? 1 : -1
    animLenA = Math.abs(d)
    animT = 0
    // 路径越长给的时间越多，但控制在 0.25–0.9s
    animDur = Math.min(0.9, 0.22 + (animLenA + MAIN_START) / 520)
    mode = M_TO_IDLE
  }

  function enterFollow() {
    animStart = Math.max(0, Math.min(displayFrame(), MAIN_START))
    animT = 0
    animDur = 0.3
    mode = M_TO_FOLLOW
  }

  function advance(dt) {
    if (mode === M_FOLLOW) {
      const delta = wrapDelta(targetU - cur)
      const ad = Math.abs(delta)
      let tau = firstFrame ? 0.4 : ad > 0.15 ? 0.028 : 0.055
      tau = tau / Math.max(0.5, smoothing)
      const k = 1 - Math.exp(-dt / tau)
      cur = (cur + delta * k + 1) % 1
      if (firstFrame && ad < 0.002) firstFrame = false
      return
    }

    if (mode === M_TO_IDLE) {
      animT = Math.min(1, animT + dt / animDur)
      if (animT >= 1) {
        mode = M_IDLE
        animT = 1
      }
      return
    }

    if (mode === M_TO_FOLLOW) {
      animT = Math.min(1, animT + dt / animDur)
      if (animT >= 1) {
        mode = M_FOLLOW
        cur = uFromFrame(MAIN_START) // 与帧 32 对齐，避免回跟随瞬间跳变
        firstFrame = false
      }
      return
    }
    // M_IDLE 不动
  }

  /**
   * 唯一的推进入口：由 rAF 和 IPC 事件共同调用。
   * 共享 lastT，dt 基于真实时间，不会被重复推进。
   * 这样即使 WebView 在窗口无操作时节流了 rAF，光标事件仍能驱动动画走完。
   */
  function step() {
    const now = performance.now()
    const dt = Math.min(0.05, (now - lastT) / 1000)
    if (dt <= 0.0004) return
    lastT = now
    if (!ready) return
    advance(dt)
    const curFrame = Math.round(displayFrame())
    if (curFrame !== lastRefill) {
      lastRefill = curFrame
      refill(curFrame)
    }
    draw()
  }

  function loop() {
    step()
    requestAnimationFrame(loop)
  }
  // 先主动拉一次初始帧：step() 里 `if (!ready) return` 会挡住所有预取，
  // 而 ready 又要靠帧加载回来才置真——不先拉一次就会互相等待、永远白屏。
  refill(MAIN_START)

  requestAnimationFrame(loop)

  layout()

  window.__dbg = () => ({
    mode,
    cur,
    targetU,
    held,
    animT,
    ready,
    loaded,
    petSize,
    winSize,
    dpr,
    smoothing,
    frame: displayFrame(),
  })

  // ---- 右键菜单 ----
  let curLocked = false

  function openCtxMenu(x, y) {
    const lockBtn = ctxMenu.querySelector('[data-act="lock"]')
    if (lockBtn) lockBtn.textContent = curLocked ? '解锁（可拖动）' : '锁定（穿透）'
    ctxMenu.classList.add('show')
    const mw = ctxMenu.offsetWidth
    const mh = ctxMenu.offsetHeight
    const px = Math.max(2, Math.min(x, window.innerWidth - mw - 2))
    const py = Math.max(2, Math.min(y, window.innerHeight - mh - 2))
    ctxMenu.style.left = px + 'px'
    ctxMenu.style.top = py + 'px'
  }

  function closeCtxMenu() {
    ctxMenu.classList.remove('show')
  }

  // 只让左键拖动窗口。之前用 data-tauri-drag-region，右键也会触发拖动，
  // 导致右键菜单时有时无、窗口被拖走。
  const stageEl = document.getElementById('stage')
  stageEl.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return
    if (ctxMenu.contains(e.target)) return
    if (!isTauri) return
    // 只有真的点在猫身上才算"抚摸"；点在窗口空白处仍然可以拖窗口
    lastPressHit = hitTest(e.clientX, e.clientY)
    if (lastPressHit) {
      playDown()
      // 按下即压住，拖拽期间保持，松开由 pet:release 回弹
      document.body.classList.remove('bounce')
      document.body.classList.add('pressing')
    }
    T.window.getCurrentWindow().startDragging()
  })

  document.addEventListener('contextmenu', (e) => {
    e.preventDefault()
    e.stopPropagation()
    openCtxMenu(e.clientX, e.clientY)
  })

  document.addEventListener('mousedown', (e) => {
    if (!ctxMenu.contains(e.target)) closeCtxMenu()
  })

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeCtxMenu()
  })

  ctxMenu.addEventListener('click', async (e) => {
    const btn = e.target.closest('button')
    if (!btn) return
    e.stopPropagation()
    closeCtxMenu()
    const act = btn.dataset.act
    if (act === 'settings') invoke('open_settings_cmd')
    else if (act === 'lock') invoke('set_locked', { value: !curLocked })
    else if (act === 'corner') invoke('place_corner')
    else if (act === 'quit') invoke('quit_app')
  })

  // ---- 输入 ----
  if (isTauri) {
    T.event.listen('pet:cursor', (e) => {
      const p = e.payload
      if (p.hold) {
        if (!held) {
          held = true
          if (mode === M_FOLLOW) enterIdle()
        }
      } else {
        if (held) {
          held = false
          if (mode === M_IDLE || mode === M_TO_IDLE) enterFollow()
        }
        targetU = p.u
      }
      step() // 事件驱动推进，摆脱对 rAF 的依赖
    })

    T.event.listen('pet:size', (e) => {
      petSize = e.payload.petSize
      smoothing = e.payload.smoothing
      layout()
    })
    T.event.listen('pet:opacity', (e) => {
      document.documentElement.style.opacity = String(e.payload)
    })
    T.event.listen('pet:mirror', (e) => {
      document.body.classList.toggle('mirror', !!e.payload)
    })
    T.event.listen('pet:locked', (e) => {
      curLocked = !!e.payload
    })
    // 松手：无论是点一下还是拖动结束，都要解除压住状态并回弹
    T.event.listen('pet:release', (e) => {
      const wasPressing = document.body.classList.contains('pressing')
      document.body.classList.remove('pressing')
      // 只要之前压住了（点击或拖动），松手都要回弹，否则会"啪"一下瞬间弹回
      if (wasPressing) {
        document.body.classList.remove('bounce')
        void document.body.offsetHeight
        document.body.classList.add('bounce')
        clearTimeout(window.__bounceTimer)
        window.__bounceTimer = setTimeout(() => document.body.classList.remove('bounce'), 380)
      }
      // 松手音与 is_pat 解耦：拖动/甩出松手也要有第二段音
      if (lastPressHit) playUp()
      lastPressHit = false
    })
    T.event.listen('pet:landed', (e) => {
      const impact = Number(e.payload) || 0
      if (impact < 280) return
      document.body.classList.remove('land')
      void document.body.offsetHeight
      document.body.classList.add('land')
      clearTimeout(window.__landTimer)
      window.__landTimer = setTimeout(() => document.body.classList.remove('land'), 240)
    })

    T.core
      .invoke('get_config')
      .then((c) => {
        if (!c) return
        petSize = c.size
        smoothing = c.smoothing
        curLocked = !!c.locked
        document.documentElement.style.opacity = String(c.opacity)
        document.body.classList.toggle('mirror', !!c.mirror)
        layout()
      })
      .catch(() => {})
  } else {
    window.addEventListener('mousemove', (e) => {
      const cx = window.innerWidth / 2
      const cy = window.innerHeight / 2
      const dx = e.clientX - cx
      const dy = e.clientY - cy
      if (Math.hypot(dx, dy) < 28) {
        if (!held) {
          held = true
          if (mode === M_FOLLOW) enterIdle()
        }
      } else {
        if (held) {
          held = false
          if (mode === M_IDLE || mode === M_TO_IDLE) enterFollow()
        }
        targetU = (Math.atan2(dy, dx) + Math.PI) / (2 * Math.PI)
      }
    })
    petSize = Math.min(160, window.innerWidth - PAD * 2)
    layout()
  }
})()
