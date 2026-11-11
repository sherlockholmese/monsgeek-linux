//! Interface preferences, kept apart from keyboard state.
use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default)]
    pub theme: Option<ThemeChoice>,
    pub battery_alerts: bool,
    pub battery_threshold: u8,
    pub tray: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: None,
            battery_alerts: true,
            battery_threshold: 15,
            tray: true,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("monsgeek-linux").join("gui.json"))
}

/// Missing or unreadable settings fall back to defaults.
pub fn load() -> Settings {
    let mut settings: Settings = path()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    settings.battery_threshold = settings.battery_threshold.clamp(5, 50);
    settings
}

/// Write atomically so a crash never leaves a truncated file.
pub fn save(settings: &Settings) -> anyhow::Result<()> {
    let path = path().ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_round_trips() {
        let s = Settings {
            theme: Some(ThemeChoice::Dark),
            ..Default::default()
        };
        let text = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.theme, Some(ThemeChoice::Dark));
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap().theme, None);
    }
}
