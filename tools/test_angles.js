/**
 * 角度数学验证：模拟鼠标绕屏幕中心一整圈，检查 u 到帧号的映射是否连续、无跳变。
 * 纯 Node，不依赖 Electron。
 */
const FC = 241
const W = 1920
const H = 1080
const cx = W / 2
const cy = H / 2

function toU(px, py) {
  const dx = px - cx
  const dy = py - cy
  if (Math.hypot(dx, dy) < 26) return null
  const theta = Math.atan2(dy, dx)
  return (theta + Math.PI) / (2 * Math.PI)
}

const STEPS = 720
const R = 400
const us = []
for (let i = 0; i < STEPS; i++) {
  const a = (i / STEPS) * Math.PI * 2
  const u = toU(cx + Math.cos(a) * R, cy + Math.sin(a) * R)
  us.push(u)
}

// 环形相邻差
function ringDelta(a, b) {
  let d = b - a
  while (d > 0.5) d -= 1
  while (d < -0.5) d += 1
  return d
}

let maxStep = 0
let maxAt = -1
for (let i = 0; i < us.length; i++) {
  const a = us[i]
  const b = us[(i + 1) % us.length]
  const d = Math.abs(ringDelta(a, b))
  if (d > maxStep) {
    maxStep = d
    maxAt = i
  }
}

const frameSteps = us.map((u) => u * FC)
let maxFrameStep = 0
for (let i = 0; i < frameSteps.length; i++) {
  const d = Math.abs(ringDelta(frameSteps[i] / FC, frameSteps[(i + 1) % frameSteps.length] / FC)) * FC
  if (d > maxFrameStep) maxFrameStep = d
}

console.log('samples          :', STEPS)
console.log('u range          :', Math.min(...us).toFixed(4), '->', Math.max(...us).toFixed(4))
console.log('max u step       :', maxStep.toFixed(6), 'at sample', maxAt)
console.log('max frame step   :', maxFrameStep.toFixed(3), 'frames')
console.log('expected step    :', (FC / STEPS).toFixed(3), 'frames/sample')
console.log('monotonic-ish?   : each quarter spans', (FC / 4).toFixed(1), 'frames')

// 死区检查
console.log('center deadzone  :', toU(cx + 10, cy + 10) === null ? 'OK (null inside 26px)' : 'FAIL')

// 同一方向重复采样应稳定（无噪声）
const repeat = []
for (let k = 0; k < 5; k++) repeat.push(toU(cx + 300, cy).toFixed(6))
console.log('repeat stability :', new Set(repeat).size === 1 ? 'OK' : 'FAIL ' + repeat.join(','))

const verdict = maxFrameStep < 1.0 ? 'PASS' : 'FAIL'
console.log('\nverdict          :', verdict, '(相邻采样跨帧 < 1 帧，不会出现跳帧)')
