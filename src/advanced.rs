//! Family-specific settings. Raw arrays retain unknown values when editing a key.
use crate::{
    models::{Family, Model},
    protocol::{Lighting, Packet, packet},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub rf_revision: Option<u16>,
    #[serde(default = "default_precision")]
    pub units_per_mm: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_rate: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_lighting: Option<Lighting>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub magnetic: BTreeMap<u8, Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magnetic_mode: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_mode: Option<u8>,
}

fn default_precision() -> u16 {
    100
}

/// Vendor fallback when the precision feature list is unavailable: nonzero
/// RF version takes precedence over USB version, even when it is older.
pub fn legacy_precision(usb_revision: u16, rf_revision: Option<u16>) -> u16 {
    match rf_revision.filter(|v| *v != 0).unwrap_or(usb_revision) {
        0x0500.. => 200,
        0x0300..=0x04ff => 100,
        _ => 10,
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rf_revision: None,
            units_per_mm: default_precision(),
            options: None,
            report_rate: None,
            side_lighting: None,
            magnetic: BTreeMap::new(),
            magnetic_mode: None,
            output_mode: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "setting", rename_all = "kebab-case")]
pub enum Change {
    Options { bytes: [u8; 4] },
    ReportRate { hz: u16 },
    SideLighting { settings: Lighting },
    Magnetic { parameter: u8, data: Vec<u8> },
    MagneticMode { value: u8 },
    OutputMode { value: u8 },
}

/// Number of raw bytes in a parameter's full 128-key array.
pub fn magnetic_bytes(model: &Model, parameter: u8) -> Result<usize> {
    ensure!(
        model.magnetic,
        "magnetic settings unavailable on {}",
        model.name
    );
    Ok(match parameter {
        0..=4 | 6 if model.family == Family::Ry5088 => 256,
        0..=7 | 9 => 128,
        10 => 512,
        252 if model.replaceable_switches => 128,
        251 if model.family == Family::Ry5088 => 128,
        _ => anyhow::bail!("unsupported magnetic parameter {parameter}"),
    })
}

impl Change {
    pub fn packets(&self, model: &Model) -> Result<Vec<Packet>> {
        let modern = model.family == Family::Ry5088;
        Ok(match self {
            Self::OutputMode { value } => {
                ensure!(
                    modern && model.magnetic && *value <= 1,
                    "output mode unavailable"
                );
                vec![packet(&[29, *value], 7)]
            }
            Self::Options { bytes } => {
                ensure!(modern, "modern options unavailable on this model");
                vec![packet(&[9, 0, bytes[0], bytes[1], bytes[2], bytes[3]], 7)]
            }
            Self::ReportRate { hz } => {
                ensure!(
                    modern,
                    "report-rate configuration unavailable on this model"
                );
                ensure!(
                    *hz <= model.max_report_rate,
                    "report rate exceeds the advertised model limit"
                );
                let value = [8000, 4000, 2000, 1000, 500, 250, 125]
                    .iter()
                    .position(|v| v == hz)
                    .ok_or_else(|| anyhow::anyhow!("unsupported report rate"))?;
                vec![packet(&[3, 0, value as u8], 7)]
            }
            Self::SideLighting { settings } => {
                ensure!(model.side_light, "side lighting unavailable on this model");
                let mut p = settings.packet();
                p[0] = 8;
                // Side effects use direct speed, unlike the main lighting.
                p[8] = 0;
                vec![packet(&p, 8)]
            }
            Self::MagneticMode { value } => {
                ensure!(
                    model.magnetic && !modern && *value <= 3,
                    "magnetic preset unavailable"
                );
                vec![packet(&[29, *value], 7)]
            }
            Self::Magnetic { parameter, data } => {
                ensure!(
                    data.len() == magnetic_bytes(model, *parameter)?,
                    "invalid magnetic array size"
                );
                // DKS readback is four channel planes; writer uses a per-key
                // four-byte record (operation 8), never operation 10.
                if *parameter == 10 {
                    (0..128)
                        .map(|slot| {
                            let values: Vec<_> =
                                (0..4).map(|plane| data[plane * 128 + slot]).collect();
                            let mut p = packet(&[101, 8, 0, slot as u8, u8::from(slot == 127)], 7);
                            p[8..12].copy_from_slice(&values);
                            p
                        })
                        .collect()
                } else {
                    let count = data.len().div_ceil(56);
                    data.chunks(56)
                        .enumerate()
                        .map(|(page, bytes)| {
                            let mut p = packet(
                                &[101, *parameter, 1, page as u8, u8::from(page + 1 == count)],
                                7,
                            );
                            p[8..8 + bytes.len()].copy_from_slice(bytes);
                            p
                        })
                        .collect()
                }
            }
        })
    }

