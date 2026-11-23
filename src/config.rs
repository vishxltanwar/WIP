//! User preferences persisted to data\config.json

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub struct Config {
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default = "default_strength")]
    pub wallpaper_strength: f32,
    #[serde(default)]
    pub last_theme: String,
    #[serde(default)]
    pub reduced_motion: bool,
}

#[allow(dead_code)]
fn default_strength() -> f32 { 0.85 }

impl Default for Config {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            wallpaper_strength: 0.85,
            last_theme: String::new(),
            reduced_motion: false,
        }
    }
}

#[allow(dead_code)]
impl Config {
    pub fn load() -> Self {
        let path = crate::apply::data_dir().join("config.json");
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = crate::apply::data_dir().join("config.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let c = Config::default();
        assert!(!c.start_with_windows);
        assert_eq!(c.wallpaper_strength, 0.85);
    }

    #[test]
    fn roundtrip_serialization() {
        let c = Config {
            start_with_windows: true,
            wallpaper_strength: 0.5,
            last_theme: "relic".into(),
            reduced_motion: true,
        };
        let json = serde_json::to_string(&c).unwrap();
        let c2: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let json = r#"{"start_with_windows": true}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert!(c.start_with_windows);
        assert_eq!(c.wallpaper_strength, 0.85);
        assert!(c.last_theme.is_empty());
    }
}