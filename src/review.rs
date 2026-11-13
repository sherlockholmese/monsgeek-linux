//! Human-readable differences, shared by the GUI and offline CLI review.
use crate::{device::Snapshot, keys, protocol::Change};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// What changed: a key, macro, or setting group.
    pub setting: String,
    /// Which part of it changed, such as a layer or a lighting field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub before: String,
    pub after: String,
    pub slot: Option<u8>,
}

/// One displayable part of a change.
struct Field {
    setting: String,
    scope: Option<String>,
    value: String,
    slot: Option<u8>,
}

fn field(setting: impl Into<String>, scope: Option<&str>, value: impl Into<String>) -> Field {
    Field {
        setting: setting.into(),
        scope: scope.map(Into::into),
        value: value.into(),
        slot: None,
    }
}

/// Only the parts that differ become rows, so a brightness change does not
/// repeat the effect, speed and color around it.
pub fn rows(base: &Snapshot, draft: &Snapshot) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for change in &base.restore_changes(draft)? {
        let old = base.inverse(change)?;
        let precision = base.advanced.as_ref().map_or(100, |a| a.units_per_mm);
        for (before, after) in fields(&old, base.model()?, precision)
            .into_iter()
            .zip(fields(change, base.model()?, precision))
        {
            if before.value != after.value {
                rows.push(Row {
                    setting: after.setting,
                    scope: after.scope,
                    before: before.value,
                    after: after.value,
                    slot: after.slot,
                });
            }
        }
    }
    Ok(rows)
}

