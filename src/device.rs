use std::{
    collections::BTreeSet,
    ffi::CStr,
    fs::{File, OpenOptions},
    sync::{Arc, atomic::AtomicUsize},
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use hidapi::{DeviceInfo, HidApi};
use serde::{Deserialize, Serialize};

use crate::config::SleepTimers;
use crate::protocol::{self, Change, Lighting, Packet, packet};
use crate::transport::{self, Channel, Kind};

pub use transport::usb_candidate as candidate;

/// Exchanges in a full snapshot after identification: revision, 64 keymap
/// pages, six settings queries, 6 RGB pages and 200 macro pages.
pub const SNAPSHOT_EXCHANGES: usize = 1 + 64 + 6 + 6 + 200;
/// Bytes carried by one reply.
pub const EXCHANGE_BYTES: usize = 64;

/// What a full snapshot is reading after `done` exchanges, in `snapshot` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Firmware,
    Keymap {
        layer: u8,
        fn_layer: bool,
    },
    Settings,
    Picture,
    /// Zero-based macro slot.
    Macro(u8),
    Done,
}

pub fn snapshot_phase(done: usize) -> Phase {
    match done {
        0 => Phase::Firmware,
        1..=64 => {
            let i = done - 1;
            Phase::Keymap {
                layer: (i / 16) as u8,
                fn_layer: i % 16 >= 8,
            }
        }
        65..=70 => Phase::Settings,
        71..=76 => Phase::Picture,
        77..=276 => Phase::Macro(((done - 77) / 4) as u8),
        _ => Phase::Done,
    }
}

pub struct Keyboard {
    channel: Channel,
    _lock: File,
    model: &'static crate::models::Model,
}

/// Lock the configuration node itself (no stale lockfile or /tmp path),
/// waiting up to `wait` for another short operation to finish.
pub fn lock_node(info: &DeviceInfo, wait: Duration) -> Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(info.path().to_str()?)
        .context("cannot open configuration node for exclusive access")?;
    let deadline = Instant::now() + wait;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(e) if Instant::now() >= deadline => {
                bail!("configuration device is busy: {e}")
            }
            Err(_) => sleep(Duration::from_millis(20)),
        }
    }
}

