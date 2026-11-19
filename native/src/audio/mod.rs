//! 音效行为引擎。
//!
//! 数据模型：**音效组 = 一组行为**，行为 = 触发（on）× 播放（play）× 条件（when）。
//! 参数全部来自 assets/sounds.json 与 assets/settings.json，代码里没有阈值常量。
//!
//! 为什么不用「按下/长按/松开三个槽位 + 开关」：
//! 槽位模型下，「长按 600ms 循环呼噜」和「按下即循环」需要不同字段、
//! 松手是否补一声又需要布尔开关，新增一种时间行为就要加字段、加分支。
//! 改成行为列表后，同一组可以有两条 press 行为（600ms 一条、1200ms 一条），
//! 互不干扰，也不需要任何开关。

mod mci;

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::settings;

// build.rs 生成：音频 key -> RCDATA id 与字节数（素材缺失时该 key 不出现）
include!(concat!(env!("OUT_DIR"), "/sounds_map.rs"));

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[allow(dead_code)]
    pub version: u32,
    pub packs: Vec<Pack>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub id: String,
    pub label: String,
    pub behaviors: Vec<Behavior>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Behavior {
    pub on: Trigger,
    /// 对应 web/sounds/<sound>.wav 的文件名（不含扩展名）
    pub sound: String,
    pub play: Play,
    /// 条件：不满足则本次不触发（目前只在 release 上求值）
    #[serde(default)]
    pub when: Option<Condition>,
    #[serde(default = "full_volume")]
    pub volume: f32,
    /// 该音频的 MCI 实例数；缺省用 settings.audio.defaultInstances
    #[serde(default)]
    pub instances: Option<u32>,
}

fn full_volume() -> f32 {
    1.0
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Trigger {
    pub event: Event,
    /// 按下/松手后延迟多久触发；期间松手会取消
    #[serde(default)]
    pub delay_ms: u64,
    /// true = 按下即开始（不判断按多久），配合 loop 使用
    #[serde(default)]
    pub while_held: bool,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Press,
    Release,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Play {
    pub mode: Mode,
    #[serde(default)]
    pub until: Until,
    /// 停止时淡出时长；缺省用 settings.audio.defaultFadeOutMs
    #[serde(default)]
    pub fade_out_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Once,
    Loop,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Until {
    /// 松手即停（可淡出）
    #[default]
    Release,
    /// 一直循环，直到下次按下或切换音效组
    Forever,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// 按住时长小于该值才触发（毫秒）
    #[serde(default)]
    pub held_ms_lt: Option<u64>,
    /// 按住时长不小于该值才触发（毫秒）
    #[serde(default)]
    pub held_ms_gte: Option<u64>,
    /// 拖动距离小于该值才算「没动」（像素）
    #[serde(default)]
    pub moved_lt: Option<i32>,
}

fn manifest() -> &'static Manifest {
    static M: OnceLock<Manifest> = OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/sounds.json"))
            .unwrap_or_else(|e| panic!("assets/sounds.json 解析失败：{e}"))
    })
}

struct Pool {
    aliases: Vec<String>,
}

struct Active {
    alias: String,
    looped: bool,
    until: Until,
    fade_ms: u32,
    base_volume: f32,
    /// (淡出起点, 起始音量)
    fade: Option<(Instant, f32)>,
}

struct Pending {
    at: Instant,
    pack: usize,
    behavior: usize,
}

struct Engine {
    pack: usize,
    pools: HashMap<&'static str, Pool>,
    next_alias: usize,
    next_voice: usize,
    pressed_at: Option<Instant>,
    pending: Vec<Pending>,
    active: Vec<Active>,
}

impl Engine {
    fn new() -> Self {
        let default_id = settings::get().audio.default_pack.as_str();
        let pack = manifest()
            .packs
            .iter()
            .position(|p| p.id == default_id)
            .unwrap_or(0);
        Self {
            pack,
            pools: HashMap::new(),
            next_alias: 0,
            next_voice: 0,
            pressed_at: None,
            pending: Vec::new(),
            active: Vec::new(),
        }
    }

