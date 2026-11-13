//! Model facts extracted from the official v500.3.3 host, never owner backups.
use std::{collections::BTreeMap, sync::LazyLock};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Yc500,
    Ry5088,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub id: u32,
    pub name: String,
    pub driver: String,
    pub vid: u16,
    pub pid: u16,
    pub layers: u8,
    pub fn_counts: BTreeMap<String, u8>,
    pub family: Family,
    pub magnetic: bool,
    pub sleep: bool,
    pub debounce: bool,
    pub dazzle: u8,
    pub normal: u8,
    pub palette: [[u8; 3]; 7],
    pub max_report_rate: u16,
    pub knob: Vec<String>,
    pub fixed_inputs: Vec<String>,
    pub side_light: bool,
    pub replaceable_switches: bool,
    pub switches: Vec<Switch>,
    pub travel_max: f32,
    pub magnetic_ranges: BTreeMap<u8, [f32; 3]>,
    pub sleep_ranges: Vec<Option<[u16; 2]>>,
    pub firmware_prefix: usize,
    pub firmware_wide: bool,
    pub matrices: BTreeMap<String, Vec<[u8; 4]>>,
    pub geometry: Geometry,
    pub lighting: LightCapabilities,
}

#[derive(Debug, Deserialize)]
pub struct Geometry {
    pub width: f32,
    pub height: f32,
    pub layout: BTreeMap<String, Rectangle>,
}

#[derive(Debug, Deserialize)]
pub struct Rectangle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
pub struct LightCapabilities {
    pub types: Vec<Effect>,
}

#[derive(Debug, Deserialize)]
pub struct Effect {
    #[serde(rename = "type")]
    pub name: String,
    #[serde(rename = "maxValue", default)]
    pub brightness: u8,
    #[serde(rename = "maxSpeed", default)]
    pub speed: u8,
    #[serde(default)]
    pub rgb: bool,
    #[serde(default)]
    pub dazzle: bool,
    #[serde(default)]
    pub options: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct Switch {
    pub code: u8,
    pub label: String,
}

pub static MODELS: LazyLock<Vec<Model>> = LazyLock::new(|| {
    #[derive(Deserialize)]
    struct Catalog {
        models: Vec<Model>,
    }
    serde_json::from_str::<Catalog>(include_str!("../docs/models.json"))
        .expect("generated model catalog")
        .models
});

pub static RECEIVERS: LazyLock<Vec<[u16; 2]>> = LazyLock::new(|| {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../docs/models.json")).expect("catalog");
    serde_json::from_value(doc["receivers"].clone()).expect("receiver pairs")
});

pub fn get(id: u32) -> Result<&'static Model> {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .with_context(|| format!("unsupported keyboard ID {id}; refusing configuration commands"))
}

pub fn original() -> &'static Model {
    get(983).expect("M1W catalog entry")
}

pub fn select(value: &str) -> Result<&'static Model> {
    if let Ok(id) = value.parse() {
        return get(id);
    }
    let found: Vec<_> = MODELS
        .iter()
        .filter(|m| m.name.eq_ignore_ascii_case(value) || m.driver.eq_ignore_ascii_case(value))
        .collect();
    ensure!(
        found.len() == 1,
        "model name is unknown or ambiguous; use the numeric ID from `models`"
    );
    Ok(found[0])
}

// Wire effect IDs are shared by these two families; advertised controls differ.
pub const EFFECT_NAMES: [&str; 26] = [
    "LightOff",
    "LightAlwaysOn",
    "LightBreath",
    "LightNeon",
    "LightWave",
    "LightRipple",
    "LightRaindrop",
    "LightSnake",
    "LightPressAction",
    "LightConverage",
    "LightSineWave",
    "LightKaleidoscope",
    "LightLineWave",
    "LightUserPicture",
    "LightLaser",
    "LightCircleWave",
    "LightDazzing",
    "LightRainDown",
    "LightMeteor",
    "LightPressActionOff",
    "LightMusicFollow3",
    "LightScreenColor",
    "LightMusicFollow2",
    "LightTrain",
    "LightFireWorks",
    "LightUserColor",
];