/// Wireless status of the keyboard: battery when online, `None` when asleep.
/// `Ok(None)` when neither a receiver nor a Bluetooth keyboard is present.
pub fn wireless_status(api: &HidApi) -> Result<Option<(Kind, Option<u8>)>> {
    let receiver = crate::dongle::status(api)?;
    if let Some(crate::dongle::Link::Online { battery }) = receiver {
        let keyboard = Keyboard::open_with_transport(api, None, Some(Kind::Dongle))?;
        let _ = keyboard.model();
        return Ok(Some((Kind::Dongle, Some(battery))));
    }
    if let Some(info) = api
        .device_list()
        .find(|d| transport::bluetooth_candidate(d))
    {
        // The vendor ID/collection is shared with other boards. Identify the
        // keyboard before presenting its status as an original M1W.
        let keyboard =
            Keyboard::open_with_transport(api, Some(info.path()), Some(Kind::Bluetooth))?;
        return Ok(Some((Kind::Bluetooth, keyboard.battery()?)));
    }
    Ok(receiver.map(|_| (Kind::Dongle, None)))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Onboard {
    pub debounce: u8,
    pub sleep: SleepTimers,
    /// Raw option, Fn and power bytes. Never round-trip through lossy Booleans.
    pub options: [u8; 3],
    pub auto_os: bool,
    pub rgb: Vec<u8>,
    /// All 50 raw 256-byte macro slots, including unreferenced slots.
    pub macros: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced: Option<crate::advanced::Settings>,
    pub format_version: u8,
    pub device_id: u32,
    pub usb_revision: u16,
    pub active_layer: u8,
    pub lighting: Lighting,
    /// Eight raw feature pages per layer, flattened to 512 bytes.
    pub keymaps: Vec<Vec<u8>>,
    pub fn_keymaps: Vec<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboard: Option<Onboard>,
}

impl Snapshot {
    pub fn model(&self) -> Result<&'static crate::models::Model> {
        crate::models::get(self.device_id)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.format_version, 1..=3),
            "unsupported snapshot format"
        );
        let model = self.model()?;
        model.layer(self.active_layer)?;
        for (maps, count) in [
            (&self.keymaps, model.base_maps()),
            (&self.fn_keymaps, model.fn_maps()),
        ] {
            ensure!(
                maps.len() == usize::from(count) && maps.iter().all(|m| m.len() == 512),
                "invalid keymap dimensions"
            );
        }
        if let Some(o) = &self.onboard {
            ensure!(
                o.rgb.len() == 384
                    && o.macros.len() == 50
                    && o.macros.iter().all(|m| m.len() == 256),
                "invalid RGB or macro dimensions"
            );
        }
        ensure!(
            self.format_version == 1 || self.onboard.is_some(),
            "incomplete version2 snapshot"
        );
        if let Some(settings) = &self.advanced {
            for (parameter, data) in &settings.magnetic {
                ensure!(
                    data.len() == crate::advanced::magnetic_bytes(model, *parameter)?,
                    "invalid magnetic parameter dimensions"
                );
            }
            if self.format_version == 3 {
                if model.family == crate::models::Family::Ry5088 {
                    ensure!(
                        settings.options.is_some() && settings.report_rate.is_some(),
                        "modern snapshot lacks options or report rate"
                    );
                }
                ensure!(
                    !model.side_light || settings.side_lighting.is_some(),
                    "snapshot lacks side lighting"
                );
                if model.magnetic {
                    ensure!(
                        matches!(settings.units_per_mm, 10 | 100 | 200 | 1000),
                        "invalid magnetic precision"
                    );
                    for parameter in [0, 1, 2, 3, 4, 5, 6, 7, 9, 10] {
                        ensure!(
                            settings.magnetic.contains_key(&parameter),
                            "snapshot lacks magnetic parameter {parameter}"
                        );
                    }
                    ensure!(
                        !model.replaceable_switches || settings.magnetic.contains_key(&252),
                        "snapshot lacks switch types"
                    );
                    ensure!(
                        model.family != crate::models::Family::Yc500
                            || settings.magnetic_mode.is_some(),
                        "snapshot lacks actuation preset"
                    );
                }
            }
        }
        ensure!(
            self.format_version < 3 || self.advanced.is_some(),
            "incomplete version3 snapshot"
        );
        Ok(())
    }

    pub fn inverse(&self, change: &Change) -> Result<Change> {
        self.validate()?;
        change.packets_for(self.model()?)?;
        let onboard = || {
            self.onboard
                .as_ref()
                .context("legacy snapshot lacks this setting")
        };
        Ok(match change {
            Change::Advanced { change } => Change::Advanced {
                change: change.inverse(
                    self.advanced
                        .as_ref()
                        .context("advanced settings not captured")?,
                )?,
            },
            Change::Profile { .. } => Change::Profile {
                layer: self.active_layer,
            },
            Change::Lighting { .. } => Change::Lighting {
                settings: self.lighting.clone(),
            },
            Change::Remap {
                layer,
                fn_layer,
                slot,
                ..
            } => {
                let maps = if *fn_layer {
                    &self.fn_keymaps
                } else {
                    &self.keymaps
                };
                Change::Remap {
                    layer: *layer,
                    fn_layer: *fn_layer,
                    slot: *slot,
                    binding: maps[usize::from(*layer)]
                        [usize::from(*slot) * 4..usize::from(*slot) * 4 + 4]
                        .try_into()?,
                }
            }
            Change::Debounce { .. } => Change::Debounce {
                value: onboard()?.debounce,
            },
            Change::Sleep { .. } => Change::Sleep {
                timers: onboard()?.sleep.clone(),
            },
            Change::Options { .. } => Change::Options {
                bytes: onboard()?.options,
            },
            Change::AutoOs { .. } => Change::AutoOs {
                enabled: onboard()?.auto_os,
            },
            Change::Macro { slot, .. } => Change::Macro {
                slot: *slot,
                data: onboard()?.macros[usize::from(*slot)].clone(),
            },
            Change::Rgb { .. } => Change::Rgb {
                data: onboard()?.rgb.clone(),
            },
        })
    }

    /// Restore only differences. Macros precede bindings; lighting/layer are last.
    pub fn restore_changes(&self, target: &Self) -> Result<Vec<Change>> {
        self.validate()?;
        target.validate()?;
        ensure!(
            self.device_id == target.device_id,
            "snapshot keyboard model mismatch"
        );
        ensure!(
            self.usb_revision == target.usb_revision,
            "snapshot firmware revision mismatch"
        );
        let mut changes = Vec::new();
        if let Some(wanted) = &target.advanced {
            let current = self
                .advanced
                .as_ref()
                .context("current snapshot lacks advanced settings")?;
            ensure!(
                !self.model()?.magnetic || current.units_per_mm == wanted.units_per_mm,
                "magnetic precision differs; capture a new snapshot before restoring"
            );
            changes.extend(
                current
                    .differences(wanted)
                    .into_iter()
                    .map(|change| Change::Advanced { change }),
            );
        }
        if let Some(t) = &target.onboard {
            let c = self
                .onboard
                .as_ref()
                .context("current snapshot lacks onboard data")?;
            for slot in 0..50 {
                if c.macros[slot] != t.macros[slot] {
                    changes.push(Change::Macro {
                        slot: slot as u8,
                        data: t.macros[slot].clone(),
                    });
                }
            }
            if c.rgb != t.rgb {
                changes.push(Change::Rgb {
                    data: t.rgb.clone(),
                });
            }
            if c.debounce != t.debounce {
                changes.push(Change::Debounce { value: t.debounce });
            }
            if c.sleep != t.sleep {
                changes.push(Change::Sleep {
                    timers: t.sleep.clone(),
                });
            }
            if c.options != t.options {
                changes.push(Change::Options { bytes: t.options });
            }
            if c.auto_os != t.auto_os {
                changes.push(Change::AutoOs { enabled: t.auto_os });
            }
        }
        for (fn_layer, maps, wanted) in [
            (false, &self.keymaps, &target.keymaps),
            (true, &self.fn_keymaps, &target.fn_keymaps),
        ] {
            for layer in 0..maps.len() {
                for slot in 0..128 {
                    let start = slot * 4;
                    if maps[layer][start..start + 4] != wanted[layer][start..start + 4] {
                        changes.push(Change::Remap {
                            layer: layer as u8,
                            fn_layer,
                            slot: slot as u8,
                            binding: wanted[layer][start..start + 4].try_into()?,
                        });
                    }
                }
            }
        }
        if self.lighting != target.lighting {
            changes.push(Change::Lighting {
                settings: target.lighting.clone(),
            });
        }
        if self.active_layer != target.active_layer {
            changes.push(Change::Profile {
                layer: target.active_layer,
            });
        }
        for c in &changes {
            c.packets_for(self.model()?)?;
        }
        Ok(changes)
    }
}

