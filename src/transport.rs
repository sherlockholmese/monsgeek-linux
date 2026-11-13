//! Byte transports to the keyboard. The 64-byte command packets are identical
//! on every transport; only the framing differs.
//!
//! - USB cable: SET/GET feature report on interface 2.
//! - 2.4 GHz receiver: the same feature reports, gated by the receiver's status
//!   (`0xF7`). Wait for can-send, send; for replies poll until can-read, notify
//!   with `0xFC`, then GET the reply. `0xF6 0x0A` routes traffic to the keyboard.
//!   Verified on receiver 3151:4011 with reads and a no-op write.
//! - Bluetooth: output report `[0x55, packet…]`; replies arrive as input reports
//!   with the same marker. `0x77` requests battery, answered as `[0x77, %]`;
//!   `0x88` means the keyboard went offline. From the vendor host; not yet
//!   verified on this keyboard.
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use hidapi::{BusType, DeviceInfo, HidApi, HidDevice};

use crate::{dongle, protocol::Packet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Kind {
    Usb,
    #[value(name = "receiver")]
    Dongle,
    Bluetooth,
}

pub const BLUETOOTH_WARNING: &str =
    "Experimental Bluetooth. Untested on hardware. Use USB if it fails.";

impl Kind {
    pub fn warning(self) -> Option<&'static str> {
        (self == Self::Bluetooth).then_some(BLUETOOTH_WARNING)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Usb => "USB",
            Kind::Dongle => "2.4 GHz",
            Kind::Bluetooth => "Bluetooth",
        })
    }
}

pub fn usb_candidate(info: &DeviceInfo) -> bool {
    crate::models::MODELS
        .iter()
        .any(|m| m.vid == info.vendor_id() && m.pid == info.product_id())
        && matches!(info.bus_type(), BusType::Usb)
        && info.usage_page() == 0xffff
        && info.usage() == 2
        && info.interface_number() == 2
}

/// Vendor configuration collection of a MonsGeek Bluetooth LE keyboard.
pub fn bluetooth_candidate(info: &DeviceInfo) -> bool {
    crate::models::MODELS
        .iter()
        .any(|m| m.vid == info.vendor_id())
        && matches!(info.bus_type(), BusType::Bluetooth)
        && matches!(info.usage_page(), 0xff55 | 0xff66)
        && info.usage() == 0x0202
}

pub fn kind_of(info: &DeviceInfo) -> Option<Kind> {
    if usb_candidate(info) {
        Some(Kind::Usb)
    } else if dongle::candidate(info) {
        Some(Kind::Dongle)
    } else if bluetooth_candidate(info) {
        Some(Kind::Bluetooth)
    } else {
        None
    }
}

/// Report layout of the Bluetooth vendor collection. Lengths exclude report IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reports {
    pub output_id: u8,
    pub output_len: usize,
    pub input_id: u8,
    pub input_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureReport {
    pub id: u8,
    pub len: usize,
}

/// Feature report IDs belong to a descriptor, not a keyboard model or PID.
pub fn parse_feature_report(descriptor: &[u8]) -> Result<FeatureReport> {
    let (mut page, mut id, mut size, mut count) = (0u32, 0u8, 0usize, 0usize);
    let mut stack = Vec::new();
    let mut bits = BTreeMap::<u8, usize>::new();
    let mut vendor = BTreeSet::new();
    let mut i = 0;
    while i < descriptor.len() {
        let prefix = descriptor[i];
        if prefix == 0xfe {
            let len = usize::from(*descriptor.get(i + 1).context("truncated long HID item")?);
            ensure!(i + 3 + len <= descriptor.len(), "truncated long HID item");
            i += 3 + len;
            continue;
        }
        let len = [0, 1, 2, 4][usize::from(prefix & 3)];
        let data = descriptor
            .get(i + 1..i + 1 + len)
            .context("truncated HID descriptor")?;
        let value = data
            .iter()
            .rev()
            .fold(0u32, |v, b| (v << 8) | u32::from(*b));
        match prefix & 0xfc {
            0x04 => page = value,
            0x74 => size = value as usize,
            0x84 => {
                ensure!((1..=255).contains(&value), "invalid report ID");
                id = value as u8;
            }
            0x94 => count = value as usize,
            0xa4 => stack.push((page, id, size, count)),
            0xb4 => (page, id, size, count) = stack.pop().context("unmatched HID global pop")?,
            0xb0 => {
                let total = bits.entry(id).or_default();
                *total = total
                    .checked_add(
                        size.checked_mul(count)
                            .context("feature report too large")?,
                    )
                    .context("feature report too large")?;
                ensure!(*total <= 4096 * 8, "feature report too large");
                if page >= 0xff00 {
                    vendor.insert(id);
                }
            }
            _ => {}
        }
        i += 1 + len;
    }
    ensure!(stack.is_empty(), "unclosed HID global scope");
    let ids: Vec<_> = if vendor.is_empty() {
        bits.keys().copied().collect()
    } else {
        vendor.into_iter().collect()
    };
    let reports: Vec<_> = ids
        .into_iter()
        .filter_map(|id| {
            let len = bits[&id].div_ceil(8);
            (len == 64).then_some(FeatureReport { id, len })
        })
        .collect();
    ensure!(
        reports.len() == 1,
        "expected one 64-byte vendor feature report, found {}",
        reports.len()
    );
    Ok(reports[0])
}