impl Model {
    /// Sample defaults for the offline browser. Never used to restore a device.
    pub fn example_snapshot(&self) -> crate::device::Snapshot {
        use crate::{
            advanced,
            config::SleepTimers,
            device::{Onboard, Snapshot},
            protocol::Lighting,
        };
        let mut advanced = advanced::Settings {
            units_per_mm: if self.family == Family::Ry5088 {
                100
            } else {
                10
            },
            ..Default::default()
        };
        if self.family == Family::Ry5088 {
            advanced.options = Some([0; 4]);
            advanced.report_rate = Some(1000);
        }
        if self.magnetic {
            if self.family == Family::Yc500 {
                advanced.magnetic_mode = Some(3);
            }
            for p in [0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 252] {
                if let Ok(size) = advanced::magnetic_bytes(self, p) {
                    advanced.magnetic.insert(p, vec![0; size]);
                }
            }
            for slot in 0..128 {
                for p in [0, 1] {
                    let _ = advanced.set_value(self, p, slot, 2.);
                }
            }
        }
        if self.side_light {
            advanced.side_lighting = Some(Lighting {
                mode: 1,
                wire_speed: 2,
                brightness: 4,
                flags: self.normal,
                rgb: [0, 200, 255],
            });
        }
        let time = |i: usize| {
            self.sleep_ranges
                .get(i)
                .copied()
                .flatten()
                .map_or(0, |[min, max]| 120u16.clamp(min, max))
        };
        Snapshot {
            advanced: Some(advanced),
            format_version: 3,
            device_id: self.id,
            usb_revision: 0,
            active_layer: 0,
            lighting: Lighting {
                mode: 4,
                wire_speed: self.wire_max_speed() - 2,
                brightness: 4,
                flags: (self.options(4).first().map_or(0, |o| o.0) << 4) | self.dazzle,
                rgb: [0, 200, 255],
            },
            keymaps: vec![
                self.defaults(false).iter().flatten().copied().collect();
                usize::from(self.base_maps())
            ],
            fn_keymaps: (0..self.fn_maps())
                .map(|map| {
                    (0..128)
                        .flat_map(|slot| self.default_binding(true, map, slot))
                        .collect()
                })
                .collect(),
            onboard: Some(Onboard {
                debounce: 2,
                sleep: SleepTimers {
                    time_bt: time(0),
                    time_24: time(1),
                    deep_time_bt: time(2),
                    deep_time_24: time(3),
                },
                options: [0; 3],
                auto_os: false,
                rgb: vec![0; 384],
                macros: vec![vec![0; 256]; 50],
            }),
        }
    }
    pub fn base_maps(&self) -> u8 {
        if self.magnetic {
            self.layers * 4
        } else {
            self.layers
        }
    }

    pub fn fn_maps(&self) -> u8 {
        self.fn_counts.values().sum()
    }

    pub fn fn_address(&self, map: u8) -> Result<(u8, u8)> {
        let win = self.fn_counts.get("win").copied().unwrap_or(0);
        ensure!(map < self.fn_maps(), "Fn map is out of range");
        Ok(if map < win { (0, map) } else { (1, map - win) })
    }

    pub fn wire_max_speed(&self) -> u8 {
        if self.family == Family::Yc500 { 5 } else { 4 }
    }

    pub fn layout_size(&self) -> (f32, f32) {
        (self.geometry.width / 46., self.geometry.height / 46.)
    }

    pub fn layout(&self) -> Vec<crate::keys::Cap> {
        let mut caps: Vec<_> = self
            .geometry
            .layout
            .iter()
            .filter(|(_, r)| r.kind != "knob")
            .filter_map(|(code, r)| {
                Some(crate::keys::Cap {
                    slot: self.slot_for_code(code)?,
                    usage: code_usage(code).unwrap_or(0),
                    x: r.x / 46.,
                    y: r.y / 46.,
                    w: (r.width + 4.) / 46.,
                    h: (r.height + 4.) / 46.,
                })
            })
            .collect();
        caps.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        caps
    }

    pub fn knob_slots(&self) -> Vec<u8> {
        self.knob
            .iter()
            .filter_map(|c| self.slot_for_code(c))
            .collect()
    }

    pub fn is_remappable(&self, slot: u8) -> bool {
        !self
            .fixed_inputs
            .iter()
            .any(|code| self.slot_for_code(code) == Some(slot))
    }

    pub fn targets(&self) -> Vec<crate::keys::Target> {
        let mut targets: Vec<_> = crate::keys::TARGETS
            .iter()
            .filter(|t| t.group != "Layers")
            .cloned()
            .collect();
        for layer in 0..self.layers {
            targets.push(crate::keys::Target {
                group: "Layers",
                label: format!("Layer {}", layer + 1),
                value: Some([8, 0, 4, layer]),
            });
        }
        targets
    }

    pub fn knob_cap(&self) -> Option<crate::keys::Cap> {
        let parts: Vec<_> = self
            .geometry
            .layout
            .values()
            .filter(|r| r.kind == "knob")
            .collect();
        let first = parts.first()?;
        let code = self.knob.get(1)?;
        let x = parts.iter().map(|r| r.x).fold(first.x, f32::min);
        let right = parts
            .iter()
            .map(|r| r.x + r.width)
            .fold(first.x + first.width, f32::max);
        Some(crate::keys::Cap {
            slot: self.slot_for_code(code)?,
            usage: 0,
            x: x / 46.,
            y: first.y / 46.,
            w: (right - x) / 46.,
            h: first.height / 46.,
        })
    }

