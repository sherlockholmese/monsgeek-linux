//! Human-readable differences, shared by the GUI and offline CLI review.
use crate::{device::Snapshot, keys, protocol::Change};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub setting: String,
    pub before: String,
    pub after: String,
    pub slot: Option<u8>,
}
pub fn rows(base: &Snapshot, draft: &Snapshot) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for change in &base.restore_changes(draft)? {
        let old = base.inverse(change)?;
        if let (Change::Rgb { data: before }, Change::Rgb { data: after }) = (&old, change) {
            for (slot, (before, after)) in before
                .as_chunks::<3>()
                .0
                .iter()
                .zip(after.as_chunks::<3>().0.iter())
                .enumerate()
            {
                if before != after {
                    rows.push(Row {
                        setting: format!("{} · RGB", keys::slot_name(slot as u8)),
                        before: format!("#{:02X}{:02X}{:02X}", before[0], before[1], before[2]),
                        after: format!("#{:02X}{:02X}{:02X}", after[0], after[1], after[2]),
                        slot: Some(slot as u8),
                    });
                }
            }
            continue;
        }
        let (setting, after, slot) = describe(change);
        rows.push(Row {
            setting,
            before: describe(&old).1,
            after,
            slot,
        });
    }
    Ok(rows)
}
fn describe(change: &Change) -> (String, String, Option<u8>) {
    let (label, value) = match change {
        Change::Remap {
            layer,
            fn_layer,
            slot,
            binding,
        } => {
            return (
                format!(
                    "{} · Layer {}{}",
                    keys::slot_name(*slot),
                    layer + 1,
                    if *fn_layer { " Fn" } else { "" }
                ),
                keys::binding_label(*binding),
                Some(*slot),
            );
        }
        Change::Profile { layer } => ("Active layer", format!("Layer {}", layer + 1)),
        Change::Lighting { settings: l } => (
            "Lighting",
            format!(
                "{}, brightness {}/4, speed {}/4, option {}, #{:02X}{:02X}{:02X}",
                keys::EFFECTS
                    .iter()
                    .find(|(id, _)| *id == l.mode)
                    .map(|(_, name)| *name)
                    .unwrap_or("Unknown effect"),
                l.brightness,
                5u8.saturating_sub(l.wire_speed),
                l.flags >> 4,
                l.rgb[0],
                l.rgb[1],
                l.rgb[2]
            ),
        ),
        Change::Debounce { value } => ("Debounce", format!("{value} vendor units")),
        Change::Sleep { timers: t } => (
            "Sleep timers",
            format!(
                "BT {}s / {}s deep; receiver {}s / {}s deep",
                t.time_bt, t.deep_time_bt, t.time_24, t.deep_time_24
            ),
        ),
        Change::AutoOs { enabled } => (
            "Automatic OS detection",
            if *enabled { "On" } else { "Off" }.into(),
        ),
        Change::Options { bytes } => (
            "Keyboard options",
            format!(
                "Windows lock: {}; WASD swap: {}; LEDs off: {}; keyboard lock: {}; power save: {} (raw {:02X?})",
                bytes[0] & 1 != 0,
                bytes[0] & 8 != 0,
                bytes[0] & 16 != 0,
                bytes[0] & 128 != 0,
                bytes[2] & 1 != 0,
                bytes
            ),
        ),
        Change::Macro { slot, data } => {
            return (
                format!("Macro {}", slot + 1),
                match crate::config::Macro::decode(data) {
                    Ok(m) => format!(
                        "Repeat {}: {}",
                        m.repeat,
                        m.events
                            .iter()
                            .map(keys::macro_event_label)
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                    Err(_) if data.iter().all(|b| *b == 0) => "Empty".into(),
                    Err(_) => "Stored macro (raw data preserved)".into(),
                },
                None,
            );
        }
        Change::Rgb { data } => (
            "Custom RGB picture",
            format!(
                "{} coloured slots",
                data.chunks(3).filter(|p| p.iter().any(|b| *b != 0)).count()
            ),
        ),
    };
    (label.into(), value, None)
}