pub fn feature_report(device: &HidDevice) -> Result<FeatureReport> {
    let mut descriptor = [0; 4096];
    let n = device
        .get_report_descriptor(&mut descriptor)
        .context("cannot read HID feature descriptor")?;
    parse_feature_report(&descriptor[..n])
}

pub fn decode_feature_report(wire: &[u8], report: FeatureReport) -> Result<Packet> {
    if wire.len() == report.len + 1 {
        ensure!(wire[0] == report.id, "unexpected feature report ID");
        Ok(wire[1..].try_into()?)
    } else {
        crate::protocol::decode_feature(wire)
    }
}

/// Recover complete report sizes, including padding and split main items.
/// The reference host uses report 6 with 65 payload bytes. Read the actual
/// descriptor rather than assuming that layout on every firmware revision.
pub fn parse_reports(descriptor: &[u8]) -> Result<Reports> {
    let (mut page, mut id, mut size, mut count) = (0u32, 0u8, 0usize, 0usize);
    let mut stack = Vec::new();
    let mut bits = BTreeMap::<(u8, u8), usize>::new();
    let mut vendor = BTreeSet::new();
    let mut collections = Vec::new();
    let mut i = 0;
    while i < descriptor.len() {
        let prefix = descriptor[i];
        if prefix == 0xfe {
            let len = usize::from(*descriptor.get(i + 1).context("truncated long HID item")?);
            ensure!(i + 3 + len <= descriptor.len(), "truncated long HID item");
            i += 3 + len;
            continue;
        }
        let len = [0, 1, 2, 4][usize::from(prefix & 3)];
        let data = descriptor
            .get(i + 1..i + 1 + len)
            .context("truncated report descriptor")?;
        let value = data
            .iter()
            .rev()
            .fold(0u32, |v, b| (v << 8) | u32::from(*b));
        match prefix & 0xfc {
            0x04 => page = value,
            0x74 => size = value as usize,
            0x84 => {
                ensure!((1..=255).contains(&value), "invalid HID report ID");
                id = value as u8;
            }
            0x94 => count = value as usize,
            0xa4 => stack.push((page, id, size, count)),
            0xb4 => (page, id, size, count) = stack.pop().context("unmatched HID global pop")?,
            0xa0 => collections.push(
                matches!(page, 0xff55 | 0xff66) || collections.last().copied().unwrap_or(false),
            ),
            0xc0 => {
                collections.pop().context("unmatched HID collection end")?;
            }
            direction @ (0x80 | 0x90) => {
                let key = (direction, id);
                let total = bits.entry(key).or_default();
                *total = total
                    .checked_add(size.checked_mul(count).context("HID report too large")?)
                    .context("HID report too large")?;
                ensure!(*total <= 4096 * 8, "HID report too large");
                if collections.last().copied().unwrap_or(false) {
                    vendor.insert(key);
                }
            }
            _ => {}
        }
        i += 1 + len;
    }
    ensure!(
        stack.is_empty() && collections.is_empty(),
        "unclosed HID descriptor scope"
    );
    let find = |direction| -> Result<(u8, usize)> {
        let candidates: Vec<_> = vendor
            .iter()
            .filter(|(d, _)| *d == direction)
            .map(|key| (key.1, bits[key].div_ceil(8)))
            .filter(|(_, len)| *len >= 65)
            .collect();
        ensure!(
            candidates.len() == 1,
            "expected one Bluetooth vendor {} report of at least 65 bytes, found {}",
            if direction == 0x80 { "input" } else { "output" },
            candidates.len()
        );
        Ok(candidates[0])
    };
    let (output_id, output_len) = find(0x90)?;
    let (input_id, input_len) = find(0x80)?;
    Ok(Reports {
        output_id,
        output_len,
        input_id,
        input_len,
    })
}

