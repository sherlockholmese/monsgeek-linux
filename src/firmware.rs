//! Experimental wired USB updater, reconstructed from CommonKeyboard.upgrade
//! and gd.upgrade_usb in the official macOS host. No hardware validation yet.
use std::{
    ffi::CStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use hidapi::{BusType, DeviceInfo, HidApi, HidDevice};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    device::{Keyboard, Snapshot, lock_node},
    protocol::{self, Packet, packet},
    transaction,
    transport::{self, Kind},
};

pub const WARNING: &str = "Experimental firmware update. Wrong firmware or interruption can brick the keyboard. USB only.";
pub const PREFIX_BYTES: usize = 65_536;
pub const MAX_PAYLOAD_BYTES: usize = u16::MAX as usize * 64;

#[derive(Debug, Clone, Serialize)]
pub struct ImageInfo {
    pub experimental: bool,
    pub target_device_id: u32,
    pub image_model_verified: bool,
    pub file_bytes: usize,
    pub prefix_bytes: usize,
    pub payload_bytes: usize,
    pub blocks: u16,
    pub payload_sum: u32,
    pub sha256: String,
}

#[derive(Clone)]
pub struct Image {
    info: ImageInfo,
    payload: Vec<u8>,
}

impl Image {
    /// Vendor firmwareFile.bin, after decompressing any download container.
    /// The host skips 64 KiB. No model/signature format is established in that
    /// prefix: do not claim that this parser authenticates the image's model.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() > PREFIX_BYTES,
            "vendor firmware file must exceed the 65536-byte prefix"
        );
        let payload = &bytes[PREFIX_BYTES..];
        ensure!(
            payload.len() <= MAX_PAYLOAD_BYTES,
            "firmware exceeds the 65535-block protocol limit"
        );
        Ok(Self {
            info: ImageInfo {
                experimental: true,
                target_device_id: protocol::DEVICE_ID,
                image_model_verified: false,
                file_bytes: bytes.len(),
                prefix_bytes: PREFIX_BYTES,
                payload_bytes: payload.len(),
                blocks: payload.len().div_ceil(64) as u16,
                payload_sum: payload.iter().map(|b| u32::from(*b)).sum(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
            },
            payload: payload.to_vec(),
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)
            .with_context(|| format!("cannot open {}", path.display()))?
            .take((PREFIX_BYTES + MAX_PAYLOAD_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::parse(&bytes)
    }

    pub fn info(&self) -> &ImageInfo {
        &self.info
    }

    pub fn check_sha256(&self, expected: &str) -> Result<()> {
        ensure!(
            expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
            "SHA-256 must contain 64 hex digits"
        );
        ensure!(
            self.info.sha256.eq_ignore_ascii_case(expected),
            "firmware SHA-256 mismatch"
        );
        Ok(())
    }

    pub fn prepare_packet(&self) -> Packet {
        let mut header = [0; 8];
        header[..2].copy_from_slice(&[0xba, 0xc0]);
        header[2..4].copy_from_slice(&self.info.blocks.to_le_bytes());
        header[4..8].copy_from_slice(&(self.info.payload_bytes as u32).to_le_bytes());
        packet(&header, 7)
    }

    pub fn finish_packet(&self) -> Packet {
        // ci.None: byte 7 is part of the 32-bit data sum, not a header checksum.
        let mut out = [0; 64];
        out[..2].copy_from_slice(&[0xba, 0xc2]);
        out[2..4].copy_from_slice(&self.info.blocks.to_le_bytes());
        out[4..8].copy_from_slice(&self.info.payload_sum.to_le_bytes());
        out[8..12].copy_from_slice(&(self.info.payload_bytes as u32).to_le_bytes());
        out
    }

    fn pages(&self) -> impl Iterator<Item = Packet> + '_ {
        self.payload.chunks(64).map(|chunk| {
            let mut page = [0; 64];
            page[..chunk.len()].copy_from_slice(chunk);
            page
        })
    }
}

pub fn boot_packet() -> Packet {
    packet(&[0x7f, 0x55, 0xaa, 0x55, 0xaa, 0, 0], 7)
}

#[derive(Debug, Clone, Copy)]
pub enum Progress {
    BackingUp,
    EnteringBoot,
    Preparing,
    Writing { done: u16, total: u16 },
    Verifying,
    Reconnecting,
}

/// Deliberately small injectable interface: tests never open real HID devices.
trait BootIo {
    fn send(&mut self, packet: &Packet) -> Result<()>;
    fn receive(&mut self) -> Result<Packet>;
}

fn transfer(
    io: &mut impl BootIo,
    image: &Image,
    wait: &mut impl FnMut(Duration),
    progress: &mut impl FnMut(Progress),
) -> Result<()> {
    progress(Progress::Preparing);
    wait(Duration::from_millis(10));
    io.send(&image.prepare_packet())
        .context("firmware prepare send failed")?;
    wait(Duration::from_millis(10));
    // The vendor accepts any full prepare reply. No stronger ACK schema has
    // been established. Unlike the vendor's ten attempts, do not retry a
    // potentially destructive prepare operation after a transport failure.
    io.receive().context("firmware prepare reply failed")?;
    for (index, page) in image.pages().enumerate() {
        wait(Duration::from_millis(10));
        io.send(&page)
            .with_context(|| format!("firmware block {} failed; not retried", index + 1))?;
        progress(Progress::Writing {
            done: (index + 1) as u16,
            total: image.info.blocks,
        });
    }
    progress(Progress::Verifying);
    wait(Duration::from_millis(100));
    io.send(&image.finish_packet())
        .context("firmware finish send failed")?;
    wait(Duration::from_millis(100));
    let reply = io.receive().context("firmware completion reply failed")?;
    ensure!(
        reply[4] == 0x55,
        "bootloader did not confirm firmware completion"
    );
    Ok(())
}

struct BootConnection {
    device: HidDevice,
    _lock: File,
}

impl BootIo for BootConnection {
    fn send(&mut self, packet: &Packet) -> Result<()> {
        let mut wire = [0; 65];
        wire[1..].copy_from_slice(packet);
        self.device
            .send_feature_report(&wire)
            .context("USB bootloader feature send failed")
    }

    fn receive(&mut self) -> Result<Packet> {
        let mut wire = [0; 65];
        let count = self
            .device
            .get_feature_report(&mut wire)
            .context("USB bootloader feature read failed")?;
        protocol::decode_feature(&wire[..count])
    }
}

// Exact generic bootKeyboard filters from the vendor's uR table. These are
// candidates, not original-M1W identities. Re-enumeration must match the USB
// port of the ID983 keyboard identified before entering boot mode.
const BOOT_FILTERS: &[(u16, u16, u16, u16)] = &[
    (0x0c45, 0x7040, 0x000c, 1),
    (0x25a7, 0x2300, 0x000c, 1),
    (0x0461, 0x4001, 0xff01, 1),
    (0x0461, 0x4001, 0x0001, 6),
    (0x3151, 0x4001, 0xff01, 1),
    (0x3151, 0x400d, 0xff01, 1),
    (0x3151, 0x4016, 0xff01, 1),
    (0x3151, 0x4019, 0xff01, 1),
    (0x3151, 0x401a, 0xffff, 2),
    (0x3151, 0x4036, 0xff01, 1),
    (0x3151, 0x403b, 0xff01, 1),
    (0x3151, 0x5001, 0xff01, 1),
    (0x3151, 0x5024, 0xff01, 1),
    (0x3151, 0x502a, 0xff01, 1),
    (0x3151, 0x504d, 0xff01, 1),
    (0x3151, 0x5057, 0xff01, 1),
    (0x4353, 0x8008, 0xff80, 1),
    (0x3151, 0x5047, 0xff01, 1),
    (0x3151, 0x5049, 0xff01, 1),
    (0x25aa, 0x200c, 0xff01, 1),
    (0x25aa, 0x8003, 0xff01, 1),
    (0x25aa, 0x2009, 0xff01, 1),
    (0x25aa, 0x200a, 0xff01, 1),
    (0x25aa, 0x200b, 0xff01, 1),
];

pub fn boot_candidate(info: &DeviceInfo) -> bool {
    matches!(info.bus_type(), BusType::Usb)
        && info.interface_number() == 0
        && BOOT_FILTERS.contains(&(
            info.vendor_id(),
            info.product_id(),
            info.usage_page(),
            info.usage(),
        ))
}

/// Stable physical USB port path, not the hidraw number or VID/PID, which change
/// during reboot. Refuse to proceed if sysfs cannot establish that relationship.
fn usb_port(info: &DeviceInfo) -> Result<PathBuf> {
    let node = Path::new(info.path().to_str()?)
        .file_name()
        .context("invalid HID path")?;
    let path = fs::canonicalize(Path::new("/sys/class/hidraw").join(node).join("device"))
        .context("cannot establish the keyboard's physical USB port")?;
    usb_ancestor(&path)
}

fn usb_ancestor(path: &Path) -> Result<PathBuf> {
    path.ancestors()
        .find(|p| p.join("idVendor").is_file() && p.join("idProduct").is_file())
        .map(Path::to_path_buf)
        .context("no USB device ancestor in sysfs")
}

fn find_on_port(api: &mut HidApi, port: &Path, boot: bool) -> Result<DeviceInfo> {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        api.refresh_devices()?;
        let mut matches: Vec<_> = api
            .device_list()
            .filter(|info| {
                if boot {
                    boot_candidate(info)
                } else {
                    transport::usb_candidate(info)
                }
            })
            .filter(|info| usb_port(info).is_ok_and(|p| p == port))
            .collect();
        matches.sort_by(|a, b| a.path().cmp(b.path()));
        matches.dedup_by(|a, b| a.path() == b.path());
        ensure!(
            matches.len() <= 1,
            "multiple firmware interfaces on the original USB port"
        );
        if let Some(info) = matches.first() {
            return Ok((*info).clone());
        }
        if Instant::now() >= deadline {
            bail!(
                "{} did not appear on the original USB port; do not retry automatically",
                if boot {
                    "bootloader"
                } else {
                    "keyboard after flashing"
                }
            );
        }
        sleep(Duration::from_millis(250));
    }
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub old_revision: u16,
    pub new_revision: u16,
    pub sha256: String,
}

