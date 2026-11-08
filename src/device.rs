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
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.format_version, 1 | 2) && self.device_id == protocol::DEVICE_ID,
            "not a supported original M1W snapshot"
        );
        protocol::layer(self.active_layer)?;
        for maps in [&self.keymaps, &self.fn_keymaps] {
            ensure!(
                maps.len() == 4 && maps.iter().all(|m| m.len() == 512),
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
            self.format_version != 2 || self.onboard.is_some(),
            "incomplete version2 snapshot"
        );
        Ok(())
    }

    pub fn inverse(&self, change: &Change) -> Result<Change> {
        self.validate()?;
        change.packets()?;
        let onboard = || {
            self.onboard
                .as_ref()
                .context("legacy snapshot lacks this setting")
        };
        Ok(match change {
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
            self.usb_revision == target.usb_revision,
            "snapshot firmware revision mismatch"
        );
        let mut changes = Vec::new();
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
            for layer in 0..4 {
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
            c.packets()?;
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
                "no original M1W found over USB cable, 2.4 GHz receiver or Bluetooth; connect it and run `list`"
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
        let keyboard = Self {
            channel: Channel::open(api, info, kind)?,
            _lock: lock,
        };
        protocol::identify(&keyboard.query(0x8f, &[])?)?;
        Ok(keyboard)
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
        protocol::layer(self.query(0x85, &[])?[1])
    }

    pub fn lighting(&self) -> Result<Lighting> {
        Lighting::decode(&self.query(0x87, &[])?)
    }

    pub fn debounce(&self) -> Result<u8> {
        Ok(self.query(0x91, &[])?[2])
    }

    pub fn sleep_timers(&self) -> Result<SleepTimers> {
        SleepTimers::decode(&self.query(0x92, &[])?)
    }

    pub fn options(&self) -> Result<[u8; 3]> {
        Ok(self.query(0x86, &[0])?[2..5].try_into()?)
    }

    pub fn auto_os(&self) -> Result<bool> {
        let r = self.query(0x97, &[])?;
        ensure!(r[1] <= 1, "invalid auto-OS response");
        Ok(r[1] == 1)
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
        self.raw_pages(0x8c, 0, 6)
    }

    pub fn keymap(&self, layer: u8, fn_layer: bool) -> Result<Vec<u8>> {
        protocol::layer(layer)?;
        self.raw_pages(if fn_layer { 0x90 } else { 0x89 }, layer, 8)
    }

    pub fn firmware_revision(&self) -> Result<u16> {
        let reply = self.query(0x80, &[])?;
        Ok(u16::from_le_bytes([reply[1], reply[2]]))
    }

    pub(crate) fn enter_firmware_boot(&self) -> Result<()> {
        ensure!(
            self.kind() == Kind::Usb,
            "firmware updates require the USB cable"
        );
        self.send(&crate::firmware::boot_packet())
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let revision = self.query(0x80, &[])?;
        let mut keymaps = Vec::new();
        let mut fn_keymaps = Vec::new();
        for layer in 0..4 {
            keymaps.push(self.keymap(layer, false)?);
            fn_keymaps.push(self.keymap(layer, true)?);
        }
        Ok(Snapshot {
            format_version: 2,
            device_id: protocol::DEVICE_ID,
            usb_revision: u16::from_le_bytes([revision[1], revision[2]]),
            active_layer: self.active_layer()?,
            lighting: self.lighting()?,
            keymaps,
            fn_keymaps,
            onboard: Some(Onboard {
                debounce: self.debounce()?,
                sleep: self.sleep_timers()?,
                options: self.options()?,
                auto_os: self.auto_os()?,
                rgb: self.rgb()?,
                macros: (0..50)
                    .map(|slot| self.macro_data(slot))
                    .collect::<Result<_>>()?,
            }),
        })
    }

    /// Re-read only what `changes` touch (plus revision, active layer and
    /// lighting, which writes may temporarily alter) on top of `base`.
    /// Over 2.4 GHz a full snapshot takes about 30 seconds; this takes seconds.
    pub fn refresh(&self, base: &Snapshot, changes: &[Change]) -> Result<Snapshot> {
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
        let mut s = base.clone();
        let revision = self.query(0x80, &[])?;
        s.usb_revision = u16::from_le_bytes([revision[1], revision[2]]);
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
        let packets = change.packets()?;
        // Recheck identity immediately before each write, even after a snapshot.
        protocol::identify(&self.query(0x8f, &[])?)?;
        // Revision0108 ignores the single-binding packet's layer selector and
        // writes the active map. Select the requested layer and restore it only
        // after a verified write; on failure retain the saved state for inspection.
        let original_layer = if let Change::Remap { layer, .. } = change {
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
        let original_lighting = if matches!(change, Change::Macro { .. }) {
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
        let macro_rgb = if matches!(change, Change::Macro { .. }) {
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