    pub fn inverse(&self, settings: &Settings) -> Result<Self> {
        use anyhow::Context;
        Ok(match self {
            Self::OutputMode { .. } => Self::OutputMode {
                value: settings.output_mode.context("output mode not captured")?,
            },
            Self::Options { .. } => Self::Options {
                bytes: settings.options.context("options not captured")?,
            },
            Self::ReportRate { .. } => Self::ReportRate {
                hz: settings.report_rate.context("report rate not captured")?,
            },
            Self::SideLighting { .. } => Self::SideLighting {
                settings: settings
                    .side_lighting
                    .clone()
                    .context("side lighting not captured")?,
            },
            Self::MagneticMode { .. } => Self::MagneticMode {
                value: settings
                    .magnetic_mode
                    .context("magnetic preset not captured")?,
            },
            Self::Magnetic { parameter, .. } => Self::Magnetic {
                parameter: *parameter,
                data: settings
                    .magnetic
                    .get(parameter)
                    .context("magnetic parameter not captured")?
                    .clone(),
            },
        })
    }
}

impl Settings {
    pub fn bounds(&self, model: &Model, parameter: u8) -> (f32, f32, f32) {
        let precision_step = if model.family == Family::Yc500 {
            0.1
        } else {
            1. / f32::from(self.units_per_mm.max(1))
        };
        let [mut min, mut max, mut step] = model
            .magnetic_ranges
            .get(&parameter)
            .copied()
            .unwrap_or([0., 0., 0.]);
        if step == 0. {
            step = precision_step;
        }
        match parameter {
            0 | 1 => {
                if self.units_per_mm == 10 {
                    max = 4.;
                }
            }
            2 | 3 => {
                if min == 0. {
                    min = step;
                }
                if max == 0. {
                    max = if self.units_per_mm == 10 { 2.5 } else { 2. };
                }
            }
            4 => {
                min = if model.family == Family::Yc500 {
                    0.2
                } else {
                    0.1
                };
                max = model.travel_max;
            }
            5 => {
                min = 0.;
                max = 2550.;
                step = 10.;
            }
            6 => {
                if max == 0. {
                    max = if self.units_per_mm == 10 {
                        if model.family == Family::Yc500 {
                            3.9
                        } else {
                            4.
                        }
                    } else {
                        1.
                    };
                }
            }
            251 => {
                min = 0.;
                max = 1f32.min(255. / f32::from(self.units_per_mm.max(1)));
            }
            _ => {
                min = 0.;
                max = 255.;
                step = 1.;
            }
        }
        (min, max, step)
    }
    pub fn set_mode(&mut self, slot: u8, mode: u8, rapid: bool) -> Result<()> {
        use anyhow::Context;
        ensure!(
            slot < 128 && matches!(mode, 0 | 2..=5),
            "unsupported key mode"
        );
        let old_partner = self
            .magnetic
            .get(&9)
            .map(|v| usize::from(v[usize::from(slot)]));
        let modes = self
            .magnetic
            .get_mut(&7)
            .context("key modes not captured")?;
        if modes[usize::from(slot)] & 127 == 7
            && let Some(partner) = old_partner
            && partner < 128
        {
            modes[partner] &= 128;
        }
        modes[usize::from(slot)] = mode | if rapid { 128 } else { 0 };
        Ok(())
    }
    pub fn value(&self, model: &Model, parameter: u8, slot: u8) -> Option<f32> {
        let data = self.magnetic.get(&parameter)?;
        let word = model.family == Family::Ry5088 && matches!(parameter, 0..=4 | 6);
        let i = usize::from(slot) * if word { 2 } else { 1 };
        let raw = if word {
            u16::from_le_bytes(data.get(i..i + 2)?.try_into().ok()?)
        } else {
            u16::from(*data.get(i)?)
        };
        Some(if parameter == 5 {
            f32::from(raw) * 10.
        } else if model.family == Family::Yc500 && parameter == 6 {
            (39. - f32::from(raw)) / 10.
        } else if model.family == Family::Yc500 && parameter <= 4 {
            (f32::from(raw) + if parameter == 4 { 2. } else { 1. }) / 10.
        } else if matches!(parameter, 0..=4 | 6 | 251) {
            f32::from(raw) / f32::from(self.units_per_mm.max(1))
        } else {
            f32::from(raw)
        })
    }