/// Explicit destructive operation. Caller must obtain experimental-flashing
/// consent. Always validate the exact image and save configuration before boot.
/// A snapshot is not a firmware backup and provides no unbrick guarantee.
pub fn flash(
    api: &mut HidApi,
    image: &Image,
    expected_sha256: &str,
    path: Option<&CStr>,
    before: &Path,
    baseline: Option<&Snapshot>,
    mut progress: impl FnMut(Progress),
) -> Result<Outcome> {
    image.check_sha256(expected_sha256)?;
    let candidates: Vec<_> = api
        .device_list()
        .filter(|info| transport::usb_candidate(info) && path.is_none_or(|p| p == info.path()))
        .collect();
    ensure!(
        candidates.len() == 1,
        "firmware update needs exactly one original-M1W USB candidate; select --device"
    );
    let info = candidates[0].clone();
    let port = usb_port(&info)?;
    let keyboard = Keyboard::open_with_transport(api, Some(info.path()), Some(Kind::Usb))?;
    progress(Progress::BackingUp);
    let snapshot = keyboard.snapshot()?;
    if let Some(baseline) = baseline {
        ensure!(
            &snapshot == baseline,
            "keyboard changed since it was read; reload before flashing"
        );
    }
    transaction::save_new(before, &snapshot)?;
    let old_revision = snapshot.usb_revision;
    let update = || -> Result<Outcome> {
        progress(Progress::EnteringBoot);
        keyboard.enter_firmware_boot()?;
        sleep(Duration::from_secs(1));
        let boot = find_on_port(api, &port, true)?;
        let lock = lock_node(&boot, Duration::from_secs(1))?;
        let device = boot
            .open_device(api)
            .context("cannot open bootloader; install the current access rule")?;
        let mut connection = BootConnection {
            device,
            _lock: lock,
        };
        sleep(Duration::from_secs(1));
        transfer(&mut connection, image, &mut sleep, &mut progress)?;
        progress(Progress::Reconnecting);
        drop(connection);
        sleep(Duration::from_secs(2));
        // The old hidraw node lock must not collide with a reused hidraw number.
        drop(keyboard);
        let normal = find_on_port(api, &port, false)?;
        let returned = Keyboard::open_with_transport(api, Some(normal.path()), Some(Kind::Usb))?;
        let new_revision = returned.firmware_revision()?;
        Ok(Outcome {
            old_revision,
            new_revision,
            sha256: image.info.sha256.clone(),
        })
    };
    update().with_context(|| format!(
        "experimental firmware update failed; do not retry automatically. Configuration backup: {} (not a firmware recovery image)", before.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(payload: &[u8]) -> Image {
        let mut bytes = vec![0x99; PREFIX_BYTES];
        bytes.extend_from_slice(payload);
        Image::parse(&bytes).unwrap()
    }

    #[derive(Default)]
    struct FakeIo {
        sent: Vec<Packet>,
        replies: std::collections::VecDeque<Result<Packet>>,
        fail_send: Option<usize>,
    }
    impl BootIo for FakeIo {
        fn send(&mut self, packet: &Packet) -> Result<()> {
            self.sent.push(*packet);
            ensure!(
                self.fail_send != Some(self.sent.len()),
                "injected send failure"
            );
            Ok(())
        }
        fn receive(&mut self) -> Result<Packet> {
            self.replies.pop_front().expect("unexpected firmware read")
        }
    }
    fn ready() -> FakeIo {
        let mut completion = [0; 64];
        completion[4] = 0x55;
        FakeIo {
            replies: [Ok([0; 64]), Ok(completion)].into(),
            ..FakeIo::default()
        }
    }

    #[test]
    fn vendor_update_packets_and_payload_boundaries() {
        let image = image(&[1; 65]);
        assert_eq!(
            &boot_packet()[..8],
            &[0x7f, 0x55, 0xaa, 0x55, 0xaa, 0, 0, 0x82]
        );
        assert_eq!(
            &image.prepare_packet()[..8],
            &[0xba, 0xc0, 2, 0, 65, 0, 0, 0x42]
        );
        assert_eq!(
            &image.finish_packet()[..12],
            &[0xba, 0xc2, 2, 0, 65, 0, 0, 0, 65, 0, 0, 0]
        );
        let pages: Vec<_> = image.pages().collect();
        assert_eq!(
            pages,
            [[1; 64], {
                let mut last = [0; 64];
                last[0] = 1;
                last
            }]
        );
        assert!(!image.info().image_model_verified);
        assert_eq!(image.info().payload_bytes, 65);
        assert_eq!(image.info().target_device_id, 983);
        assert!(Image::parse(&[0; PREFIX_BYTES]).is_err());
        assert!(Image::parse(&vec![0; PREFIX_BYTES + MAX_PAYLOAD_BYTES + 1]).is_err());
        let maximum = self::image(&vec![255; MAX_PAYLOAD_BYTES]);
        assert_eq!(maximum.info.blocks, 65535);
        assert_eq!(maximum.info.payload_sum, (MAX_PAYLOAD_BYTES * 255) as u32);
    }

    #[test]
    fn sha256_covers_the_prefix_and_the_payload() {
        let image = image(&[1; 64]);
        image
            .check_sha256(&image.info.sha256.to_uppercase())
            .unwrap();
        assert!(image.check_sha256(&"0".repeat(64)).is_err());
        assert!(image.check_sha256("not a digest").is_err());
        let mut changed = vec![0x99; PREFIX_BYTES];
        changed.extend([1; 64]);
        changed[0] ^= 1;
        assert_ne!(
            Image::parse(&changed).unwrap().info.sha256,
            image.info.sha256
        );
    }

    #[test]
    fn complete_transfer_waits_and_requires_completion_ack() {
        let image = image(&[0x5a; 65]);
        let mut io = ready();
        let mut waits = Vec::new();
        let mut written = Vec::new();
        transfer(
            &mut io,
            &image,
            &mut |d| waits.push(d.as_millis()),
            &mut |p| {
                if let Progress::Writing { done, total } = p {
                    written.push((done, total));
                }
            },
        )
        .unwrap();
        assert_eq!(io.sent.len(), 4);
        assert_eq!(io.sent[0], image.prepare_packet());
        assert_eq!(io.sent[1], [0x5a; 64]);
        assert_eq!(io.sent[2][0], 0x5a);
        assert_eq!(&io.sent[2][1..], &[0; 63]);
        assert_eq!(io.sent[3], image.finish_packet());
        assert_eq!(waits, [10, 10, 10, 10, 100, 100]);
        assert_eq!(written, [(1, 2), (2, 2)]);
        let mut rejected = ready();
        rejected.replies[1] = Ok([0; 64]);
        assert!(transfer(&mut rejected, &image, &mut |_| {}, &mut |_| {}).is_err());
    }

    #[test]
    fn failures_stop_without_retry_or_finish() {
        let image = image(&[1; 128]);
        for failed in 1..=4 {
            let mut io = ready();
            io.fail_send = Some(failed);
            assert!(transfer(&mut io, &image, &mut |_| {}, &mut |_| {}).is_err());
            assert_eq!(io.sent.len(), failed);
        }
        let mut io = ready();
        io.replies[0] = Err(anyhow::anyhow!("read failed"));
        assert!(transfer(&mut io, &image, &mut |_| {}, &mut |_| {}).is_err());
        assert_eq!(io.sent.len(), 1);
    }
}
