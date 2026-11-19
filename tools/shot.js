/**
 * 渲染验证：加载 index.html（网页预览路径），模拟不同鼠标方位，截取画面。
 * 用法: npx electron scripts/shot.js
 */
const { app, BrowserWindow } = require('electron')
const path = require('path')
const fs = require('fs')

app.disableHardwareAcceleration()

const OUT = path.join(__dirname, '..', 'build', 'shots')
const SIZE = 380

// 俯仰/方位采样：绕一圈 12 个方位 + 中心（idle）
const RADIUS = 150
function points() {
  const pts = []
  for (let i = 0; i < 12; i++) {
    const a = (i / 12) * Math.PI * 2
    pts.push({ name: `u${String(i).padStart(2, '0')}`, x: SIZE / 2 + Math.cos(a) * RADIUS, y: SIZE / 2 + Math.sin(a) * RADIUS })
  }
  pts.push({ name: 'idle', x: SIZE / 2, y: SIZE / 2 })
  return pts
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

app.whenReady().then(async () => {
  fs.mkdirSync(OUT, { recursive: true })

  const win = new BrowserWindow({
    width: SIZE,
    height: SIZE,
    show: true,
    transparent: true,
    frame: false,
    backgroundColor: '#00000000',
    webPreferences: { offscreen: false, backgroundThrottling: false },
  })

  const errs = []
  win.webContents.on('console-message', (_e, level, msg) => {
    if (level >= 2) errs.push(msg)
  })

  await win.loadFile(path.join(__dirname, '..', 'src', 'renderer', 'index.html'))
  await sleep(3500)

  const info = await win.webContents.executeJavaScript(`(() => {
    const cv = document.getElementById('cv')
    return { w: cv.width, h: cv.height, hint: document.getElementById('hint').textContent }
  })()`)
  console.log('canvas:', JSON.stringify(info))

  for (const p of points()) {
    await win.webContents.executeJavaScript(`(() => {
      const e = new MouseEvent('mousemove', { clientX: ${p.x}, clientY: ${p.y}, bubbles: true })
      window.dispatchEvent(e)
    })()`)
    await sleep(p.name === 'idle' ? 1800 : 700)
    const st = await win.webContents.executeJavaScript('window.__dbg()')
    console.log(String(p.name).padEnd(5), `cur=${st.cur.toFixed(4)} tgt=${st.targetU.toFixed(4)} idle=${st.idle} frame=${(st.cur * 241).toFixed(1)}`)
    const img = await win.webContents.capturePage()
    fs.writeFileSync(path.join(OUT, `${p.name}.png`), img.toPNG())
  }

  console.log('shots ->', OUT)
  if (errs.length) console.log('console errors:', errs.slice(0, 10))
  app.quit()
})