impl Keyboard {
    /// Open the keyboard over the first available transport: USB cable, then
    /// 2.4 GHz receiver, then Bluetooth. Identity is verified on every one.
    pub fn open(api: &HidApi, path: Option<&CStr>) -> Result<Self> {
        Self::open_with_transport(api, path, None)
    }

    /// Restrict discovery to an explicitly selected transport when requested.
    pub fn open_with_transport(
        api: &HidApi,
        path: Option<&CStr>,
        kind: Option<Kind>,
    ) -> Result<Self> {
        Self::open_reporting(api, path, kind, |_| {})
    }

    /// Report the selected transport before opening or querying it, including
    /// when opening/identity subsequently fails. Used for GUI connection warnings.
    pub fn open_reporting(
        api: &HidApi,
        path: Option<&CStr>,
        kind: Option<Kind>,
        on_selected: impl FnOnce(Kind),
    ) -> Result<Self> {
        let all: Vec<_> = api
            .device_list()
            .filter(|d| path.is_none_or(|p| p == d.path()))
            .filter_map(|d| transport::kind_of(d).map(|k| (d, k)))
            .filter(|(_, k)| kind.is_none_or(|selected| selected == *k))
            .collect();
        let Some(mut preferred) = [Kind::Usb, Kind::Dongle, Kind::Bluetooth]
            .into_iter()
            .find(|k| all.iter().any(|(_, kind)| kind == k))
        else {
            bail!(
                "no supported keyboard found over USB cable, 2.4 GHz receiver or Bluetooth; connect it and run `list`"
            );
        };
        if preferred == Kind::Dongle
            && kind.is_none()
            && path.is_none()
            && all.iter().any(|(_, k)| *k == Kind::Bluetooth)
            && crate::dongle::status(api)? == Some(crate::dongle::Link::Offline)
        {
            preferred = Kind::Bluetooth;
        }
        let kind = preferred;
        let matches: Vec<_> = all.iter().filter(|(_, k)| *k == kind).collect();
        ensure!(
            matches.len() == 1,
            "multiple {kind} candidates; select one with --device /dev/hidrawN"
        );
        on_selected(kind);
        let info = matches[0].0;
        let lock = lock_node(info, Duration::from_secs(1))?;
        let mut keyboard = Self {
            model: crate::models::original(),
            channel: Channel::open(api, info, kind)?,
            _lock: lock,
        };
        keyboard.model = crate::models::get(protocol::identify(&keyboard.query(0x8f, &[])?)?)?;
        Ok(keyboard)
    }