impl Reports {
    fn encode(self, payload: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            payload.len() <= self.output_len,
            "command does not fit the {}-byte Bluetooth report",
            self.output_len
        );
        let mut wire = vec![0; self.output_len + 1];
        wire[0] = self.output_id;
        wire[1..1 + payload.len()].copy_from_slice(payload);
        Ok(wire)
    }

    fn payload(self, wire: &[u8]) -> Option<&[u8]> {
        if self.input_id == 0 {
            Some(wire)
        } else {
            (wire.first() == Some(&self.input_id)).then(|| &wire[1..])
        }
    }
}

fn bluetooth_packet(reply: &[u8]) -> Result<Packet> {
    ensure!(
        reply.len() >= 64,
        "short Bluetooth command reply: {} of 64 bytes",
        reply.len()
    );
    Ok(reply[..64].try_into()?)
}

/// Query-only retry policy. Setting writes use `send` exactly once.
fn bluetooth_exchange(
    mut send: impl FnMut() -> Result<()>,
    mut read: impl FnMut() -> Result<Option<Vec<u8>>>,
) -> Result<Packet> {
    // Reference commonMsg: initial request plus at most three unanswered retries.
    for _ in 0..4 {
        send()?;
        if let Some(reply) = read()? {
            return bluetooth_packet(&reply);
        }
    }
    bail!("no reply over Bluetooth; wake the keyboard or use USB")
}

pub struct Channel {
    device: HidDevice,
    kind: Kind,
    reports: Option<Reports>,
    feature: FeatureReport,
    exchanges: Option<Arc<AtomicUsize>>,
    last_bt_send: Mutex<Option<Instant>>,
}

const BT_MARK: u8 = 0x55;
static BT_WARNING_SHOWN: AtomicBool = AtomicBool::new(false);

impl Channel {
    pub fn open(api: &HidApi, info: &DeviceInfo, kind: Kind) -> Result<Self> {
        if let Some(warning) = kind.warning()
            && !BT_WARNING_SHOWN.swap(true, Ordering::Relaxed)
        {
            eprintln!("Warning: {warning}");
        }
        let device = info
            .open_device(api)
            .context("cannot open HID device; see README udev permissions")?;
        let reports = if kind == Kind::Bluetooth {
            let mut buf = [0u8; 4096];
            let n = device
                .get_report_descriptor(&mut buf)
                .context("cannot read the Bluetooth report descriptor")?;
            Some(parse_reports(&buf[..n])?)
        } else {
            None
        };
        let feature = if kind == Kind::Bluetooth {
            FeatureReport { id: 0, len: 64 }
        } else {
            feature_report(&device)?
        };
        let channel = Self {
            device,
            kind,
            reports,
            feature,
            exchanges: None,
            last_bt_send: Mutex::new(None),
        };
        if kind == Kind::Dongle {
            match dongle::decode_status(&channel.dongle_status()?)? {
                dongle::Link::Online { .. } => {}
                dongle::Link::Offline => {
                    bail!("the keyboard linked to the 2.4 GHz receiver is asleep; press a key")
                }
                dongle::Link::Boot => bail!("the 2.4 GHz receiver is in firmware-update mode"),
            }
            // Route traffic to the keyboard rather than a paired mouse.
            channel.set_feature(&[0xf6, 0x0a])?;
        }
        Ok(channel)
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Count every request/reply exchange, for progress reporting.
    pub fn count_exchanges(&mut self, counter: Arc<AtomicUsize>) {
        self.exchanges = Some(counter);
    }

    fn set_feature(&self, data: &[u8]) -> Result<()> {
        let mut wire = [0; 65];
        wire[0] = self.feature.id;
        wire[1..1 + data.len()].copy_from_slice(data);
        self.device
            .send_feature_report(&wire)
            .context("HID feature send failed (not retried)")
    }

    fn get_feature(&self) -> Result<Packet> {
        let mut wire = [0; 65];
        wire[0] = self.feature.id;
        let n = self
            .device
            .get_feature_report(&mut wire)
            .context("HID feature read failed")?;
        decode_feature_report(&wire[..n], self.feature)
    }

    fn dongle_status(&self) -> Result<Packet> {
        self.set_feature(&[0xf7])?;
        self.get_feature()
    }

    /// Poll receiver status until `ready` holds for it.
    fn dongle_wait(&self, ready: impl Fn(&Packet) -> bool, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let status = self.dongle_status()?;
            if ready(&status) {
                return Ok(true);
            }
            if Instant::now() > deadline {
                return Ok(false);
            }
            sleep(Duration::from_millis(2));
        }
    }

