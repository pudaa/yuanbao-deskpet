/**
 * V2 角度索引验证：证明鼠标方向与帧号的对应关系。
 * 不依赖 Electron/Tauri，纯数学。
 */
const FC = 361
const DEG_OFFSET = -90

function uFrom(dx, dy) {
  const theta = Math.atan2(dy, dx) // 屏幕坐标，y 向下，顺时针为正
  return (theta + Math.PI) / (2 * Math.PI)
}

function frameFrom(u) {
  const deg = (((u * 360 + DEG_OFFSET) % 360) + 360) % 360
  return { deg, frame: deg }
}

const R = 120
const cases = [
  ['正上', 0, -R],
  ['右上', R * 0.707, -R * 0.707],
  ['正右', R, 0],
  ['右下', R * 0.707, R * 0.707],
  ['正下', 0, R],
  ['左下', -R * 0.707, R * 0.707],
  ['正左', -R, 0],
  ['左上', -R * 0.707, -R * 0.707],
]

const expect = {
  正上: 0,
  右上: 45,
  正右: 90,
  右下: 135,
  正下: 180,
  左下: 225,
  正左: 270,
  左上: 315,
}

let pass = true
console.log('方向    鼠标(dx,dy)        角度deg   帧号     期望   结果')
for (const [name, dx, dy] of cases) {
  const u = uFrom(dx, dy)
  const { deg, frame } = frameFrom(u)
  const f = Math.round(frame) % FC
  const e = expect[name]
  const ok = Math.abs(f - e) <= 1
  if (!ok) pass = false
  console.log(
    `${name}  (${dx.toFixed(0).padStart(4)},${dy.toFixed(0).padStart(4)})   ${deg
      .toFixed(1)
      .padStart(6)}   ${f.toString().padStart(4)}   ${e.toString().padStart(4)}   ${ok ? 'OK' : 'FAIL'}`
  )
}

// 连续性：绕猫一圈 720 采样，帧号环形步长应恒定且无跳变
// 注意：deg 的周期是 360（不是帧数 361），帧 360 与帧 0 同姿态
function ringDelta(a, b) {
  let d = b - a
  while (d > 180) d -= 360
  while (d < -180) d += 360
  return d
}

const STEPS = 720
let maxStep = 0
for (let i = 0; i < STEPS; i++) {
  const a = (i / STEPS) * Math.PI * 2
  const b = ((i + 1) / STEPS) * Math.PI * 2
  const f1 = frameFrom(uFrom(Math.cos(a) * R, Math.sin(a) * R)).frame
  const f2 = frameFrom(uFrom(Math.cos(b) * R, Math.sin(b) * R)).frame
  const d = Math.abs(ringDelta(f1, f2))
  if (d > maxStep) maxStep = d
}
const expectedStep = FC / STEPS
console.log(`\n绕一圈采样 ${STEPS} 次：最大跨帧 ${maxStep.toFixed(3)}（理论 ${expectedStep.toFixed(3)}）`)
const smooth = maxStep <= expectedStep * 1.5

// 闭环：帧 360 应与帧 0 同姿态（由帧数 361 保证索引不越界）
const wrapOk = frameFrom(uFrom(0, -R)).frame === 0

console.log(`闭环接缝：帧 360 -> 0  ${wrapOk ? 'OK' : 'FAIL'}`)
console.log(`\n结论：${pass && smooth && wrapOk ? 'PASS' : 'FAIL'}`)
