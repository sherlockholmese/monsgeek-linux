//! Configuration codecs only; these functions never open a device.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::protocol::{Packet, key_slot, packet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Binding {
    Keyboard {
        keys: Vec<u8>,
    },
    Consumer {
        usage: u16,
    },
    System {
        usage: u8,
    },
    /// Vendor button codes F0..F4 (left, right, middle, back, forward).
    Mouse {
        button: u8,
    },
    /// Discrete scroll or movement from the vendor's ConfigMouse table.
    Pointer {
        action: String,
    },
    Function {
        value: [u8; 4],
    },
    Macro {
        slot: u8,
        mode: u8,
    },
    Disabled,
    Raw {
        value: [u8; 4],
    },
}

impl Binding {
    pub fn encode(&self) -> Result<[u8; 4]> {
        Ok(match self {
            Self::Keyboard { keys } => {
                ensure!(
                    (1..=3).contains(&keys.len()),
                    "keyboard binding needs 1..3 usages"
                );
                ensure!(
                    keys.iter().all(|k| (4..=239).contains(k)),
                    "reserved or unsupported keyboard usage"
                );
                let mut value = [0; 4];
                if keys.len() == 1 {
                    value[2] = keys[0];
                } else {
                    value[4 - keys.len()..].copy_from_slice(keys);
                }
                value
            }
            Self::Consumer { usage } => {
                let [lo, hi] = usage.to_le_bytes();
                [3, 0, lo, hi]
            }
            Self::System { usage } => [2, 0, *usage, 0],
            Self::Mouse { button } => {
                ensure!(
                    (0xf0..=0xf4).contains(button),
                    "mouse button must be F0..F4"
                );
                // Official index.1970d80c.js, cn table at character 225648:
                // Left=[1,0,240,0], Right=241, Middle=242, Back=243, Forward=244.
                // ConfigMouse copies cn[t.key] verbatim (character 1273644).
                [1, 0, *button, 0]
            }
            Self::Pointer { action } => match action.as_str() {
                "scroll-up" => [1, 0, 245, 1],
                "scroll-down" => [1, 0, 245, 255],
                "move-left" => [1, 0, 246, 251],
                "move-right" => [1, 0, 246, 5],
                "move-up" => [1, 0, 247, 251],
                "move-down" => [1, 0, 247, 5],
                "wheel-left" => [1, 0, 245, 0],
                "wheel-right" => [1, 0, 246, 0],
                "wheel-forward" => [1, 0, 247, 0],
                "wheel-back" => [1, 0, 248, 0],
                _ => bail!("unknown pointer action"),
            },
            Self::Macro { slot, mode } => {
                ensure!(
                    *slot < 50 && *mode <= 2,
                    "macro slot must be 0..49 and mode 0..2"
                );
                [9, *mode, *slot, 0]
            }
            Self::Disabled => [0; 4],
            Self::Function { value } | Self::Raw { value } => *value,
        })
    }

    /// Preserve tuples exactly, including unknown or noncanonical combinations.
    pub fn decode(value: [u8; 4]) -> Self {
        Self::Raw { value }
    }
}

