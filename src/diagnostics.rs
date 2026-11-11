//! Support reports omit keymaps, macros, input events, serials and personal paths.
use crate::{device::Snapshot, transport::Kind};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Report {
    pub report_version: u8,
    pub application_version: &'static str,
    pub os: &'static str,
    pub architecture: &'static str,
    pub device_id: Option<u32>,
    pub firmware_revision: Option<String>,
    pub transport: Option<String>,
    pub snapshot_format: Option<u8>,
    pub pending_changes: usize,
    pub errors: Vec<String>,
}
pub fn report(
    snapshot: Option<&Snapshot>,
    transport: Option<Kind>,
    pending_changes: usize,
    errors: Vec<String>,
) -> Report {
    Report {
        report_version: 1,
        application_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        device_id: snapshot.map(|s| s.device_id),
        firmware_revision: snapshot.map(|s| format!("{:04x}", s.usb_revision)),
        transport: transport.map(|k| k.to_string()),
        snapshot_format: snapshot.map(|s| s.format_version),
        pending_changes,
        // Only caller-supplied operation titles, never raw error chains.
        errors: errors
            .into_iter()
            .take(50)
            .map(|s| s.chars().take(160).collect())
            .collect(),
    }
}
