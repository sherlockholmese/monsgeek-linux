//! 2.4 GHz receiver status (`0xF7`). Configuration traffic through the
//! receiver lives in `transport`.
//!
//! Reply layout (payload after the HID report-ID byte), from the vendor host:
//! byte0 readable flag, byte1 keyboard battery %, byte2 mouse battery %,
//! byte3 keyboard offline (0 = online), byte4 mouse offline, byte5 can-send,
//! byte6 linked devices (1 keyboard, 2 mouse, 3 both), byte8 RF boot mode.
//! Observed on receiver 3151:4011 with a linked M1W: asleep
//! `00 00 00 01 01 01 00`, awake `00 60 00 00 01 01 01` (96 %).
use std::{thread::sleep, time::Duration};

use anyhow::{Context, Result, ensure};
use hidapi::{BusType, DeviceInfo, HidApi};

use crate::protocol::VID;

/// Receiver product IDs confirmed with this keyboard.
pub const PIDS: [u16; 1] = [0x4011];

pub fn candidate(info: &DeviceInfo) -> bool {
    info.vendor_id() == VID
        && PIDS.contains(&info.product_id())
        && matches!(info.bus_type(), BusType::Usb)
        && info.interface_number() == 2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The keyboard is linked and awake; battery in percent.
    Online { battery: u8 },
    /// The receiver reports no awake keyboard (asleep, off or out of range).
    Offline,
    /// The receiver is in firmware-update mode.
    Boot,
}

pub fn decode_status(payload: &[u8]) -> Result<Link> {
    ensure!(payload.len() >= 9, "short receiver status reply");
    if payload[8] == 1 {
        return Ok(Link::Boot);
    }
    let keyboard = matches!(payload[6], 1 | 3);
    Ok(if keyboard && payload[3] == 0 {
        Link::Online {
            battery: payload[1].min(100),
        }
    } else {
        Link::Offline
    })
}

/// Query the first connected receiver. `Ok(None)` means no receiver is plugged in.
pub fn status(api: &HidApi) -> Result<Option<Link>> {
    let Some(info) = api.device_list().find(|d| candidate(d)) else {
        return Ok(None);
    };
    // Never interleave with a configuration session on the same receiver.
    let _lock = crate::device::lock_node(info, Duration::from_millis(200))?;
    let device = info
        .open_device(api)
        .context("Cannot open the 2.4 GHz receiver; see README udev permissions")?;
    let mut wire = [0u8; 65];
    wire[1] = 0xf7;
    sleep(Duration::from_millis(10));
    device
        .send_feature_report(&wire)
        .context("Receiver status request failed")?;
    sleep(Duration::from_millis(20));
    let mut reply = [0u8; 65];
    let n = device
        .get_feature_report(&mut reply)
        .context("Receiver status read failed")?;
    decode_status(&reply[1..n.max(1)]).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_replies_decode() {
        let asleep = [0, 0, 0, 1, 1, 1, 0, 0, 0, 0];
        let awake = [0, 0x60, 0, 0, 1, 1, 1, 0, 0, 0];
        assert_eq!(decode_status(&asleep).unwrap(), Link::Offline);
        assert_eq!(decode_status(&awake).unwrap(), Link::Online { battery: 96 });
        let mut boot = awake;
        boot[8] = 1;
        assert_eq!(decode_status(&boot).unwrap(), Link::Boot);
        assert!(decode_status(&[0; 4]).is_err());
    }
}
