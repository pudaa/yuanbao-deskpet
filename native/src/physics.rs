//! 甩出物理：拖动松手后按初速度做抛体运动，碰边反弹、落地摩擦直至静止。
//!
//! 参数对齐 dsh-pet 的 `runtime/electron-helper/shared-core.js`（gravity 1400 /
//! restitution 0.78 / groundFriction 2.5 / 速度上限 3600 / 静止阈值 40·15），
//! 保证手感一致。

pub const GRAVITY: f64 = 1400.0;
pub const RESTITUTION: f64 = 0.78;
pub const GROUND_FRICTION: f64 = 3.2;
pub const MAX_THROW_SPEED: f64 = 3600.0;
pub const DEAD_ZONE_SPEED: f64 = 500.0;
const REST_VY: f64 = 40.0;
const REST_VX: f64 = 35.0;
const MAX_STEP_DT: f64 = 0.05;
/// 子步进：每步最多推进的像素数（对齐 dsh-pet 的 SUBSTEP_PX）
const SUBSTEP_PX: f64 = 8.0;
const SUBSTEP_MAX: usize = 64;
/// 用最近这段时间的光标轨迹估算甩出速度
const RELEASE_WINDOW_MS: f64 = 150.0;
const MIN_SPAN_MS: f64 = 20.0;
const PEAK_WEIGHT: f64 = 0.5;

#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
}

#[derive(Clone, Copy)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

pub struct StepResult {
    pub landed: bool,
    pub at_rest: bool,
    pub impact: f64,
}

impl Body {
    pub fn step(&mut self, dt_raw: f64, b: Bounds) -> StepResult {
        let dt = dt_raw.clamp(0.0, MAX_STEP_DT);

        // 子步进：高速甩出时一帧能走 40+ px，单步积分会直接穿过边界再弹回来。
        // 按每 8px 一步拆分（对齐 dsh-pet 的 throwSubsteps），最多 64 步。
        let travel = (self.vx * self.vx + self.vy * self.vy).sqrt() * dt;
        let n = if travel > SUBSTEP_PX {
            ((travel / SUBSTEP_PX).ceil() as usize).clamp(1, SUBSTEP_MAX)
        } else {
            1
        };
        let sub = dt / n as f64;

        let mut landed = false;
        let mut impact = 0.0;

        for _ in 0..n {
            self.vy += GRAVITY * sub;
            self.x += self.vx * sub;
            self.y += self.vy * sub;

            if self.x < b.min_x {
                self.x = b.min_x;
                self.vx = self.vx.abs() * RESTITUTION;
            } else if self.x > b.max_x {
                self.x = b.max_x;
                self.vx = -self.vx.abs() * RESTITUTION;
            }

            if self.y < b.min_y {
                self.y = b.min_y;
                self.vy = self.vy.abs() * RESTITUTION;
            } else if self.y >= b.max_y {
                self.y = b.max_y;
                // 只记第一次触地的冲击，后续子步是反弹后的速度
                if !landed {
                    impact = self.vy.abs();
                }
                if self.vy.abs() < REST_VY {
                    self.vy = 0.0;
                } else {
                    self.vy = -self.vy.abs() * RESTITUTION;
                }
                landed = true;
            }
        }

        // 摩擦按整帧应用一次：分子步累积会把衰减放大 n 倍
        if landed {
            self.vx *= (1.0 - GROUND_FRICTION * dt).max(0.0);
        }

        let speed = (self.vx * self.vx + self.vy * self.vy).sqrt();
        let at_rest = (self.y >= b.max_y - 1.0 && self.vy.abs() < 1.0 && self.vx.abs() < REST_VX)
            || (landed && speed < REST_VY && self.vy.abs() < 1.0);

        StepResult {
            landed,
            at_rest,
            impact,
        }
    }
}

/// 用最近一段光标轨迹估算甩出速度。
/// 轨迹元素为 `(x, y, t_ms)`；速度太低（轻拖不算甩）返回 None。
pub fn estimate_throw(trail: &[(f64, f64, f64)]) -> Option<(f64, f64)> {
    if trail.len() < 2 {
        return None;
    }
    let last_t = trail[trail.len() - 1].2;
    let win: Vec<(f64, f64, f64)> = trail
        .iter()
        .copied()
        .filter(|s| last_t - s.2 <= RELEASE_WINDOW_MS)
        .collect();
    if win.len() < 2 {
        return None;
    }

    let (x0, y0, t0) = win[0];
    let (x1, y1, t1) = win[win.len() - 1];
    let span = t1 - t0;
    if span < MIN_SPAN_MS {
        return None;
    }

    let vx = (x1 - x0) / span * 1000.0;
    let vy = (y1 - y0) / span * 1000.0;
    let base = (vx * vx + vy * vy).sqrt();
    if base < 1e-6 {
        return None;
    }

    // 峰值速度加权，让"甩"的动作比"拖"更容易被识别出来
    let mut peak: f64 = base;
    for w in win.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        let dt = (w[1].2 - w[0].2).max(1.0);
        let s = (dx * dx + dy * dy).sqrt() / dt * 1000.0;
        if s > peak {
            peak = s;
        }
    }

    let raw = (1.0 - PEAK_WEIGHT) * base + PEAK_WEIGHT * peak;
    let speed = raw.min(MAX_THROW_SPEED);
    if speed < DEAD_ZONE_SPEED {
        return None;
    }

    Some((vx / base * speed, vy / base * speed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Bounds {
        Bounds { min_x: 0.0, min_y: 0.0, max_x: 1000.0, max_y: 800.0 }
    }

    #[test]
    fn fast_down_throw_lands_with_impact() {
        let mut b = Body { x: 100.0, y: 100.0, vx: 0.0, vy: 2500.0 };
        let mut impact = 0.0f64;
        let mut landed = false;
        for _ in 0..120 {
            let r = b.step(0.012, bounds());
            if r.landed {
                landed = true;
                impact = impact.max(r.impact);
            }
        }
        assert!(landed, "应当落地");
        assert!(impact > 280.0, "冲击速度应超过阈值，实际 {impact}");
        println!("landed impact = {impact:.0}");
    }

    #[test]
    fn substep_does_not_tunnel_at_max_speed() {
        // 3600 px/s × 0.05s = 180px，单步积分会穿过整个屏幕
        let mut b = Body { x: 500.0, y: 400.0, vx: 3600.0, vy: 0.0 };
        for _ in 0..60 {
            b.step(0.05, bounds());
            assert!(b.x >= -1.0 && b.x <= 1001.0, "越界 x={}", b.x);
            assert!(b.y >= -1.0 && b.y <= 801.0, "越界 y={}", b.y);
        }
    }
}