fn hex(rgb: &[u8]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn on(flag: bool) -> String {
    if flag { "On" } else { "Off" }.into()
}

fn seconds(value: u16) -> String {
    if value.is_multiple_of(60) {
        format!("{} min", value / 60)
    } else {
        format!("{value} s")
    }
}

fn advanced_fields(
    change: &crate::advanced::Change,
    model: &crate::models::Model,
    precision: u16,
) -> Vec<Field> {
    use crate::advanced::{Change as A, Settings};
    match change {
        A::Options { bytes } => vec![
            field("Fn map", None, format!("{}", u16::from(bytes[0]) + 1)),
            field("Prevent accidental presses", None, on(bytes[1] != 0)),
            field(
                "Rapid trigger stability",
                None,
                format!("{}", u16::from(bytes[2]) * 25),
            ),
            field("Swap WASD and arrows", None, on(bytes[3] != 0)),
        ],
        A::ReportRate { hz } => vec![field("Report rate", None, format!("{hz} Hz"))],
        A::MagneticMode { value } => vec![field(
            "Actuation preset",
            None,
            ["Comfort", "Sensitive", "Gaming", "Custom"]
                .get(usize::from(*value))
                .copied()
                .unwrap_or("Unknown"),
        )],
        A::OutputMode { value } => vec![field(
            "Output mode",
            None,
            if *value == 0 { "Keyboard" } else { "Gamepad" },
        )],
        A::SideLighting { settings: l } => vec![
            field(
                "Side lighting",
                Some("Effect"),
                [
                    "Off",
                    "Static",
                    "Breathing",
                    "Neon",
                    "Wave",
                    "Snake",
                    "Music",
                ]
                .get(usize::from(l.mode))
                .copied()
                .unwrap_or("Unknown"),
            ),
            field(
                "Side lighting",
                Some("Brightness"),
                format!("{}/4", l.brightness),
            ),
            field(
                "Side lighting",
                Some("Speed"),
                format!("{}/4", l.wire_speed),
            ),
            field(
                "Side lighting",
                Some("Direction"),
                format!("{}", l.flags >> 4),
            ),
            field(
                "Side lighting",
                Some("Rainbow"),
                on(l.flags & 15 == model.dazzle),
            ),
            field("Side lighting", Some("Color"), hex(&l.rgb)),
        ],
        A::Magnetic { parameter, data } => {
            let mut settings = Settings {
                units_per_mm: precision,
                ..Default::default()
            };
            settings.magnetic.insert(*parameter, data.clone());
            let mut fields = Vec::new();
            for slot in 0..128u8 {
                let row = |scope: &str, value: String| Field {
                    slot: Some(slot),
                    ..field(model.slot_name(slot), Some(scope), value)
                };
                if *parameter == 10 {
                    for output in 0..4usize {
                        let byte = data[output * 128 + usize::from(slot)];
                        let stages = (0..4)
                            .map(|stage| {
                                ["Off", "Tap", "End hold", "Start hold"]
                                    [usize::from((byte >> (stage * 2)) & 3)]
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        fields.push(row(&format!("Dynamic output {}", output + 1), stages));
                    }
                } else if *parameter == 7 {
                    let value = data[usize::from(slot)];
                    fields.push(row(
                        "Behavior",
                        match value & 127 {
                            0 => "Normal".into(),
                            2 => "Dynamic".into(),
                            3 => "Tap-hold".into(),
                            4 => "Hold toggle".into(),
                            5 => "Tap toggle".into(),
                            7 => "Snap pair".into(),
                            n => format!("Vendor mode {n}"),
                        },
                    ));
                    fields.push(row("Rapid trigger", on(value & 128 != 0)));
                } else if *parameter == 9 {
                    fields.push(row(
                        "Snap partner",
                        model.slot_name(data[usize::from(slot)]),
                    ));
                } else if *parameter == 252 {
                    let code = data[usize::from(slot)];
                    fields.push(row(
                        "Switch",
                        model
                            .switches
                            .iter()
                            .find(|s| s.code == code)
                            .map(|s| s.label.clone())
                            .unwrap_or_else(|| format!("Switch {code}")),
                    ));
                } else {
                    let value = settings.value(model, *parameter, slot).unwrap_or(0.);
                    let name = match parameter {
                        0 => "Actuation",
                        1 => "Release",
                        2 => "Rapid press",
                        3 => "Rapid release",
                        4 => "Dynamic threshold",
                        5 => "Tap-hold time",
                        6 => "Bottom dead zone",
                        251 => "Top dead zone",
                        _ => "Magnetic setting",
                    };
                    fields.push(row(
                        name,
                        if *parameter == 5 {
                            format!("{value:.0} ms")
                        } else {
                            format!("{value:.3} mm")
                        },
                    ));
                }
            }
            fields
        }
    }
}

fn fields(change: &Change, model: &crate::models::Model, precision: u16) -> Vec<Field> {
    match change {
        Change::Advanced { change } => advanced_fields(change, model, precision),
        Change::Rgb { data } => data
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .map(|(slot, rgb)| Field {
                slot: Some(slot as u8),
                ..field(model.slot_name(slot as u8), Some("Color"), hex(rgb))
            })
            .collect(),
        Change::Remap {
            layer,
            fn_layer,
            slot,
            binding,
        } => vec![Field {
            slot: Some(*slot),
            ..field(
                model.slot_name(*slot),
                Some(&format!(
                    "Layer {}{}",
                    layer + 1,
                    if *fn_layer { " Fn" } else { "" }
                )),
                keys::binding_label(*binding),
            )
        }],
        Change::Profile { layer } => {
            vec![field("Active layer", None, format!("Layer {}", layer + 1))]
        }
        Change::Lighting { settings: l } => vec![
            field(
                "Lighting",
                Some("Effect"),
                keys::EFFECTS
                    .iter()
                    .find(|(id, _)| *id == l.mode)
                    .map(|(_, name)| *name)
                    .unwrap_or("Unknown effect"),
            ),
            field(
                "Lighting",
                Some("Brightness"),
                format!("{}/4", l.brightness),
            ),
            field(
                "Lighting",
                Some("Speed"),
                format!("{}/4", 5u8.saturating_sub(l.wire_speed)),
            ),
            field("Lighting", Some("Option"), format!("{}", l.flags >> 4)),
            field("Lighting", Some("Color"), hex(&l.rgb)),
        ],
        Change::Debounce { value } => vec![field("Debounce", None, format!("{value}"))],
        Change::Sleep { timers: t } => vec![
            field("Sleep", Some("Bluetooth"), seconds(t.time_bt)),
            field("Sleep", Some("2.4 GHz"), seconds(t.time_24)),
            field(
                "Sleep",
                Some("Bluetooth deep sleep"),
                seconds(t.deep_time_bt),
            ),
            field("Sleep", Some("2.4 GHz deep sleep"), seconds(t.deep_time_24)),
        ],
        Change::AutoOs { enabled } => vec![field("Automatic OS detection", None, on(*enabled))],
        Change::Options { bytes } => {
            let mut other = bytes.to_vec();
            other[0] &= !(1 | 8 | 16 | 128);
            other[2] &= !1;
            vec![
                field("Windows key lock", None, on(bytes[0] & 1 != 0)),
                field("Swap WASD and arrows", None, on(bytes[0] & 8 != 0)),
                field("LEDs off", None, on(bytes[0] & 16 != 0)),
                field("Keyboard lock", None, on(bytes[0] & 128 != 0)),
                field("Power saving", None, on(bytes[2] & 1 != 0)),
                field(
                    "Keyboard options",
                    Some("Other"),
                    other
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
            ]
        }
        Change::Macro { slot, data } => vec![field(
            format!("Macro {}", slot + 1),
            None,
            match crate::config::Macro::decode(data) {
                Ok(m) => format!(
                    "{}{}",
                    m.events
                        .iter()
                        .map(keys::macro_event_label)
                        .collect::<Vec<_>>()
                        .join(", "),
                    if m.repeat > 1 {
                        format!(", repeat {}", m.repeat)
                    } else {
                        String::new()
                    }
                ),
                Err(_) if data.iter().all(|b| *b == 0) => "Empty".into(),
                Err(_) => "Unreadable macro".into(),
            },
        )],
    }
}