    pub fn layer(&self, layer: u8) -> Result<u8> {
        ensure!(
            layer < self.layers,
            "{} layer must be 0..{}",
            self.name,
            self.layers - 1
        );
        Ok(layer)
    }

    pub fn defaults(&self, fn_layer: bool) -> &[[u8; 4]] {
        &self.matrices[if fn_layer {
            "defaultFnMatrix"
        } else {
            "defaultMatrix"
        }]
    }

    pub fn key_slot(&self, usage: u8) -> Result<u8> {
        ensure!(usage != 0, "usage 0 is not a physical key");
        // The M7W's 68-key drawing labels this position Insert; its concrete
        // matrix retains Home from the shared scan layout (slot 85).
        if self.id == 1215 && usage == 73 {
            return self
                .tuple_slot([0, 0, 74, 0])
                .context("M7W navigation position");
        }
        self.tuple_slot([0, 0, usage, 0])
            .with_context(|| format!("usage {usage} is not in the {} keymap", self.name))
    }

    pub fn tuple_slot(&self, tuple: [u8; 4]) -> Option<u8> {
        self.defaults(false)
            .iter()
            .position(|v| *v == tuple)
            .map(|i| i as u8)
    }

    pub fn slot_for_code(&self, code: &str) -> Option<u8> {
        let tuple = match code {
            "Insert" if self.id == 1215 => [0, 0, 74, 0],
            "Fn" => [10, 1, 0, 0],
            "AudioVolumeMute" | "VolumeBrightnessToggle3" if !self.knob.is_empty() => {
                // Several knob centers are a vendor volume/backlight toggle.
                if let Some(slot) = self.tuple_slot([10, 14, 0, 0]) {
                    return Some(slot);
                }
                [3, 0, 226, 0]
            }
            "AudioVolumeDown" => [3, 0, 234, 0],
            "AudioVolumeUp" => [3, 0, 233, 0],
            "AudioVolumeMute" => [3, 0, 226, 0],
            "MediaPlayPause" => [3, 0, 205, 0],
            "LaunchCalculator" => [3, 0, 146, 1],
            _ => [0, 0, code_usage(code)?, 0],
        };
        self.tuple_slot(tuple)
    }

    pub fn source_slot(&self, name: &str) -> Result<u8> {
        if let Some(slot) = name.strip_prefix("slot:") {
            let slot: u8 = slot.parse()?;
            ensure!(slot < 128, "matrix slot must be 0..127");
            return Ok(slot);
        }
        let lower = name.to_ascii_lowercase();
        if lower.bytes().all(|c| c.is_ascii_digit()) && !lower.is_empty() {
            return self.key_slot(lower.parse()?);
        }
        if let Some(index) = ["knob-left", "knob-press", "knob-right"]
            .iter()
            .position(|n| *n == lower)
        {
            return self
                .knob
                .get(index)
                .and_then(|c| self.slot_for_code(c))
                .context("this model has no configurable knob input with that name");
        }
        if lower == "fn" {
            return self.slot_for_code("Fn").context("Fn position unavailable");
        }
        if let Some(slot) = self
            .geometry
            .layout
            .keys()
            .find(|code| code.eq_ignore_ascii_case(name))
            .and_then(|c| self.slot_for_code(c))
        {
            return Ok(slot);
        }
        let usage = crate::keys::usage_from_key_name(name)
            .or_else(|| match lower.as_str() {
                "esc" => Some(41),
                "leftctrl" => Some(224),
                "rightctrl" => Some(228),
                "leftshift" => Some(225),
                "rightshift" => Some(229),
                "leftalt" => Some(226),
                "rightalt" => Some(230),
                "leftmeta" => Some(227),
                "rightmeta" => Some(231),
                _ => lower
                    .strip_prefix("0x")
                    .map(|s| u8::from_str_radix(s, 16).ok())
                    .unwrap_or_else(|| lower.parse().ok()),
            })
            .context("unknown source key")?;
        self.key_slot(usage)
    }

    pub fn slot_legend(&self, slot: u8) -> String {
        if let Some((code, _)) = self
            .geometry
            .layout
            .iter()
            .find(|(code, _)| self.slot_for_code(code) == Some(slot))
        {
            return code_usage(code)
                .map(crate::keys::usage_legend)
                .unwrap_or_else(|| match code.as_str() {
                    "AudioVolumeDown" => "Vol −".into(),
                    "AudioVolumeUp" => "Vol +".into(),
                    "AudioVolumeMute" => "Mute".into(),
                    "VolumeBrightnessToggle3" => "Toggle".into(),
                    "LaunchCalculator" => "Calc".into(),
                    _ => code.clone(),
                });
        }
        format!("Slot {slot}")
    }