    pub fn set_value(&mut self, model: &Model, parameter: u8, slot: u8, value: f32) -> Result<()> {
        use anyhow::Context;
        ensure!(
            slot < 128 && value.is_finite() && value >= 0.,
            "invalid key setting"
        );
        let (min, max, _) = self.bounds(model, parameter);
        ensure!(
            value + 0.00001 >= min && value <= max + 0.00001,
            "setting must be {min}..{max} on {}",
            model.name
        );
        let raw = if parameter == 5 {
            ensure!(value <= 2550., "tap-hold time must be 0..2550 ms");
            value / 10.
        } else if model.family == Family::Yc500 && parameter == 6 {
            39. - value * 10.
        } else if model.family == Family::Yc500 && parameter <= 4 {
            ensure!((0.1..=4.).contains(&value), "travel must be 0.1..4 mm");
            value * 10. - if parameter == 4 { 2. } else { 1. }
        } else if matches!(parameter, 0..=4 | 6 | 251) {
            value * f32::from(self.units_per_mm.max(1))
        } else {
            ensure!(value <= 255., "value must fit a byte");
            value
        };
        let word = model.family == Family::Ry5088 && matches!(parameter, 0..=4 | 6);
        ensure!(
            raw >= 0. && raw <= if word { 65535. } else { 255. },
            "value cannot be represented on this model"
        );
        ensure!(
            (raw - raw.round()).abs() < 0.001,
            "value cannot be represented at the keyboard's precision"
        );
        let data = self
            .magnetic
            .get_mut(&parameter)
            .context("parameter not captured")?;
        let i = usize::from(slot) * if word { 2 } else { 1 };
        if word {
            data[i..i + 2].copy_from_slice(&(raw.round() as u16).to_le_bytes());
        } else {
            data[i] = raw.round() as u8;
        }
        Ok(())
    }

    pub fn pair(&mut self, slot: u8, partner: u8) -> Result<()> {
        use anyhow::Context;
        ensure!(
            slot < 128 && partner < 128 && slot != partner,
            "choose two different keys"
        );
        let links = self
            .magnetic
            .get(&9)
            .context("Snap links not captured")?
            .clone();
        let modes = self
            .magnetic
            .get_mut(&7)
            .context("key modes not captured")?;
        for key in [slot, partner] {
            let old = usize::from(links[usize::from(key)]);
            if modes[usize::from(key)] & 127 == 7
                && old < 128
                && old != usize::from(slot)
                && old != usize::from(partner)
                && links[old] == key
            {
                modes[old] &= 128;
            }
            modes[usize::from(key)] = (modes[usize::from(key)] & 128) | 7;
        }
        let links = self.magnetic.get_mut(&9).expect("checked");
        links[usize::from(slot)] = partner;
        links[usize::from(partner)] = slot;
        Ok(())
    }

    pub fn differences(&self, wanted: &Self) -> Vec<Change> {
        let mut out = Vec::new();
        if let Some(bytes) = wanted.options
            && self.options != Some(bytes)
        {
            out.push(Change::Options { bytes });
        }
        if let Some(hz) = wanted.report_rate
            && self.report_rate != Some(hz)
        {
            out.push(Change::ReportRate { hz });
        }
        if let Some(settings) = &wanted.side_lighting
            && self.side_lighting.as_ref() != Some(settings)
        {
            out.push(Change::SideLighting {
                settings: settings.clone(),
            });
        }
        // Values precede modes so an advanced action never sees a half-written body.
        for (parameter, data) in wanted
            .magnetic
            .iter()
            .filter(|(p, _)| **p != 7)
            .chain(wanted.magnetic.iter().filter(|(p, _)| **p == 7))
        {
            if self.magnetic.get(parameter) != Some(data) {
                out.push(Change::Magnetic {
                    parameter: *parameter,
                    data: data.clone(),
                });
            }
        }
        if let Some(value) = wanted.magnetic_mode
            && self.magnetic_mode != Some(value)
        {
            out.push(Change::MagneticMode { value });
        }
        if let Some(value) = wanted.output_mode
            && self.output_mode != Some(value)
        {
            out.push(Change::OutputMode { value });
        }
        out
    }
}