    pub fn model(&self) -> &'static crate::models::Model {
        self.model
    }

    fn modern(&self) -> bool {
        self.model.family == crate::models::Family::Ry5088
    }

    pub fn kind(&self) -> Kind {
        self.channel.kind()
    }

    /// Battery in percent over a wireless transport; `None` over the cable.
    pub fn battery(&self) -> Result<Option<u8>> {
        self.channel.battery()
    }

    /// Count exchanges from now on, for progress against `SNAPSHOT_EXCHANGES`.
    pub fn count_exchanges(&mut self, counter: Arc<AtomicUsize>) {
        self.channel.count_exchanges(counter);
    }

    fn send(&self, data: &Packet) -> Result<()> {
        self.channel.send(data)
    }

    fn exchange(&self, data: &Packet) -> Result<Packet> {
        self.channel.exchange(data)
    }

    fn query(&self, opcode: u8, args: &[u8]) -> Result<Packet> {
        let mut header = vec![opcode];
        header.extend_from_slice(args);
        let request = packet(&header, 7);
        let mut response = self.exchange(&request)?;
        // Radio links can drop a reply. Queries are reads, so re-asking is safe.
        for _ in 0..2 {
            if response[0] == opcode || self.kind() == Kind::Usb {
                break;
            }
            sleep(Duration::from_millis(50));
            response = self.exchange(&request)?;
        }
        ensure!(
            response[0] == opcode,
            "expected response {opcode:02x}, got {:02x}; stop other keyboard software and replug",
            response[0]
        );
        Ok(response)
    }

    pub fn active_layer(&self) -> Result<u8> {
        let value = self.query(if self.modern() { 0x84 } else { 0x85 }, &[])?[1];
        self.model.layer(if self.model.magnetic && !self.modern() {
            value / 4
        } else {
            value
        })
    }

    pub fn lighting(&self) -> Result<Lighting> {
        Lighting::decode(&self.query(0x87, &[])?)
    }

    pub fn debounce(&self) -> Result<u8> {
        ensure!(self.model.debounce, "debounce unavailable on this model");
        Ok(if self.modern() {
            self.query(0x86, &[])?[1]
        } else {
            self.query(0x91, &[])?[2]
        })
    }

    pub fn sleep_timers(&self) -> Result<SleepTimers> {
        ensure!(self.model.sleep, "wireless sleep unavailable on this model");
        if self.modern() {
            let r = self.query(0x91, &[])?;
            let value = |i| u16::from_le_bytes([r[i], r[i + 1]]);
            Ok(SleepTimers {
                time_bt: value(8),
                time_24: value(10),
                deep_time_bt: value(12),
                deep_time_24: value(14),
            })
        } else {
            SleepTimers::decode(&self.query(0x92, &[])?)
        }
    }

    pub fn options(&self) -> Result<[u8; 3]> {
        ensure!(!self.modern(), "this model uses modern-options");
        Ok(self.query(0x86, &[0])?[2..5].try_into()?)
    }

    pub fn auto_os(&self) -> Result<bool> {
        let r = self.query(0x97, &[])?;
        ensure!(r[1] <= 1, "invalid auto-OS response");
        Ok(r[1] == 1)
    }

    pub fn advanced_settings(&self, revision: u16) -> Result<crate::advanced::Settings> {
        use crate::advanced::Settings;
        let mut settings = Settings {
            units_per_mm: 10,
            ..Default::default()
        };
        if self.modern() {
            let rf = self.query(0x80, &[])?;
            let rf = u16::from_le_bytes([rf[1], rf[2]]);
            settings.rf_revision = (rf != 0).then_some(rf);
            settings.units_per_mm =
                crate::advanced::legacy_precision(revision, settings.rf_revision);
            if self.model.magnetic {
                let features = self.exchange(&packet(&[0xe6], 7))?;
                if features[0] == 0xe6 && features[1] == 0xaa {
                    settings.units_per_mm = match features[2] {
                        0 => 100,
                        1 => 200,
                        2 => 1000,
                        _ => anyhow::bail!("unknown magnetic precision"),
                    };
                    if features[3] == 1 {
                        let output = self.query(0x9d, &[])?;
                        ensure!(
                            output[1] <= 1,
                            "unknown output mode; select keyboard mode in the vendor driver first"
                        );
                        settings.output_mode = Some(output[1]);
                    }
                }
            }
            settings.options = Some(self.query(0x89, &[])?[2..6].try_into()?);
            let code = self.query(0x83, &[])?[2];
            settings.report_rate = Some(
                *[8000, 4000, 2000, 1000, 500, 250, 125]
                    .get(usize::from(code))
                    .context("invalid report-rate reply")?,
            );
        }
        if self.model.side_light {
            let mut response = self.query(0x88, &[])?;
            response[0] = 0x87;
            settings.side_lighting = Some(Lighting::decode(&response)?);
        }
        if self.model.magnetic {
            for parameter in [0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 252, 251] {
                if parameter == 252 && !self.model.replaceable_switches {
                    continue;
                }
                if parameter == 251
                    && (!self.modern() || revision.max(settings.rf_revision.unwrap_or(0)) < 0x0400)
                {
                    continue;
                }
                let size = crate::advanced::magnetic_bytes(self.model, parameter)?;
                let mut data = Vec::with_capacity(size);
                for page in 0..size / 64 {
                    data.extend_from_slice(
                        &self.exchange(&packet(&[0xe5, parameter, 1, page as u8], 7))?,
                    );
                }
                settings.magnetic.insert(parameter, data);
            }
            if !self.modern() {
                let mode = self.query(0x9d, &[])?[1];
                ensure!(mode <= 3, "invalid magnetic preset reply");
                settings.magnetic_mode = Some(mode);
            }
        }
        Ok(settings)
    }

    fn raw_pages(&self, opcode: u8, selector: u8, pages: u8) -> Result<Vec<u8>> {
        let mut data = Vec::with_capacity(usize::from(pages) * 64);
        for page in 0..pages {
            let request = packet(&[opcode, selector, page], 7);
            let reply = self.exchange(&request)?;
            ensure!(
                reply != request,
                "raw page query echoed request: opcode {opcode:02x}, page {page}"
            );
            data.extend_from_slice(&reply);
        }
        Ok(data)
    }

    pub fn macro_data(&self, slot: u8) -> Result<Vec<u8>> {
        ensure!(slot < 50, "macro slot must be 0..49");
        self.raw_pages(0x8b, slot, 4)
    }

    pub fn rgb(&self) -> Result<Vec<u8>> {
        if self.modern() {
            let picture = crate::transaction::picture(&self.lighting()?);
            (0..6)
                .map(|page| {
                    self.exchange(&packet(&[0x8c, picture, 255, page], 7))
                        .map(|p| p.to_vec())
                })
                .collect::<Result<Vec<_>>>()
                .map(|p| p.concat())
        } else {
            self.raw_pages(0x8c, 0, 6)
        }
    }

    pub fn keymap(&self, layer: u8, fn_layer: bool) -> Result<Vec<u8>> {
        ensure!(
            layer
                < if fn_layer {
                    self.model.fn_maps()
                } else {
                    self.model.base_maps()
                },
            "map out of range"
        );
        if self.modern() {
            (0..8)
                .map(|page| {
                    let header = if fn_layer {
                        let (system, index) = self.model.fn_address(layer)?;
                        vec![0x90, system, index, 255, page]
                    } else {
                        vec![
                            0x8a,
                            if self.model.magnetic {
                                layer / 4
                            } else {
                                layer
                            },
                            255,
                            page,
                            if self.model.magnetic { layer % 4 } else { 0 },
                        ]
                    };
                    self.exchange(&packet(&header, 7)).map(|p| p.to_vec())
                })
                .collect::<Result<Vec<_>>>()
                .map(|p| p.concat())
        } else {
            self.raw_pages(if fn_layer { 0x90 } else { 0x89 }, layer, 8)
        }
    }

    pub fn firmware_revision(&self) -> Result<u16> {
        let reply = self.query(if self.modern() { 0x8f } else { 0x80 }, &[])?;
        Ok(if self.modern() {
            u16::from_le_bytes([reply[7], reply[8]])
        } else {
            u16::from_le_bytes([reply[1], reply[2]])
        })
    }

    /// Explicit calibration session; callers journal a fresh snapshot first.
    pub fn calibrate(&self, start: bool) -> Result<()> {
        ensure!(self.model.magnetic, "calibration unavailable on this model");
        ensure!(
            self.kind() == Kind::Usb,
            "use the USB cable for calibration"
        );
        ensure!(
            protocol::identify(&self.query(0x8f, &[])?)? == self.model.id,
            "keyboard identity changed"
        );
        if start {
            self.send(&packet(&[28, 1], 7))?;
            sleep(Duration::from_secs(4));
            self.send(&packet(&[28, 0], 7))?;
            sleep(Duration::from_secs(2));
            self.send(&packet(&[30, 1], 7))?;
        } else {
            self.send(&packet(&[30, 0], 7))?;
        }
        sleep(Duration::from_secs(2));
        Ok(())
    }

    /// Live travel on modern boards; normalized calibration range on YC500.
    pub fn travel(&self) -> Result<Vec<f32>> {
        ensure!(
            self.model.magnetic,
            "travel reporting unavailable on this model"
        );
        if self.modern() {
            let revision = self.firmware_revision()?;
            let rf = self.query(0x80, &[])?;
            let mut precision = f32::from(crate::advanced::legacy_precision(
                revision,
                Some(u16::from_le_bytes([rf[1], rf[2]])),
            ));
            let features = self.exchange(&packet(&[0xe6], 7))?;
            if features[0] == 0xe6 && features[1] == 0xaa {
                precision = match features[2] {
                    0 => 100.,
                    1 => 200.,
                    2 => 1000.,
                    _ => anyhow::bail!("unknown travel precision"),
                };
            }
            let mut bytes = Vec::new();
            for page in 0..4 {
                bytes.extend_from_slice(&self.exchange(&packet(&[0xe5, 254, 1, page], 7))?);
            }
            Ok(bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| f32::from(u16::from_le_bytes([v[0], v[1]])) / precision)
                .collect())
        } else {
            let mut ranges = Vec::new();
            for opcode in [0x9e, 0x9c] {
                let mut values = Vec::new();
                for page in 0..4 {
                    values.extend_from_slice(&self.exchange(&packet(&[opcode, page], 7))?);
                }
                ranges.push(
                    values
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|v| u16::from_le_bytes([v[0], v[1]]))
                        .collect::<Vec<_>>(),
                );
            }
            Ok(ranges[0]
                .iter()
                .zip(&ranges[1])
                .map(|(max, min)| (f32::from(max.abs_diff(*min)) / 400.).min(1.))
                .collect())
        }
    }

    pub(crate) fn enter_firmware_boot(&self) -> Result<()> {
        ensure!(
            self.kind() == Kind::Usb,
            "firmware updates require the USB cable"
        );
        self.send(&crate::firmware::boot_packet())
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.snapshot_reporting(|_, _| {})
    }

    pub fn snapshot_reporting(&self, mut progress: impl FnMut(&str, usize)) -> Result<Snapshot> {
        let modern = self.modern();
        let base_queries = 1
            + usize::from(self.model.base_maps() + self.model.fn_maps()) * 8
            + 3
            + 6
            + usize::from(modern)
            + 200
            + usize::from(self.model.debounce)
            + usize::from(self.model.sleep)
            + usize::from(!modern);
        progress("Firmware", base_queries);
        let revision = self.firmware_revision()?;
        progress("Model settings", base_queries);
        let advanced = self.advanced_settings(revision)?;
        let total = base_queries
            + if modern {
                3 + usize::from(self.model.magnetic)
            } else {
                0
            }
            + usize::from(advanced.output_mode.is_some())
            + usize::from(advanced.side_lighting.is_some())
            + advanced
                .magnetic
                .values()
                .map(|data| data.len() / 64)
                .sum::<usize>()
            + usize::from(advanced.magnetic_mode.is_some());
        let mut keymaps = Vec::new();
        let mut fn_keymaps = Vec::new();
        for layer in 0..self.model.base_maps().max(self.model.fn_maps()) {
            if layer < self.model.base_maps() {
                progress(&format!("Base map {}", layer + 1), total);
                keymaps.push(self.keymap(layer, false)?);
            }
            if layer < self.model.fn_maps() {
                progress(&format!("Fn map {}", layer + 1), total);
                fn_keymaps.push(self.keymap(layer, true)?);
            }
        }
        progress("Keyboard settings", total);
        let active_layer = self.active_layer()?;
        let lighting = self.lighting()?;
        let debounce = if self.model.debounce {
            self.debounce()?
        } else {
            0
        };
        let sleep = if self.model.sleep {
            self.sleep_timers()?
        } else {
            SleepTimers {
                time_bt: 0,
                time_24: 0,
                deep_time_bt: 0,
                deep_time_24: 0,
            }
        };
        let options = if modern { [0; 3] } else { self.options()? };
        let auto_os = self.auto_os()?;
        progress("Custom picture", total);
        let rgb = self.rgb()?;
        let mut macros = Vec::new();
        for slot in 0..50 {
            progress(&format!("Macro {} of 50", slot + 1), total);
            macros.push(self.macro_data(slot)?);
        }
        progress("Finishing", total);
        Ok(Snapshot {
            advanced: Some(advanced),
            format_version: 3,
            device_id: self.model.id,
            usb_revision: revision,
            active_layer,
            lighting,
            keymaps,
            fn_keymaps,
            onboard: Some(Onboard {
                debounce,
                sleep,
                options,
                auto_os,
                rgb,
                macros,
            }),
        })
    }

    /// Re-read only what `changes` touch (plus revision, active layer and
    /// lighting, which writes may temporarily alter) on top of `base`.
    /// Over 2.4 GHz a full snapshot takes about 30 seconds; this takes seconds.
    pub fn refresh(&self, base: &Snapshot, changes: &[Change]) -> Result<Snapshot> {
        // Advanced arrays are captured together so interdependent modes,
        // travel thresholds and DKS bodies have a consistent stale-state check.
        if changes.iter().any(|c| matches!(c, Change::Advanced { .. })) {
            ensure!(
                base.device_id == self.model.id,
                "baseline keyboard model mismatch"
            );
            return self.snapshot();
        }
        #[derive(PartialEq, Eq, PartialOrd, Ord)]
        enum Region {
            Debounce,
            Sleep,
            Options,
            AutoOs,
            Rgb,
            Macro(u8),
            Keymap(u8, bool),
        }
        let mut regions = BTreeSet::new();
        for change in changes {
            match change {
                Change::Advanced { .. } => unreachable!("handled by full refresh"),
                Change::Profile { .. } | Change::Lighting { .. } => {}
                Change::Debounce { .. } => _ = regions.insert(Region::Debounce),
                Change::Sleep { .. } => _ = regions.insert(Region::Sleep),
                Change::Options { .. } => _ = regions.insert(Region::Options),
                Change::AutoOs { .. } => _ = regions.insert(Region::AutoOs),
                Change::Rgb { .. } => _ = regions.insert(Region::Rgb),
                Change::Macro { slot, .. } => {
                    regions.insert(Region::Macro(*slot));
                    // A macro write can disturb picture 0; see `apply`.
                    regions.insert(Region::Rgb);
                }
                Change::Remap {
                    layer, fn_layer, ..
                } => _ = regions.insert(Region::Keymap(*layer, *fn_layer)),
            }
        }
        base.validate()?;
        ensure!(
            base.device_id == self.model.id,
            "baseline keyboard model mismatch"
        );
        let mut s = base.clone();
        s.usb_revision = self.firmware_revision()?;
        s.active_layer = self.active_layer()?;
        s.lighting = self.lighting()?;
        for region in regions {
            if let Region::Keymap(layer, fn_layer) = region {
                let map = self.keymap(layer, fn_layer)?;
                let maps = if fn_layer {
                    &mut s.fn_keymaps
                } else {
                    &mut s.keymaps
                };
                maps[usize::from(layer)] = map;
                continue;
            }
            let onboard = s
                .onboard
                .as_mut()
                .context("snapshot lacks onboard settings; read the keyboard again")?;
            match region {
                Region::Debounce => onboard.debounce = self.debounce()?,
                Region::Sleep => onboard.sleep = self.sleep_timers()?,
                Region::Options => onboard.options = self.options()?,
                Region::AutoOs => onboard.auto_os = self.auto_os()?,
                Region::Rgb => onboard.rgb = self.rgb()?,
                Region::Macro(slot) => onboard.macros[usize::from(slot)] = self.macro_data(slot)?,
                Region::Keymap(..) => unreachable!(),
            }
        }
        Ok(s)
    }

    /// Caller must save the before-state before invoking an experimental write.
    pub fn apply(&self, change: &Change) -> Result<()> {
        let mut packets = change.packets_for(self.model)?;
        // Recheck identity immediately before each write, even after a snapshot.
        ensure!(
            protocol::identify(&self.query(0x8f, &[])?)? == self.model.id,
            "keyboard identity changed"
        );
        if self.modern() && matches!(change, Change::Rgb { .. }) {
            let picture = crate::transaction::picture(&self.lighting()?);
            for p in &mut packets {
                p[1] = picture;
                p[7] = 0;
                *p = packet(p, 7);
            }
        }
        // Revision0108 ignores the single-binding packet's layer selector and
        // writes the active map. Select the requested layer and restore it only
        // after a verified write; on failure retain the saved state for inspection.
        let original_layer = if let Change::Remap { layer, .. } = change
            && self.model.id == 983
        {
            let original = self.active_layer()?;
            if original != *layer {
                self.apply(&Change::Profile { layer: *layer })?;
                Some(original)
            } else {
                None
            }
        } else {
            None
        };
        // Macro49 affects picture0 even when another picture is selected. GET
        // follows selection, so explicitly select0 before capturing its bytes.
        // Restore the previous effect only after macro and picture verification.
        let original_lighting = if matches!(change, Change::Macro { .. }) && self.model.id == 983 {
            let original = self.lighting()?;
            let picture0 = Lighting {
                mode: 13,
                wire_speed: original.wire_speed,
                brightness: original.brightness,
                flags: 0,
                rgb: [0, 200, 200],
            };
            if original != picture0 {
                self.apply(&Change::Lighting { settings: picture0 })?;
                Some(original)
            } else {
                None
            }
        } else {
            None
        };
        // Preserve the entire picture, not just a known slot or guessed default.
        let macro_rgb = if matches!(change, Change::Macro { .. }) && self.model.id == 983 {
            Some(self.rgb()?)
        } else {
            None
        };
        for packet in packets {
            self.send(&packet)?;
        }
        // Vendor CommonKeyboard.defaultVendorSleepTime is 2000 ms.
        sleep(Duration::from_secs(2));
        if let Some(original) = macro_rgb
            && self.rgb()? != original
        {
            eprintln!(
                "Macro write changed RGB storage; preserving the captured picture before verification."
            );
            for packet in crate::config::rgb_packets(&original)? {
                self.send(&packet)?;
            }
            sleep(Duration::from_secs(2));
            ensure!(
                self.rgb()? == original,
                "macro stored but RGB preservation failed; retain before-state"
            );
        }
        let matched = match change {
            Change::Advanced { change } => {
                change.inverse(&self.advanced_settings(self.firmware_revision()?)?)? == *change
            }
            Change::Profile { layer } => self.active_layer()? == *layer,
            Change::Lighting { settings } => self.lighting()? == *settings,
            Change::Debounce { value } => self.debounce()? == *value,
            Change::Sleep { timers } => self.sleep_timers()? == *timers,
            Change::Options { bytes } => self.options()? == *bytes,
            Change::AutoOs { enabled } => self.auto_os()? == *enabled,
            Change::Macro { slot, data } => self.macro_data(*slot)? == *data,
            Change::Rgb { data } => self.rgb()? == *data,
            Change::Remap {
                layer,
                fn_layer,
                slot,
                binding,
            } => {
                let map = self.keymap(*layer, *fn_layer)?;
                map[usize::from(*slot) * 4..usize::from(*slot) * 4 + 4] == *binding
            }
        };
        ensure!(
            matched,
            "write sent but read-back differs; state is uncertain. Do not retry automatically; retain the before-state snapshot"
        );
        if let Some(layer) = original_layer {
            self.apply(&Change::Profile { layer })
                .context("binding verified but original active layer could not be restored")?;
        }
        if let Some(settings) = original_lighting {
            self.apply(&Change::Lighting { settings })
                .context("macro verified but original lighting could not be restored")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod phase_tests {
    use super::*;

    #[test]
    fn phases_follow_snapshot_order() {
        assert_eq!(snapshot_phase(0), Phase::Firmware);
        assert_eq!(
            snapshot_phase(1),
            Phase::Keymap {
                layer: 0,
                fn_layer: false
            }
        );
        assert_eq!(
            snapshot_phase(9),
            Phase::Keymap {
                layer: 0,
                fn_layer: true
            }
        );
        assert_eq!(
            snapshot_phase(64),
            Phase::Keymap {
                layer: 3,
                fn_layer: true
            }
        );
        assert_eq!(snapshot_phase(65), Phase::Settings);
        assert_eq!(snapshot_phase(71), Phase::Picture);
        assert_eq!(snapshot_phase(77), Phase::Macro(0));
        assert_eq!(snapshot_phase(276), Phase::Macro(49));
        assert_eq!(snapshot_phase(SNAPSHOT_EXCHANGES), Phase::Done);
    }
}
