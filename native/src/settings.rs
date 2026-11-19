//! 内置配置：档位、几何、帧参数。
//!
//! 这些值原来以常量形式散落在 main.rs 里，为了「同一种效果要兼容两套参数」时
//! 不堆常量，改成单一 JSON 真源（assets/settings.json）。代码里不再出现字面量。
//!
//! 解析失败直接 panic（fail fast）：桌宠缺一份合法配置属于构建事故，
//! 不该静默用兜底值跑出一个「看起来正常」的错误状态。

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub frames: Frames,
    pub geometry: Geometry,
    pub size: Choice,
    pub opacity: Choice,
    pub audio: Audio,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frames {
    /// 素材帧总数
    pub count: usize,
    /// 主序列起点：0..mainStart 只作极点 idle
    pub main_start: usize,
    /// 回 idle 时每帧步进
    pub idle_drift_frames: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    /// 画布四周透明留白
    pub pad: i32,
    /// 旋转基准在窗口高度上的比例（鼻子位置）
    pub focus_y: f64,
    /// 进入 idle 的半径
    pub idle_enter: f64,
    /// 退出 idle 的半径倍率（滞回）
    pub idle_exit_factor: f64,
    /// 命中测试的 alpha 阈值
    pub hit_alpha: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub options: Vec<i32>,
    pub min: i32,
    pub max: i32,
    #[serde(rename = "default")]
    pub default_value: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audio {
    pub default_pack: String,
    /// 循环停止时的默认淡出时长
    pub default_fade_out_ms: u32,
    /// 每个音频的默认实例数（快速连点不互相打断）
    pub default_instances: u32,
}

static S: OnceLock<Settings> = OnceLock::new();

pub fn get() -> &'static Settings {
    S.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/settings.json"))
            .unwrap_or_else(|e| panic!("assets/settings.json 解析失败：{e}"))
    })
}