/// Source lookup uses the original default matrix, never the current assignment.
pub fn source_slot(name: &str) -> Result<u8> {
    let name = name.to_ascii_lowercase();
    if let Some(slot) = name.strip_prefix("slot:") {
        let slot: u8 = slot.parse()?;
        ensure!(slot < 128, "matrix slot must be 0..127");
        return Ok(slot);
    }
    let usage = match name.as_str() {
        "knob-left" => return Ok(94),
        "knob-press" => return Ok(6),
        "knob-right" => return Ok(95),
        "fn" => return Ok(65),
        "esc" => 41,
        "enter" => 40,
        "space" => 44,
        "tab" => 43,
        "backspace" => 42,
        "capslock" => 57,
        "leftctrl" => 224,
        "leftshift" => 225,
        "leftalt" => 226,
        "leftmeta" => 227,
        "rightctrl" => 228,
        "rightshift" => 229,
        "rightalt" => 230,
        "right" => 79,
        "left" => 80,
        "down" => 81,
        "up" => 82,
        _ if name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase() => {
            name.as_bytes()[0] - b'a' + 4
        }
        _ => {
            if let Some(hex) = name.strip_prefix("0x") {
                u8::from_str_radix(hex, 16)?
            } else {
                name.parse()?
            }
        }
    };
    key_slot(usage)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Macro {
    pub repeat: u16,
    pub events: Vec<MacroEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MacroEvent {
    Keyboard {
        usage: u8,
        down: bool,
        delay_ms: u16,
    },
    MouseButton {
        button: u8,
        down: bool,
        delay_ms: u16,
    },
    Move {
        dx: i8,
        dy: i8,
        delay_ms: u16,
    },
}

impl Macro {
    pub fn encode(&self) -> Result<Vec<u8>> {
        ensure!(
            self.repeat >= 1,
            "macro repeat must be at least one (zero is an empty/raw slot)"
        );
        let mut out = self.repeat.to_le_bytes().to_vec();
        let mut held = [false; 245];
        for event in &self.events {
            let (code, down, delay) = match *event {
                MacroEvent::Keyboard {
                    usage,
                    down,
                    delay_ms,
                } => {
                    ensure!(
                        (4..=239).contains(&usage),
                        "unsupported macro keyboard usage"
                    );
                    (usage, down, delay_ms)
                }
                MacroEvent::MouseButton {
                    button,
                    down,
                    delay_ms,
                } => {
                    ensure!(
                        (0xf0..=0xf4).contains(&button),
                        "macro mouse button must be F0..F4"
                    );
                    (button, down, delay_ms)
                }
                MacroEvent::Move { dx, dy, delay_ms } => {
                    // Always extended: the vendor short writer and reader disagree.
                    out.extend_from_slice(&[0xf9, 0, dx as u8, dy as u8]);
                    out.extend_from_slice(&delay_ms.to_le_bytes());
                    ensure!(out.len() <= 256, "macro exceeds 254 event bytes");
                    continue;
                }
            };
            ensure!(
                held[code as usize] != down,
                "unbalanced macro key/button event for {code}"
            );
            held[code as usize] = down;
            let flag = if down { 0x80 } else { 0 };
            if (1..=127).contains(&delay) {
                out.extend_from_slice(&[code, flag | delay as u8]);
            } else {
                out.extend_from_slice(&[code, flag]);
                out.extend_from_slice(&delay.to_le_bytes());
            }
            ensure!(out.len() <= 256, "macro exceeds 254 event bytes");
        }
        ensure!(!held.contains(&true), "macro ends with held keys/buttons");
        out.resize(256, 0);
        Ok(out)
    }

    /// Semantic decode is not a lossless storage image. Keep captured raw bytes separately.
    pub fn decode(data: &[u8]) -> Result<Self> {
        ensure!(data.len() == 256, "macro image must be exactly 256 bytes");
        let repeat = u16::from_le_bytes([data[0], data[1]]);
        ensure!(
            repeat >= 1,
            "zero-repeat slot is not a user macro; retain raw image"
        );
        let mut events = Vec::new();
        let mut held = [false; 245];
        let mut pos = 2;
        while pos < data.len() {
            let code = data[pos];
            if code == 0 {
                ensure!(
                    data[pos..].iter().all(|b| *b == 0),
                    "nonzero data after macro terminator"
                );
                break;
            }
            let flags = *data
                .get(pos + 1)
                .ok_or_else(|| anyhow::anyhow!("truncated macro event"))?;
            let movement = code == 0xf9;
            let short = flags & 0x7f != 0;
            let size = match (movement, short) {
                (false, true) => 2,
                (false, false) => 4,
                (true, true) => 4,
                (true, false) => 6,
            };
            let record = data
                .get(pos..pos + size)
                .ok_or_else(|| anyhow::anyhow!("truncated macro record"))?;
            let delay_ms = if short {
                if movement {
                    u16::from(flags >> 1)
                } else {
                    u16::from(flags & 0x7f)
                }
            } else {
                u16::from_le_bytes([record[size - 2], record[size - 1]])
            };
            let down = flags & 0x80 != 0;
            if (4..=244).contains(&code) {
                ensure!(
                    held[code as usize] != down,
                    "unbalanced macro key/button event for {code}"
                );
                held[code as usize] = down;
            }
            events.push(match code {
                4..=239 => MacroEvent::Keyboard {
                    usage: code,
                    down,
                    delay_ms,
                },
                0xf0..=0xf4 => MacroEvent::MouseButton {
                    button: code,
                    down,
                    delay_ms,
                },
                0xf9 => {
                    ensure!(flags & 0x80 == 0, "invalid movement flags");
                    MacroEvent::Move {
                        dx: record[2] as i8,
                        dy: record[3] as i8,
                        delay_ms,
                    }
                }
                _ => bail!("unknown macro event code {code}"),
            });
            pos += size;
        }
        ensure!(!held.contains(&true), "macro ends with held keys/buttons");
        Ok(Self { repeat, events })
    }
}

/// Send all five pages, including zero gaps and an entirely zero slot image.
pub fn macro_packets(slot: u8, data: &[u8]) -> Result<Vec<Packet>> {
    ensure!(slot < 50, "macro slot must be 0..49");
    ensure!(data.len() == 256, "macro image must be exactly 256 bytes");
    Ok(data
        .chunks(56)
        .enumerate()
        .map(|(page, chunk)| {
            let mut out = packet(
                &[
                    0x16,
                    slot,
                    page as u8,
                    chunk.len() as u8,
                    u8::from(page == 4),
                ],
                7,
            );
            out[8..8 + chunk.len()].copy_from_slice(chunk);
            out
        })
        .collect())
}

pub fn rgb_packets(data: &[u8]) -> Result<Vec<Packet>> {
    ensure!(data.len() == 384, "RGB image must be exactly 384 bytes");
    Ok(data
        .chunks(56)
        .enumerate()
        .map(|(page, chunk)| {
            let mut out = packet(&[0x0c, 0, 0x80, 1, page as u8], 7);
            out[8..8 + chunk.len()].copy_from_slice(chunk);
            out
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepTimers {
    pub time_bt: u16,
    pub time_24: u16,
    pub deep_time_bt: u16,
    pub deep_time_24: u16,
}

impl SleepTimers {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.time_bt, self.time_24]
                .iter()
                .all(|v| (60..=3600).contains(v)),
            "sleep timers must be 60..3600 seconds"
        );
        ensure!(
            [self.deep_time_bt, self.deep_time_24]
                .iter()
                .all(|v| (600..=3600).contains(v)),
            "deep sleep timers must be 600..3600 seconds"
        );
        Ok(())
    }

    pub fn packet(&self) -> Result<Packet> {
        self.validate()?;
        let mut out = packet(&[0x12], 7);
        for (i, value) in [
            self.time_bt,
            self.time_24,
            self.deep_time_bt,
            self.deep_time_24,
        ]
        .iter()
        .enumerate()
        {
            out[8 + 2 * i..10 + 2 * i].copy_from_slice(&value.to_le_bytes());
        }
        Ok(out)
    }

    pub fn decode(reply: &Packet) -> Result<Self> {
        ensure!(reply[0] == 0x92, "unexpected sleep response opcode");
        // GET offsets are 1,3,5,7, not the SET payload offsets.
        Ok(Self {
            time_bt: u16::from_le_bytes([reply[1], reply[2]]),
            time_24: u16::from_le_bytes([reply[3], reply[4]]),
            deep_time_bt: u16::from_le_bytes([reply[5], reply[6]]),
            deep_time_24: u16::from_le_bytes([reply[7], reply[8]]),
        })
    }
}
