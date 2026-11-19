// Phase A 拆分：从 main.rs 原样搬出，逻辑未改。
use crate::app::state::*;

#[derive(Clone, Copy)]
pub(crate) enum Press {
    Idle,
    Down(f64),   // 按下，0→0.07s
    Bounce(f64), // 松手回弹，0→0.34s
    Land(f64, f64), // 落地挤压：(进度 0→0.22s, 压扁深度 0.2~0.45)
}

pub(crate) fn bezier_component(t: f64, p1: f64, p2: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

pub(crate) fn bezier_derivative(t: f64, p1: f64, p2: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// 精确求 CSS cubic-bezier(x1,y1,x2,y2) 在给定时间进度 x 上的输出。
/// CSS 的 timing-function 是先按 x 反解参数 t，再取 y，不能拿 ease-out 之类的近似糊弄。
pub(crate) fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let mut t = x;
    for _ in 0..8 {
        let err = bezier_component(t, x1, x2) - x;
        if err.abs() < 1e-6 {
            break;
        }
        let d = bezier_derivative(t, x1, x2);
        if d.abs() < 1e-6 {
            break;
        }
        t = (t - err / d).clamp(0.0, 1.0);
    }
    bezier_component(t, y1, y2)
}

/// CSS `ease-out` = cubic-bezier(0, 0, .58, 1) —— 用于 squeeze
pub(crate) fn css_ease_out(k: f64) -> f64 {
    cubic_bezier(0.0, 0.0, 0.58, 1.0, k)
}

/// Web 版 bounce / squash 的 cubic-bezier(.3, .9, .4, 1)
/// 前段冲得快、尾巴拖得长，所以观感「慢而丝滑」
pub(crate) fn css_bounce(k: f64) -> f64 {
    cubic_bezier(0.3, 0.9, 0.4, 1.0, k)
}

/// 按 Web 版 CSS 关键帧插值；每个区间都套用 animation-timing-function
pub(crate) fn keys(t: f64, pts: &[(f64, f64, f64)], ease: fn(f64) -> f64) -> (f64, f64) {
    for w in pts.windows(2) {
        if t <= w[1].0 {
            let k = ((t - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0);
            let e = ease(k);
            return (
                w[0].1 + (w[1].1 - w[0].1) * e,
                w[0].2 + (w[1].2 - w[0].2) * e,
            );
        }
    }
    let last = pts[pts.len() - 1];
    (last.1, last.2)
}

/// 落地挤压深度随冲击速度变化（对齐 dsh-pet 的 landingSquash）：
/// 轻落压 20%，猛砸压到 45%；再叠加 ease-out-back 的过冲回弹，最多拉伸到 1.12。
pub(crate) fn impact_depth(impact: f64) -> f64 {
    let t = ((impact.abs() - 300.0) / (1500.0 - 300.0)).clamp(0.0, 1.0);
    let squash = (1.0 - 0.45 * t).min(0.8);
    1.0 - squash
}

pub(crate) fn land_scale(u: f64, depth: f64) -> (f64, f64) {
    let d = depth.clamp(0.0, 0.45);
    let sy = if u < 0.45 {
        // 前 45%：二次加速压到最扁
        let p = (u / 0.45).clamp(0.0, 1.0);
        1.0 - d * p * p
    } else {
        // 后 55%：ease-out-back 回到 1，允许过冲
        let p = ((u - 0.45) / 0.55).clamp(0.0, 1.0);
        let c1 = 1.70158;
        let c3 = c1 + 1.0;
        let f = 1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2);
        ((1.0 - d) + d * f.max(0.0)).min(1.12)
    };
    // 纵向压扁时横向补偿变宽，体积感更自然
    (1.0 + (1.0 - sy) * 0.85, sy)
}

/// 返回 (scaleX, scaleY)，完全对应 web/pet/style.css 的 squeeze / bounce / squash
pub(crate) fn press_scale(p: Press) -> (f64, f64) {
    match p {
        Press::Idle => (1.0, 1.0),
        Press::Down(t) => {
            let e = css_ease_out(t / 0.07);
            (1.0 + 0.17 * e, 1.0 - 0.24 * e) // → (1.17, 0.76)
        }
        Press::Bounce(t) => keys(
            t / 0.34,
            &[
                (0.0, 1.17, 0.76),
                (0.45, 0.96, 1.06),
                (0.75, 1.02, 0.99),
                (1.0, 1.0, 1.0),
            ],
            css_bounce,
        ),
        Press::Land(t, d) => land_scale(t / 0.22, d),
    }
}

impl State {
    pub(crate) fn tick_press(&mut self, dt: f64) {
        match self.press {
            Press::Down(t) => self.press = Press::Down((t + dt).min(0.07)),
            Press::Bounce(t) => {
                let n = t + dt;
                self.press = if n >= 0.34 { Press::Idle } else { Press::Bounce(n) };
            }
            Press::Land(t, d) => {
                let n = t + dt;
                self.press = if n >= 0.22 { Press::Idle } else { Press::Land(n, d) };
            }
            Press::Idle => {}
        }
    }
}

// ---- 开机自启（HKCU\...\Run，不需要管理员）----