    pub fn slot_name(&self, slot: u8) -> String {
        self.geometry
            .layout
            .keys()
            .find(|code| self.slot_for_code(code) == Some(slot))
            .and_then(|code| code_usage(code))
            .and_then(crate::keys::usage_name)
            .map(str::to_owned)
            .unwrap_or_else(|| self.slot_legend(slot))
    }

    pub fn effect(&self, mode: u8) -> Option<&Effect> {
        let name = EFFECT_NAMES.get(usize::from(mode))?;
        self.lighting.types.iter().find(|e| e.name == *name)
    }

    pub fn supports_effect(&self, mode: u8) -> bool {
        mode == 0 || self.effect(mode).is_some()
    }

    pub fn default_binding(&self, fn_layer: bool, map: u8, slot: u8) -> [u8; 4] {
        if fn_layer
            && self.fn_address(map).is_ok_and(|(system, _)| system == 1)
            && let Some(matrix) = self.matrices.get("defaultFnMacMatrix")
        {
            return matrix[usize::from(slot)];
        }
        self.defaults(fn_layer)[usize::from(slot)]
    }

    pub fn options(&self, mode: u8) -> Vec<(u8, String)> {
        self.effect(mode)
            .map(|e| {
                e.options
                    .iter()
                    .enumerate()
                    .filter_map(|(i, label)| {
                        label.as_ref().map(|label| {
                            (
                                i as u8,
                                match label.as_str() {
                                    "向右" => "Right".into(),
                                    "向左" => "Left".into(),
                                    "向下" => "Down".into(),
                                    "向上" => "Up".into(),
                                    "向外" => "Outward".into(),
                                    "向内" => "Inward".into(),
                                    "逆时针" => "Counterclockwise".into(),
                                    "顺时针" => "Clockwise".into(),
                                    "Z字形" => "Zigzag".into(),
                                    "回形" => "Loop".into(),
                                    _ => format!("{}", i + 1),
                                },
                            )
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn picture_count(&self) -> u8 {
        self.options(13).len() as u8
    }
}

pub fn code_usage(code: &str) -> Option<u8> {
    if let Some(c) = code
        .strip_prefix("Key")
        .filter(|c| c.len() == 1 && c.as_bytes()[0].is_ascii_alphabetic())
    {
        return Some(c.as_bytes()[0].to_ascii_lowercase() - b'a' + 4);
    }
    if let Some(c) = code
        .strip_prefix("Digit")
        .filter(|c| c.len() == 1 && c.as_bytes()[0].is_ascii_digit())
    {
        return Some(if c == "0" {
            39
        } else {
            c.as_bytes()[0] - b'1' + 30
        });
    }
    Some(match code {
        "ControlLeft" => 224,
        "ShiftLeft" => 225,
        "AltLeft" => 226,
        "MetaLeft" => 227,
        "ControlRight" => 228,
        "ShiftRight" => 229,
        "AltRight" => 230,
        "MetaRight" => 231,
        "Backquote" => 53,
        "Minus" => 45,
        "Equal" => 46,
        "BracketLeft" => 47,
        "BracketRight" => 48,
        "Backslash" => 49,
        "IntlBackslash" => 100,
        "IntlHash" => 50,
        "Semicolon" => 51,
        "Quote" => 52,
        "Comma" => 54,
        "Period" => 55,
        "Slash" => 56,
        "ArrowRight" => 79,
        "ArrowLeft" => 80,
        "ArrowDown" => 81,
        "ArrowUp" => 82,
        "NumLock" => 83,
        "NumpadDivide" => 84,
        "NumpadMultiply" => 85,
        "NumpadSubtract" => 86,
        "NumpadAdd" => 87,
        "NumpadEnter" => 88,
        "Numpad1" => 89,
        "Numpad2" => 90,
        "Numpad3" => 91,
        "Numpad4" => 92,
        "Numpad5" => 93,
        "Numpad6" => 94,
        "Numpad7" => 95,
        "Numpad8" => 96,
        "Numpad9" => 97,
        "Numpad0" => 98,
        "NumpadDecimal" => 99,
        "ContextMenu" => 101,
        "IntlRo" => 50,
        "R_IntlBackslash" => 135,
        "KanaMode" => 136,
        "IntlYen" => 137,
        "Convert" => 138,
        "NonConvert" => 139,
        _ => return crate::keys::usage_from_key_name(code),
    })
}