    fn pack_def(&self) -> &'static Pack {
        &manifest().packs[self.pack]
    }

    /// 首次使用时把资源落成临时文件并开 MCI 实例
    fn ensure_pool(&mut self, res: SoundRes, want: usize) -> bool {
        if self.pools.contains_key(res.key) {
            return true;
        }
        let Some(path) = materialize(&res) else {
            return false;
        };
        let mut aliases = Vec::new();
        for _ in 0..want.max(1) {
            let alias = format!("yb{}", self.next_alias);
            self.next_alias += 1;
            if mci::open(&alias, &path) {
                mci::set_volume(&alias, 1.0);
                aliases.push(alias);
            }
        }
        if aliases.is_empty() {
            return false;
        }
        self.pools.insert(res.key, Pool { aliases });
        true
    }

    fn start(&mut self, pack_idx: usize, bi: usize) {
        let b = &manifest().packs[pack_idx].behaviors[bi];
        let Some(res) = SOUND_RESOURCES.iter().find(|r| r.key == b.sound.as_str()).copied() else {
            // 素材缺失：该行为静默，不阻断其它行为
            return;
        };
        let want = b
            .instances
            .unwrap_or(settings::get().audio.default_instances) as usize;
        if !self.ensure_pool(res, want) {
            return;
        }
        let pool = self.pools.get(res.key).expect("pool just ensured");
        let idx = self.next_voice % pool.aliases.len();
        self.next_voice = self.next_voice.wrapping_add(1);
        let alias = pool.aliases[idx].clone();

        mci::set_volume(&alias, b.volume);
        mci::play(&alias);

        // whileHeld = 按下即开始、松手才停，等价于「loop until release」；
        // 与 mode 同时使用时以 whileHeld 为准（它描述的是生命周期，不是播放次数）
        let looped = b.play.mode == Mode::Loop || b.on.while_held;
        if looped {
            self.active.push(Active {
                alias,
                looped: true,
                until: b.play.until,
                fade_ms: b
                    .play
                    .fade_out_ms
                    .unwrap_or(settings::get().audio.default_fade_out_ms),
                base_volume: b.volume,
                fade: None,
            });
        }
    }

    fn stop_all(&mut self) {
        for a in &self.active {
            mci::stop(&a.alias);
        }
        self.active.clear();
    }

    fn press(&mut self) {
        let now = Instant::now();
        self.pressed_at = Some(now);
        self.pending.clear();
        self.stop_all();

        let pack = self.pack;
        let n = manifest().packs[pack].behaviors.len();
        for bi in 0..n {
            let b = &manifest().packs[pack].behaviors[bi];
            if b.on.event != Event::Press {
                continue;
            }
            if b.on.delay_ms > 0 {
                self.pending.push(Pending {
                    at: now + Duration::from_millis(b.on.delay_ms),
                    pack,
                    behavior: bi,
                });
            } else {
                self.start(pack, bi);
            }
        }
    }

    fn release(&mut self, drag_px: i32) {
        let now = Instant::now();
        let held = self
            .pressed_at
            .map(|t| now.duration_since(t))
            .unwrap_or_default();
        self.pressed_at = None;
        self.pending.clear();

        for a in self.active.iter_mut() {
            if a.until == Until::Release && a.fade.is_none() {
                a.fade = Some((now, a.base_volume));
            }
        }

        let pack = self.pack;
        let n = manifest().packs[pack].behaviors.len();
        for bi in 0..n {
            let b = &manifest().packs[pack].behaviors[bi];
            if b.on.event != Event::Release {
                continue;
            }
            if !condition_ok(&b.when, held, drag_px) {
                continue;
            }
            self.start(pack, bi);
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();

        let due: Vec<Pending> = {
            let mut ready = Vec::new();
            let mut i = 0;
            while i < self.pending.len() {
                if self.pending[i].at <= now {
                    ready.push(self.pending.remove(i));
                } else {
                    i += 1;
                }
            }
            ready
        };
        for p in due {
            // 松手会清空 pending，到这里说明仍按着
            if self.pressed_at.is_some() {
                self.start(p.pack, p.behavior);
            }
        }

        let mut finished: Vec<usize> = Vec::new();
        for i in 0..self.active.len() {
            let a = &mut self.active[i];
            match a.fade {
                Some((t0, from)) => {
                    let p = if a.fade_ms == 0 {
                        1.0
                    } else {
                        now.duration_since(t0).as_secs_f32() * 1000.0 / a.fade_ms as f32
                    };
                    if p >= 1.0 {
                        mci::stop(&a.alias);
                        finished.push(i);
                    } else {
                        mci::set_volume(&a.alias, from * (1.0 - p));
                    }
                }
                None if a.looped => {
                    // 循环靠「播完再补一次」维持：MCI 的 waveaudio 不支持 repeat
                    if mci::mode(&a.alias) != "playing" {
                        mci::play(&a.alias);
                    }
                }
                None => {}
            }
        }
        for i in finished.into_iter().rev() {
            self.active.remove(i);
        }
    }
}

