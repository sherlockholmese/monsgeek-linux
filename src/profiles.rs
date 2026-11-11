//! Named local configurations. Loading a profile only creates a draft.
use crate::{device::Snapshot, transaction};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Selection {
    pub keys: bool,
    pub macros: bool,
    pub lighting: bool,
    pub performance: bool,
    pub options: bool,
    pub active_layer: bool,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            keys: true,
            macros: true,
            lighting: true,
            performance: true,
            options: true,
            active_layer: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub format_version: u8,
    pub name: String,
    pub snapshot: Snapshot,
}
impl Profile {
    pub fn new(name: String, snapshot: Snapshot) -> Result<Self> {
        snapshot.validate()?;
        let name = name.trim().to_owned();
        ensure!(
            !name.is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control),
            "Use a profile name of 1–80 printable characters"
        );
        Ok(Self {
            format_version: 1,
            name,
            snapshot,
        })
    }
    pub fn draft(&self, current: &Snapshot, selected: Selection) -> Result<Snapshot> {
        ensure!(self.format_version == 1, "Unsupported profile version");
        Self::new(self.name.clone(), self.snapshot.clone())?;
        current.validate()?;
        ensure!(
            current.usb_revision == self.snapshot.usb_revision,
            "Profile firmware revision differs from keyboard"
        );
        let mut draft = current.clone();
        if selected.keys {
            draft.keymaps = self.snapshot.keymaps.clone();
            draft.fn_keymaps = self.snapshot.fn_keymaps.clone();
        }
        if selected.active_layer {
            draft.active_layer = self.snapshot.active_layer;
        }
        if selected.lighting {
            // RGB readback belongs only to the selected picture. Never mix it
            // with another picture's selection in one transaction.
            ensure!(
                transaction::picture(&current.lighting)
                    == transaction::picture(&self.snapshot.lighting),
                "Select the profile's RGB picture on the keyboard and reload before loading its lighting"
            );
            draft.lighting = self.snapshot.lighting.clone();
        }
        if let (Some(to), Some(from)) = (&mut draft.onboard, &self.snapshot.onboard) {
            if selected.macros {
                to.macros = from.macros.clone();
            }
            if selected.lighting {
                to.rgb = from.rgb.clone();
            }
            if selected.performance {
                to.debounce = from.debounce;
                to.sleep = from.sleep.clone();
            }
            if selected.options {
                to.options = from.options;
                to.auto_os = from.auto_os;
            }
        }
        let changes = current.restore_changes(&draft)?;
        transaction::check_picture(&changes, current)?;
        Ok(draft)
    }
}

pub fn directory() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("HOME is not set")?;
    Ok(base.join("monsgeek-linux/profiles"))
}
pub fn save(dir: &Path, profile: &Profile) -> Result<PathBuf> {
    Profile::new(profile.name.clone(), profile.snapshot.clone())?;
    fs::create_dir_all(dir)?;
    // User names never become file paths; create_new also prevents collisions.
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = dir.join(format!("profile-{stamp}-{}.json", std::process::id()));
    transaction::save_new(&path, profile)?;
    Ok(path)
}
pub fn load(path: &Path) -> Result<Profile> {
    let value: Profile = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(value.format_version == 1, "Unsupported profile version");
    Profile::new(value.name, value.snapshot)
}
pub fn list(dir: &Path) -> Result<Vec<(PathBuf, Profile)>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            profiles.push((
                path.clone(),
                load(&path).with_context(|| format!("Cannot load profile {}", path.display()))?,
            ));
        }
    }
    profiles.sort_by_key(|a| a.1.name.to_lowercase());
    Ok(profiles)
}
