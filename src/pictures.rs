//! Complete three-picture capture. Selection writes are explicit and journaled.
use crate::{
    device::{Keyboard, Snapshot},
    protocol::{Change, Lighting},
    transaction,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backup {
    pub format_version: u8,
    pub snapshot: Snapshot,
    pub pictures: Vec<Vec<u8>>,
}
impl Backup {
    pub fn validate(&self) -> Result<()> {
        self.snapshot.validate()?;
        ensure!(
            self.format_version == 1
                && self.pictures.len() == usize::from(self.snapshot.model()?.picture_count())
                && self.pictures.iter().all(|p| p.len() == 384),
            "Invalid picture backup dimensions for this model"
        );
        Ok(())
    }
    pub fn picture_draft(&self, current: &Snapshot, picture: u8) -> Result<Snapshot> {
        self.validate()?;
        current.validate()?;
        ensure!(
            current.device_id == self.snapshot.device_id,
            "Picture backup keyboard model differs"
        );
        ensure!(
            picture < self.snapshot.model()?.picture_count(),
            "Picture is out of range for this model"
        );
        ensure!(
            current.usb_revision == self.snapshot.usb_revision,
            "Picture backup firmware revision differs"
        );
        ensure!(
            current.lighting.mode == 13 && transaction::picture(&current.lighting) == picture,
            "Select this picture on the keyboard and reload before restoring its colours"
        );
        let mut draft = current.clone();
        draft.onboard.as_mut().context("RGB unavailable")?.rgb =
            self.pictures[usize::from(picture)].clone();
        Ok(draft)
    }
}
pub fn capture(keyboard: &Keyboard, destination: &Path, journal: &Path) -> Result<Backup> {
    ensure!(!destination.exists(), "Backup file already exists");
    let snapshot = keyboard.snapshot()?;
    // Refuse before selection changes if this implementation could not send
    // the original lighting tuple back to the device.
    Change::Lighting {
        settings: snapshot.lighting.clone(),
    }
    .packets()?;
    transaction::save_new(journal, &snapshot)?;
    let original = snapshot.lighting.clone();
    let capture = (|| -> Result<Vec<Vec<u8>>> {
        let mut pictures = Vec::new();
        for picture in 0..keyboard.model().picture_count() {
            let settings = Lighting {
                mode: 13,
                wire_speed: original.wire_speed,
                brightness: original.brightness,
                flags: picture << 4,
                rgb: [0, 200, 200],
            };
            keyboard.apply(&Change::Lighting { settings })?;
            pictures.push(keyboard.rgb()?);
        }
        Ok(pictures)
    })();
    // Always attempt to restore selection/effect, even if one read failed.
    let restored = keyboard.apply(&Change::Lighting { settings: original });
    restored.with_context(|| {
        format!(
            "Could not restore lighting after capture; recovery snapshot: {}",
            journal.display()
        )
    })?;
    let backup = Backup {
        format_version: 1,
        snapshot,
        pictures: capture?,
    };
    backup.validate()?;
    transaction::save_new(destination, &backup)?;
    Ok(backup)
}
