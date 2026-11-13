use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const VID: u16 = 0x3151;
pub const PID: u16 = 0x4015;
pub const DEVICE_ID: u32 = 983;
pub type Packet = [u8; 64];

/// Checksum covers only the command header, not the entire feature report.
pub fn packet(header: &[u8], checksum_at: usize) -> Packet {
    assert!(header.len() <= 64 && matches!(checksum_at, 7 | 8));
    let mut out = [0; 64];
    out[..header.len()].copy_from_slice(header);
    out[checksum_at] = !out[..checksum_at]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    out
}

pub fn layer(value: u8) -> Result<u8> {
    ensure!(value < 4, "layer must be 0..3 (vendor layer numbering)");
    Ok(value)
}

/// Raw lighting fields retain palette modes and unknown firmware values on reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lighting {
    pub mode: u8,
    pub wire_speed: u8,
    pub brightness: u8,
    pub flags: u8,
    pub rgb: [u8; 3],
}

impl Lighting {
    pub fn new_for(
        model: &crate::models::Model,
        mode: u8,
        speed: u8,
        brightness: u8,
        option: u8,
        rainbow: bool,
        rgb: [u8; 3],
    ) -> Result<Self> {
        ensure!(
            model.supports_effect(mode),
            "effect {mode} is unavailable on {}",
            model.name
        );
        ensure!(
            !matches!(mode, 20..=22),
            "host streaming effects require a streaming service"
        );
        ensure!(
            speed <= 4 && brightness <= 4,
            "speed and brightness must be 0..4"
        );
        if let Some(effect) = model.effect(mode) {
            ensure!(
                brightness <= effect.brightness,
                "brightness exceeds model limit"
            );
            if !effect.options.is_empty() {
                ensure!(
                    effect
                        .options
                        .get(usize::from(option))
                        .is_some_and(Option::is_some),
                    "direction or picture unavailable on this model"
                );
            } else {
                ensure!(option == 0, "effect has no direction option");
            }
        } else {
            ensure!(option == 0, "effect has no direction option");
        }
        Ok(Self {
            mode,
            wire_speed: model.wire_max_speed() - speed,
            brightness,
            flags: (option << 4)
                | if mode == 13 {
                    0
                } else if rainbow {
                    model.dazzle
                } else {
                    model.normal
                },
            rgb: if mode == 13 {
                [0, 200, 200]
            } else if rgb == [255; 3] {
                [250, 255, 250]
            } else {
                rgb
            },
        })
    }
    pub fn decode(reply: &Packet) -> Result<Self> {
        ensure!(reply[0] == 0x87, "unexpected lighting response opcode");
        Ok(Self {
            mode: reply[1],
            wire_speed: reply[2],
            brightness: reply[3],
            flags: reply[4],
            rgb: [reply[5], reply[6], reply[7]],
        })
    }

    /// Onboard effects, including custom picture selection. No host streaming.
    pub fn new(
        mode: u8,
        speed: u8,
        brightness: u8,
        option: u8,
        rainbow: bool,
        rgb: [u8; 3],
    ) -> Result<Self> {
        ensure!(matches!(mode, 0..=19), "unsupported onboard lighting mode");
        ensure!(
            speed <= 4 && brightness <= 4,
            "speed and brightness must be 0..4"
        );
        let max_option = match mode {
            4 => 3,
            13 => 2,
            7 | 11 | 12 | 15 => 1,
            _ => 0,
        };
        ensure!(
            option <= max_option,
            "option must be 0..{max_option} for this effect"
        );
        // Original M1W overrides CommonKbYc500: DAZZLE=7, NORMAL=8.
        // The vendor substitutes FAFFFA for white on the wire.
        Ok(Self {
            mode,
            wire_speed: 5 - speed,
            brightness,
            flags: (option << 4)
                | if mode == 13 {
                    0
                } else if rainbow {
                    7
                } else {
                    8
                },
            rgb: if mode == 13 {
                [0, 200, 200]
            } else if rgb == [255; 3] {
                [250, 255, 250]
            } else {
                rgb
            },
        })
    }