fn condition_ok(c: &Option<Condition>, held: Duration, drag_px: i32) -> bool {
    let Some(c) = c else { return true };
    let held_ms = held.as_millis() as u64;
    if let Some(lt) = c.held_ms_lt {
        if held_ms >= lt {
            return false;
        }
    }
    if let Some(gte) = c.held_ms_gte {
        if held_ms < gte {
            return false;
        }
    }
    if let Some(lt) = c.moved_lt {
        if drag_px >= lt {
            return false;
        }
    }
    true
}

/// 把内嵌资源落成 %TEMP% 下的 wav；长度不符（换了版本）就重写
fn materialize(res: &SoundRes) -> Option<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("yuanbao-deskpet-sounds");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.wav", res.key));
    let bytes = unsafe { crate::render::frames::resource(res.id) }?;
    let stale = std::fs::metadata(&path)
        .map(|m| m.len() as usize != bytes.len())
        .unwrap_or(true);
    if stale {
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

fn engine() -> &'static Mutex<Engine> {
    static E: OnceLock<Mutex<Engine>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(Engine::new()))
}

fn with<R>(f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    engine().lock().ok().map(|mut e| f(&mut e))
}

/// 按下（由 UI 线程调用）
pub fn press() {
    with(|e| e.press());
}

/// 松手；drag_px = 按下点到松手点的最大位移
pub fn release(drag_px: i32) {
    with(|e| e.release(drag_px));
}

/// 每帧驱动：延迟触发、循环续播、淡出
pub fn tick() {
    with(|e| e.tick());
}

pub fn packs() -> &'static [Pack] {
    &manifest().packs
}

pub fn current_pack_id() -> &'static str {
    with(|e| e.pack_def().id.as_str()).unwrap_or("")
}

/// 切换音效组；返回是否成功
pub fn set_pack(id: &str) -> bool {
    let Some(idx) = manifest().packs.iter().position(|p| p.id == id) else {
        return false;
    };
    with(|e| {
        if e.pack == idx {
            return true;
        }
        e.stop_all();
        e.pending.clear();
        e.pressed_at = None;
        e.pack = idx;
        true
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_and_ids_unique() {
        let m = manifest();
        assert!(!m.packs.is_empty());
        let mut ids: Vec<&str> = m.packs.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "音效组 id 重复");
    }

    #[test]
    fn default_pack_exists() {
        let id = settings::get().audio.default_pack.as_str();
        assert!(manifest().packs.iter().any(|p| p.id == id));
    }

    #[test]
    fn condition_semantics() {
        let c = Some(Condition {
            held_ms_lt: Some(600),
            ..Default::default()
        });
        assert!(condition_ok(&c, Duration::from_millis(100), 0));
        assert!(!condition_ok(&c, Duration::from_millis(600), 0));

        let c = Some(Condition {
            moved_lt: Some(6),
            ..Default::default()
        });
        assert!(condition_ok(&c, Duration::from_millis(10), 5));
        assert!(!condition_ok(&c, Duration::from_millis(10), 6));

        assert!(condition_ok(&None, Duration::from_secs(10), 100));
    }

    #[test]
    fn meow_has_delayed_loop_duck_has_release() {
        let m = manifest();
        let meow = m.packs.iter().find(|p| p.id == "meow").expect("meow");
        let delayed_loop = meow.behaviors.iter().any(|b| {
            b.on.event == Event::Press
                && b.on.delay_ms > 0
                && b.play.mode == Mode::Loop
                && b.play.until == Until::Release
        });
        assert!(delayed_loop, "圆宝组应有「按下延时后循环」的行为");

        let duck = m.packs.iter().find(|p| p.id == "duck").expect("duck");
        assert!(duck.behaviors.iter().any(|b| b.on.event == Event::Release));
        assert!(!duck.behaviors.iter().any(|b| b.on.delay_ms > 0));
    }
}
