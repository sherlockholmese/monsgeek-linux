//! Write transactions shared by the CLI and the GUI: before-state capture,
//! undo generation and ordered, verified application.
use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    device::{Keyboard, Snapshot},
    protocol::{Change, Lighting},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeforeState {
    pub snapshot: Snapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<Change>,
    #[serde(default)]
    pub undo_changes: Vec<Change>,
}

impl BeforeState {
    /// Ordered changes that return the device to `snapshot`.
    pub fn changes(&self) -> Vec<Change> {
        let mut changes = self.undo_changes.clone();
        if let Some(c) = &self.undo {
            changes.push(c.clone());
        }
        changes
    }
}

/// Create an owner-only file that must not already exist.
pub fn save_new(path: &Path, value: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("cannot create {} (must not already exist)", path.display()))?;
    file.write_all(&data)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

/// Selected custom picture, or 0 for every other effect.
pub fn picture(l: &Lighting) -> u8 {
    if l.mode == 13 { l.flags >> 4 } else { 0 }
}

/// GET_USERPIC follows picture selection, even though its selector is zero.
/// A single read-only snapshot cannot back up an inactive picture. Refuse a
/// mixed transaction whose inverse would write one picture's bytes to another.
pub fn check_picture(changes: &[Change], current: &Snapshot) -> Result<()> {
    if changes.iter().any(|c| matches!(c, Change::Rgb { .. })) {
        ensure!(
            changes.iter().all(|c| match c {
                Change::Lighting { settings } => picture(settings) == picture(&current.lighting),
                _ => true,
            }),
            "RGB and picture-selection changes need separate transactions: select the desired picture, capture a new snapshot, then edit or restore its RGB"
        );
    }
    Ok(())
}

/// Check that a saved undo record can be applied to the current device.
pub fn check_undo(record: &BeforeState, current: &Snapshot) -> Result<()> {
    ensure!(
        current.usb_revision == record.snapshot.usb_revision,
        "undo firmware revision mismatch"
    );
    ensure!(
        !record
            .changes()
            .iter()
            .any(|c| matches!(c, Change::Rgb { .. }))
            || picture(&current.lighting) == picture(&record.snapshot.lighting),
        "undo RGB picture differs; select the original picture before undo"
    );
    Ok(())
}

/// Save the before-state to `before`, then send each change and verify its
/// read-back, stopping at the first mismatch. `snapshot` must be fresh.
pub fn commit(
    keyboard: &Keyboard,
    snapshot: Snapshot,
    changes: &[Change],
    before: &Path,
) -> Result<BeforeState> {
    check_picture(changes, &snapshot)?;
    let undo_changes = changes
        .iter()
        .rev()
        .map(|c| snapshot.inverse(c))
        .collect::<Result<Vec<_>>>()?;
    // Refuse a write if its saved inverse could not be sent by this implementation.
    for undo in &undo_changes {
        undo.packets()?;
    }
    let record = BeforeState {
        snapshot,
        undo: None,
        undo_changes,
    };
    save_new(before, &record)?;
    for (i, c) in changes.iter().enumerate() {
        keyboard.apply(c).with_context(|| {
            format!(
                "change {} of {} failed; state may be partial; retain {}",
                i + 1,
                changes.len(),
                before.display()
            )
        })?;
    }
    Ok(record)
}

/// Write `draft` over `baseline` as one verified transaction.
///
/// Re-reads only what the changes touch (a full read takes about 30 s over
/// 2.4 GHz) and refuses when that differs from `baseline`, so edits never
/// overwrite changes made elsewhere. Saves the before-state to `before`,
/// commits every change with read-back, and returns the refreshed state.
pub fn apply_draft(
    keyboard: &Keyboard,
    baseline: &Snapshot,
    draft: &Snapshot,
    before: &Path,
) -> Result<Snapshot> {
    let changes = baseline.restore_changes(draft)?;
    ensure!(!changes.is_empty(), "Nothing to apply");
    let current = keyboard.refresh(baseline, &changes)?;
    ensure!(
        current == *baseline,
        "The keyboard changed since it was read. Reload, then edit again."
    );
    commit(keyboard, current.clone(), &changes, before)?;
    keyboard.refresh(&current, &changes)
}