    pub fn packet(&self) -> Packet {
        packet(
            &[
                7,
                self.mode,
                self.wire_speed,
                self.brightness,
                self.flags,
                self.rgb[0],
                self.rgb[1],
                self.rgb[2],
            ],
            8,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Change {
    Advanced {
        change: crate::advanced::Change,
    },
    Profile {
        layer: u8,
    },
    Lighting {
        settings: Lighting,
    },
    Remap {
        layer: u8,
        fn_layer: bool,
        slot: u8,
        binding: [u8; 4],
    },
    Debounce {
        value: u8,
    },
    Sleep {
        timers: crate::config::SleepTimers,
    },
    Options {
        bytes: [u8; 3],
    },
    AutoOs {
        enabled: bool,
    },
    Macro {
        slot: u8,
        data: Vec<u8>,
    },
    Rgb {
        data: Vec<u8>,
    },
}

impl Change {
    /// Encode against the identified model. The legacy methods retain the M1W API.
    pub fn packets_for(&self, model: &crate::models::Model) -> Result<Vec<Packet>> {
        use crate::models::Family;
        let modern = model.family == Family::Ry5088;
        match self {
            Self::Advanced { change } => change.packets(model),
            Self::Profile { layer } => {
                model.layer(*layer)?;
                Ok(vec![packet(
                    &[
                        if modern { 4 } else { 5 },
                        if model.magnetic && !modern {
                            *layer * 4
                        } else {
                            *layer
                        },
                    ],
                    7,
                )])
            }
            Self::Remap {
                layer,
                fn_layer,
                slot,
                binding,
            } => {
                ensure!(*slot < 128, "matrix slot must be 0..127");
                ensure!(
                    model.is_remappable(*slot),
                    "this input is fixed by the {} firmware",
                    model.name
                );
                ensure!(
                    *layer
                        < if *fn_layer {
                            model.fn_maps()
                        } else {
                            model.base_maps()
                        },
                    "map out of range for {}",
                    model.name
                );
                let mut header = [0; 12];
                if modern && *fn_layer {
                    let (system, index) = model.fn_address(*layer)?;
                    header[..4].copy_from_slice(&[0x10, system, index, *slot]);
                } else if modern {
                    header[..3].copy_from_slice(&[
                        0x0a,
                        if model.magnetic { *layer / 4 } else { *layer },
                        *slot,
                    ]);
                    header[5] = 1;
                    header[6] = if model.magnetic { *layer % 4 } else { 0 };
                } else {
                    header[..3].copy_from_slice(&[
                        if *fn_layer { 0x15 } else { 0x13 },
                        *layer,
                        *slot,
                    ]);
                }
                header[8..].copy_from_slice(binding);
                Ok(vec![packet(&header, 7)])
            }
            Self::Debounce { value } => {
                ensure!(model.debounce, "debounce is unavailable on {}", model.name);
                ensure!((1..=10).contains(value), "debounce must be 1..10");
                Ok(vec![if modern {
                    packet(&[6, *value], 7)
                } else {
                    packet(&[0x11, 0, *value], 7)
                }])
            }
            Self::Sleep { timers } => {
                ensure!(
                    model.sleep,
                    "wireless sleep is unavailable on {}",
                    model.name
                );
                for (value, bounds) in [
                    timers.time_bt,
                    timers.time_24,
                    timers.deep_time_bt,
                    timers.deep_time_24,
                ]
                .iter()
                .zip(&model.sleep_ranges)
                {
                    if let Some([min, max]) = bounds {
                        ensure!(
                            (*min..=*max).contains(value),
                            "timer must be {min}..{max} seconds on {}",
                            model.name
                        );
                    }
                }
                if modern {
                    let mut header = [0; 16];
                    header[0] = 0x11;
                    for (i, value) in [
                        timers.time_bt,
                        timers.time_24,
                        timers.deep_time_bt,
                        timers.deep_time_24,
                    ]
                    .iter()
                    .enumerate()
                    {
                        ensure!(*value <= 64800, "sleep timer must not exceed 18 hours");
                        header[8 + i * 2..10 + i * 2].copy_from_slice(&value.to_le_bytes());
                    }
                    Ok(vec![packet(&header, 7)])
                } else {
                    self.packets()
                }
            }
            Self::Options { .. } => {
                ensure!(!modern, "this model uses modern-options");
                self.packets()
            }
            Self::Macro { slot, data } if modern => {
                let mut packets = crate::config::macro_packets(*slot, data)?;
                for p in &mut packets {
                    p[0] = 0x0b;
                    p[7] = 0;
                    *p = packet(p, 7);
                }
                Ok(packets)
            }
            Self::Rgb { data } if modern => {
                ensure!(data.len() == 384, "RGB image must contain 384 bytes");
                Ok(data
                    .chunks(56)
                    .enumerate()
                    .map(|(page, bytes)| {
                        let mut header = [0; 64];
                        header[..6].copy_from_slice(&[
                            12,
                            0,
                            255,
                            page as u8,
                            bytes.len() as u8,
                            u8::from(page == 6),
                        ]);
                        header[8..8 + bytes.len()].copy_from_slice(bytes);
                        packet(&header, 7)
                    })
                    .collect())
            }
            _ => self.packets(),
        }
    }
    pub fn packets(&self) -> Result<Vec<Packet>> {
        match self {
            Self::Advanced { change } => change.packets(crate::models::original()),
            Self::Macro { slot, data } => crate::config::macro_packets(*slot, data),
            Self::Rgb { data } => crate::config::rgb_packets(data),
            _ => Ok(vec![self.packet()?]),
        }
    }

    pub fn packet(&self) -> Result<Packet> {
        match self {
            Self::Advanced { .. } => anyhow::bail!("advanced change: use packets_for()"),
            Self::Profile { layer: value } => Ok(packet(&[5, layer(*value)?], 7)),
            Self::Lighting { settings } => Ok(settings.packet()),
            Self::Debounce { value } => {
                ensure!((1..=10).contains(value), "debounce must be 1..10");
                Ok(packet(&[0x11, 0, *value], 7))
            }
            Self::Sleep { timers } => timers.packet(),
            Self::Options { bytes } => Ok(packet(&[6, 0, bytes[0], bytes[1], bytes[2]], 7)),
            Self::AutoOs { enabled } => Ok(packet(&[0x17, u8::from(*enabled)], 7)),
            Self::Macro { .. } | Self::Rgb { .. } => {
                anyhow::bail!("multipart change: use packets()")
            }
            Self::Remap {
                layer: value,
                fn_layer,
                slot,
                binding,
            } => {
                layer(*value)?;
                ensure!(*slot < 128, "matrix slot must be 0..127");
                let mut header = [0; 12];
                header[0] = if *fn_layer { 0x15 } else { 0x13 };
                header[1] = *value;
                header[2] = *slot;
                header[8..12].copy_from_slice(binding);
                Ok(packet(&header, 7))
            }
        }
    }
}

/// USB keyboard usage -> original M1W matrix slot. Not a visual row-major index.
pub fn key_slot(usage: u8) -> Result<u8> {
    const USAGES: [u8; 90] = [
        41, 53, 43, 57, 225, 224, 0, 30, 20, 4, 100, 0, 58, 31, 26, 22, 29, 227, 59, 32, 8, 7, 27,
        226, 60, 33, 21, 9, 6, 0, 61, 34, 23, 10, 25, 0, 62, 35, 28, 11, 5, 44, 63, 36, 24, 13, 17,
        0, 64, 37, 12, 14, 16, 0, 65, 38, 18, 15, 54, 230, 66, 39, 19, 51, 55, 0, 67, 45, 47, 52,
        56, 228, 68, 46, 48, 50, 229, 80, 69, 42, 49, 40, 82, 81, 76, 74, 75, 78, 77, 79,
    ];
    ensure!(usage != 0, "usage 0 is not a physical key");
    USAGES
        .iter()
        .position(|v| *v == usage)
        .map(|i| i as u8)
        .ok_or_else(|| anyhow::anyhow!("usage {usage} is not in the original M1W keymap"))
}

/// hidraw omits the ID for unnumbered reports; some other backends retain it.
pub fn decode_feature(bytes: &[u8]) -> Result<Packet> {
    let data = match bytes.len() {
        64 => bytes,
        65 if bytes[0] == 0 => &bytes[1..],
        n => anyhow::bail!("expected 64 data bytes or report ID 0 + 64 bytes, got {n}"),
    };
    Ok(data.try_into().expect("length checked"))
}

pub fn identify(reply: &Packet) -> Result<u32> {
    ensure!(reply[0] == 0x8f, "unexpected identity response opcode");
    let id = u32::from_le_bytes(reply[1..5].try_into()?);
    crate::models::get(id)?;
    Ok(id)
}
