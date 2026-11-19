(() => {
  'use strict'

  const T = window.__TAURI__
  if (!T || !T.core) {
    document.getElementById('metaText').textContent = '需在桌宠内打开此页'
    return
  }
  const invoke = (cmd, args) => T.core.invoke(cmd, args || {})

  const el = {
    size: document.getElementById('size'),
    sizeVal: document.getElementById('sizeVal'),
    opacity: document.getElementById('opacity'),
    opacityVal: document.getElementById('opacityVal'),
    smoothing: document.getElementById('smoothing'),
    smoothingVal: document.getElementById('smoothingVal'),
    idle: document.getElementById('idle'),
    idleVal: document.getElementById('idleVal'),
    locked: document.getElementById('locked'),
    mirror: document.getElementById('mirror'),
    alwaysOnTop: document.getElementById('alwaysOnTop'),
    autostart: document.getElementById('autostart'),
    corner: document.getElementById('corner'),
    reset: document.getElementById('reset'),
    close: document.getElementById('close'),
    themeBtns: Array.from(document.querySelectorAll('[data-theme-btn]')),
  }

  let syncing = false
  let dragging = false

  function debounce(fn, ms) {
    let t
    return (...a) => {
      clearTimeout(t)
      t = setTimeout(() => fn(...a), ms)
    }
  }

  const pushSize = debounce((v) => invoke('set_size', { size: v }), 60)
  const pushOpacity = debounce((v) => invoke('set_opacity', { value: v }), 60)
  const pushSmoothing = debounce((v) => invoke('set_smoothing', { value: v }), 60)
  const pushIdle = debounce((v) => invoke('set_idle_radius', { value: v }), 60)

  function paintTheme(theme) {
    document.body.dataset.theme = theme === 'shoujo' ? 'shoujo' : 'fresh'
    el.themeBtns.forEach((b) => b.classList.toggle('on', b.dataset.themeBtn === theme))
  }

  function pick(obj, keys, fallback) {
    for (const k of keys) {
      if (obj && obj[k] !== undefined && obj[k] !== null) return Number(obj[k])
    }
    return fallback
  }

  function apply(cfg) {
    if (!cfg) return
    const size = pick(cfg, ['size'], 160)
    const opacity = pick(cfg, ['opacity'], 1)
    const smoothing = pick(cfg, ['smoothing'], 1)
    const idleR = pick(cfg, ['idleRadius', 'idle_radius'], 28)

    syncing = true
    if (!dragging) {
      el.size.value = Math.round(size)
      el.sizeVal.textContent = `${Math.round(size)} px`
      el.opacity.value = Math.round(opacity * 100)
      el.opacityVal.textContent = `${Math.round(opacity * 100)}%`
      el.smoothing.value = smoothing
      el.smoothingVal.textContent = `${smoothing.toFixed(1)}×`
      el.idle.value = Math.round(idleR)
      el.idleVal.textContent = `${Math.round(idleR)} px`
    }
    el.locked.checked = !!cfg.locked
    el.mirror.checked = !!cfg.mirror
    el.alwaysOnTop.checked = !!cfg.alwaysOnTop
    el.autostart.checked = !!cfg.autostart
    paintTheme(cfg.theme)
    syncing = false

    const dbg = document.getElementById('dbg')
    if (dbg) dbg.textContent = JSON.stringify(cfg).slice(0, 120)
  }

  el.size.addEventListener('input', () => {
    const v = Number(el.size.value)
    el.sizeVal.textContent = `${v} px`
    dragging = true
    pushSize(v)
  })
  el.size.addEventListener('change', () => {
    dragging = false
  })

  el.opacity.addEventListener('input', () => {
    const v = Number(el.opacity.value)
    el.opacityVal.textContent = `${v}%`
    pushOpacity(v / 100)
  })

  el.smoothing.addEventListener('input', () => {
    const v = Number(el.smoothing.value)
    el.smoothingVal.textContent = `${v.toFixed(1)}×`
    pushSmoothing(v)
  })

  el.idle.addEventListener('input', () => {
    const v = Number(el.idle.value)
    el.idleVal.textContent = `${v} px`
    pushIdle(v)
  })

  el.locked.addEventListener('change', () => {
    invoke('set_locked', { value: el.locked.checked })
  })

  el.mirror.addEventListener('change', () => {
    invoke('set_mirror', { value: el.mirror.checked })
  })

  el.alwaysOnTop.addEventListener('change', () => {
    invoke('set_always_on_top', { value: el.alwaysOnTop.checked })
  })

  el.autostart.addEventListener('change', () => {
    invoke('set_autostart', { value: el.autostart.checked })
  })

  el.themeBtns.forEach((b) => {
    b.addEventListener('click', () => {
      paintTheme(b.dataset.themeBtn)
      invoke('set_theme', { value: b.dataset.themeBtn })
    })
  })

  el.corner.addEventListener('click', () => invoke('place_corner'))
  el.reset.addEventListener('click', async () => {
    const cfg = await invoke('reset_all')
    apply(cfg)
  })
  el.close.addEventListener('click', () => {
    T.window.getCurrentWindow().close()
  })

  T.event.listen('settings:state', (e) => {
    if (!syncing) apply(e.payload)
  })

  invoke('get_config').then(apply).catch(() => {})

  const meta = document.getElementById('metaText')
  if (meta) {
    meta.textContent = `361 帧 · 视口 ${window.innerWidth}×${window.innerHeight} · DPR ${(window.devicePixelRatio || 1).toFixed(2)}`
  }
})()