    fn bt_write(&self, payload: &[u8]) -> Result<()> {
        let reports = self.reports.context("Bluetooth reports unknown")?;
        let wire = reports.encode(payload)?;
        let mut last = self.last_bt_send.lock().unwrap_or_else(|e| e.into_inner());
        let spacing = Duration::from_millis(100);
        if let Some(sent) = *last {
            sleep(spacing.saturating_sub(sent.elapsed()));
        }
        let written = self
            .device
            .write(&wire)
            .context("Bluetooth write failed (not retried)")?;
        *last = Some(Instant::now());
        ensure!(written == wire.len(), "short Bluetooth write (not retried)");
        Ok(())
    }

    /// Ignore other report IDs, key events and vendor notifications.
    fn bt_read(&self, mark: u8, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let reports = self.reports.context("Bluetooth reports unknown")?;
        let deadline = Instant::now() + timeout;
        let mut buf = vec![0u8; reports.input_len + usize::from(reports.input_id != 0)];
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let n = self
                .device
                .read_timeout(&mut buf, left.as_millis().max(1) as i32)
                .context("Bluetooth read failed")?;
            let Some(payload) = reports.payload(&buf[..n]) else {
                continue;
            };
            if payload.first() == Some(&mark) {
                return Ok(Some(payload[1..].to_vec()));
            }
            if payload.first() == Some(&0x88) {
                if mark == 0x77 {
                    return Ok(None);
                }
                bail!("the Bluetooth keyboard went offline");
            }
        }
        Ok(None)
    }

    fn bt_drain(&self) -> Result<()> {
        let mut buf = [0u8; 256];
        for _ in 0..256 {
            if self.device.read_timeout(&mut buf, 0)? == 0 {
                return Ok(());
            }
        }
        bail!("Bluetooth input queue is busy; stop other keyboard software")
    }

    pub fn send(&self, data: &Packet) -> Result<()> {
        match self.kind {
            Kind::Usb => {
                sleep(Duration::from_millis(10));
                self.set_feature(data)
            }
            Kind::Dongle => {
                ensure!(
                    self.dongle_wait(|s| s[5] == 1, Duration::from_millis(1500))?,
                    "the 2.4 GHz receiver is not accepting commands"
                );
                // Vendor host spaces every receiver step 10 ms apart.
                sleep(Duration::from_millis(10));
                self.set_feature(data)
            }
            Kind::Bluetooth => {
                // bt_write spaces all Bluetooth commands, including status requests.
                let mut payload = vec![BT_MARK];
                payload.extend_from_slice(data);
                self.bt_write(&payload)
            }
        }
    }

    pub fn exchange(&self, data: &Packet) -> Result<Packet> {
        if let Some(counter) = &self.exchanges {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        match self.kind {
            Kind::Usb => {
                self.send(data)?;
                sleep(Duration::from_millis(10));
                self.get_feature()
            }
            Kind::Dongle => {
                self.send(data)?;
                ensure!(
                    self.dongle_wait(|s| s[0] == 1, Duration::from_secs(2))?,
                    "no reply over 2.4 GHz; the keyboard may be asleep or out of range"
                );
                sleep(Duration::from_millis(10));
                self.set_feature(&[0xfc])?;
                sleep(Duration::from_millis(10));
                self.get_feature()
            }
            Kind::Bluetooth => {
                self.bt_drain()?;
                bluetooth_exchange(
                    || self.send(data),
                    || self.bt_read(BT_MARK, Duration::from_millis(200)),
                )
            }
        }
    }

    /// Battery in percent, or `None` while asleep. Cable reports nothing.
    pub fn battery(&self) -> Result<Option<u8>> {
        match self.kind {
            Kind::Usb => Ok(None),
            Kind::Dongle => match dongle::decode_status(&self.dongle_status()?)? {
                dongle::Link::Online { battery } => Ok(Some(battery)),
                _ => Ok(None),
            },
            Kind::Bluetooth => {
                self.bt_drain()?;
                self.bt_write(&[0x77])?;
                let Some(reply) = self.bt_read(0x77, Duration::from_millis(200))? else {
                    return Ok(None);
                };
                let battery = *reply.first().context("short Bluetooth battery reply")?;
                ensure!(
                    battery <= 100,
                    "invalid Bluetooth battery percentage: {battery}"
                );
                Ok(Some(battery))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_vendor_reports() {
        // Keyboard collection, then vendor page 0xFF55 with ID 6: 65-byte
        // output and input reports (marker plus 64-byte packet).
        let descriptor = [
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x85, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02,
            0xc0, 0x06, 0x55, 0xff, 0x0a, 0x02, 0x02, 0xa1, 0x01, 0x85, 0x06, 0x75, 0x08, 0x95,
            0x41, 0x91, 0x02, 0x81, 0x02, 0xc0,
        ];
        assert_eq!(
            parse_reports(&descriptor).unwrap(),
            Reports {
                output_id: 6,
                output_len: 65,
                input_id: 6,
                input_len: 65
            }
        );
        assert!(parse_reports(&descriptor[..15]).is_err());
    }

    fn descriptor(count: u8) -> Vec<u8> {
        vec![
            0x06, 0x55, 0xff, 0x0a, 0x02, 0x02, 0xa1, 0x01, 0x85, 6, 0x75, 8, 0x95, count, 0x91, 2,
            0x81, 2, 0xc0,
        ]
    }

    #[test]
    fn bluetooth_preserves_complete_commands_and_pages() {
        let reports = parse_reports(&descriptor(65)).unwrap();
        let mut packet = [0; 64];
        packet[0] = 0x8f;
        packet[63] = 0xab;
        let mut payload = vec![BT_MARK];
        payload.extend_from_slice(&packet);
        let wire = reports.encode(&payload).unwrap();
        assert_eq!(wire.len(), 66);
        assert_eq!(&wire[..3], &[6, BT_MARK, 0x8f]);
        assert_eq!(wire[65], 0xab);
        let received = reports.payload(&wire).unwrap();
        assert_eq!(received[0], BT_MARK);
        assert_eq!(bluetooth_packet(&received[1..]).unwrap(), packet);
        // A zero first byte is valid in raw map/macro/RGB pages.
        packet[0] = 0;
        assert_eq!(bluetooth_packet(&packet).unwrap(), packet);
        assert!(bluetooth_packet(&packet[..63]).is_err());
        assert!(reports.encode(&[0; 66]).is_err());
        assert!(reports.payload(&[1, BT_MARK, 0x8f]).is_none());
        assert!(reports.payload(&[]).is_none());
        assert_eq!(&reports.encode(&[0x77]).unwrap()[..3], &[6, 0x77, 0]);
    }

    #[test]
    fn bluetooth_descriptor_requires_both_complete_reports() {
        assert!(parse_reports(&descriptor(64)).is_err());
        let complete = descriptor(65);
        for len in 0..complete.len() {
            assert!(
                parse_reports(&complete[..len]).is_err(),
                "accepted prefix {len}"
            );
        }
        assert!(parse_reports(&[0xfe, 8, 1]).is_err());
        let mut split = descriptor(64);
        split.pop();
        split.extend_from_slice(&[0x95, 1, 0x91, 3, 0x81, 3, 0xc0]);
        assert_eq!(parse_reports(&split).unwrap().output_len, 65);
        assert_eq!(parse_reports(&split).unwrap().input_len, 65);
        // Missing input is an error, not a guessed output ID.
        let mut output_only = complete.clone();
        output_only.drain(16..18);
        assert!(parse_reports(&output_only).is_err());
        let mut wrong_page = complete.clone();
        wrong_page[1] = 0x01;
        assert!(parse_reports(&wrong_page).is_err());
        let mut ambiguous = complete;
        ambiguous.extend(
            descriptor(65)
                .into_iter()
                .enumerate()
                .map(|(i, b)| if i == 9 { 7 } else { b }),
        );
        assert!(parse_reports(&ambiguous).is_err());
    }

    #[test]
    fn bluetooth_retries_only_unanswered_queries() {
        use std::cell::Cell;
        let sends = Cell::new(0);
        let send = || {
            sends.set(sends.get() + 1);
            Ok(())
        };
        let reply =
            bluetooth_exchange(send, || Ok((sends.get() == 4).then(|| vec![7; 64]))).unwrap();
        assert_eq!(sends.get(), 4);
        assert_eq!(reply, [7; 64]);
        sends.set(0);
        assert!(bluetooth_exchange(send, || Ok(None)).is_err());
        assert_eq!(sends.get(), 4);
        sends.set(0);
        assert!(bluetooth_exchange(send, || Ok(Some(vec![0; 63]))).is_err());
        assert_eq!(sends.get(), 1);
        sends.set(0);
        assert!(bluetooth_exchange(send, || bail!("offline")).is_err());
        assert_eq!(sends.get(), 1);
        assert!(bluetooth_exchange(|| bail!("send failed"), || panic!("must not read")).is_err());
    }
}
