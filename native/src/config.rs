//! 设置持久化：把外观与音效选择存到 %APPDATA%\\YuanBaoDeskPet\\config.json
//!
//! 原生版早期完全没有持久化：改完尺寸、透明度、音效，重启全部回到默认。
//! 这里只做三件事：读、写、把读到的值收敛到合法范围（手改坏的配置不该让桌宠崩）。
//! 写入用「临时文件 + rename」，避免断电/崩溃留下半截 JSON。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 猫的逻辑显示边长（120–360）
    pub size: i32,
    /// 不透明度（0.2–1.0）
    pub opacity: f64,
    /// 水平镜像
    pub mirror: bool,
    /// 音效组："meow"（圆宝叫声）/ "duck"（鸭子叫声）
    pub sound: String,
    /// 窗口左上角坐标；None 表示用默认的右下角
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            size: 160,
            opacity: 1.0,
            mirror: false,
            sound: "meow".to_string(),
            x: None,
            y: None,
        }
    }
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("YuanBaoDeskPet")
}

pub fn path() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    /// 收敛到合法范围：越界值直接夹紧，未知音效组回落默认
    pub fn sanitized(mut self) -> Self {
        self.size = self.size.clamp(120, 360);
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.2, 1.0)
        } else {
            1.0
        };
        if self.sound != "duck" {
            self.sound = "meow".to_string();
        }
        self
    }
}

/// 读配置。文件不存在、JSON 坏了、字段缺失一律回落默认值。
pub fn load() -> Config {
    let bytes = match std::fs::read(path()) {
        Ok(b) => b,
        Err(_) => return Config::default(),
    };
    serde_json::from_slice::<Config>(&bytes)
        .map(Config::sanitized)
        .unwrap_or_default()
}

/// 写配置。失败静默：桌宠不能因为写不了配置就崩掉或弹框。
pub fn save(cfg: &Config) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(json) = serde_json::to_vec_pretty(cfg) else {
        return;
    };
    let tmp = dir.join("config.json.tmp");
    if std::fs::write(&tmp, &json).is_ok() {
        let _ = std::fs::rename(&tmp, path());
    }
}
