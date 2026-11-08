//! Blocking device work. Every call opens, identifies and locks the keyboard,
//! so the GUI never holds the configuration node between operations.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use hidapi::HidApi;
use monsgeek_linux::{
    device::{self, Keyboard, Snapshot},
    transaction::{self, BeforeState},
    transport::{self, Kind},
};

fn open() -> Result<Keyboard> {
    let api = HidApi::new().context("Cannot enumerate HID devices")?;
    Keyboard::open(&api, None)
}

/// Live state of a full read, shared with the UI thread.
#[derive(Clone, Default)]
pub struct ReadProgress {
    /// Exchanges done, against `SNAPSHOT_EXCHANGES`.
    pub done: Arc<AtomicUsize>,
    /// Transport being opened, then the transport of the identified keyboard.
    pub kind: Arc<Mutex<Option<Kind>>>,
}

impl ReadProgress {
    pub fn kind(&self) -> Option<Kind> {
        *self.kind.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Full read, reporting progress as it goes.
pub fn read(progress: ReadProgress) -> Result<(Snapshot, Kind)> {
    progress.done.store(0, Ordering::Relaxed);
    *progress.kind.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let api = HidApi::new().context("Cannot enumerate HID devices")?;
    let mut keyboard = Keyboard::open_reporting(&api, None, None, |kind| {
        *progress.kind.lock().unwrap_or_else(|e| e.into_inner()) = Some(kind);
    })?;
    keyboard.count_exchanges(progress.done.clone());
    Ok((keyboard.snapshot()?, keyboard.kind()))
}

/// What the background poll sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    /// A cable connection is plugged in.
    pub usb: bool,
    /// Receiver or Bluetooth link with battery when awake.
    pub wireless: Option<(Kind, Option<u8>)>,
}

impl Presence {
    /// The keyboard can be configured right now.
    pub fn reachable(&self) -> bool {
        self.usb || matches!(self.wireless, Some((_, Some(_))))
    }
}

pub fn presence() -> Result<Presence> {
    let api = HidApi::new().context("Cannot enumerate HID devices")?;
    let usb = api.device_list().any(transport::usb_candidate);
    Ok(Presence {
        usb,
        wireless: device::wireless_status(&api)?,
    })
}

/// Apply `draft` over `baseline`. Refuses when the device no longer matches
/// `baseline`, so edits never overwrite changes made elsewhere.
pub fn apply(baseline: Snapshot, draft: Snapshot) -> Result<(PathBuf, Snapshot)> {
    let keyboard = open()?;
    let before = new_record_path()?;
    let after = transaction::apply_draft(&keyboard, &baseline, &draft, &before)?;
    Ok((before, after))
}

pub fn flash_firmware(
    image: monsgeek_linux::firmware::Image,
    baseline: Snapshot,
    before: PathBuf,
    status: Arc<Mutex<String>>,
) -> Result<monsgeek_linux::firmware::Outcome> {
    use monsgeek_linux::firmware::{self, Progress};
    let mut api = HidApi::new().context("Cannot enumerate HID devices")?;
    firmware::flash(
        &mut api,
        &image,
        &image.info().sha256,
        None,
        &before,
        Some(&baseline),
        |progress| {
            let text = match progress {
                Progress::BackingUp => "Saving configuration".to_string(),
                Progress::EnteringBoot => "Entering bootloader".into(),
                Progress::Preparing => "Preparing firmware".into(),
                Progress::Writing { done, total } => format!("Writing {done}/{total} blocks"),
                Progress::Verifying => "Checking completion".into(),
                Progress::Reconnecting => "Waiting for keyboard".into(),
            };
            *status.lock().unwrap_or_else(|e| e.into_inner()) = text;
        },
    )
}

pub fn firmware_backup_path() -> Result<PathBuf> {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(state_dir()?.join(format!("firmware-before-{ms}.json")))
}

pub fn export(snapshot: &Snapshot, path: &Path) -> Result<()> {
    transaction::save_new(path, snapshot)
}

pub fn load(path: &Path) -> Result<Snapshot> {
    let snapshot: Snapshot = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?,
    )
    .or_else(|_| {
        // Before-state records wrap the snapshot.
        anyhow::Ok(serde_json::from_slice::<BeforeState>(&fs::read(path)?)?.snapshot)
    })
    .context("Not an M1W snapshot")?;
    snapshot.validate()?;
    Ok(snapshot)
}

pub fn state_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .context("HOME is not set")?;
    let dir = base.join("monsgeek-linux");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

fn new_record_path() -> Result<PathBuf> {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(state_dir()?.join(format!("before-{ms}.json")))
}

#[derive(Debug, Clone)]
pub struct Record {
    pub saved: SystemTime,
    pub changes: usize,
    pub snapshot: Snapshot,
}

/// Saved before-states, newest first.
pub fn history() -> Vec<Record> {
    let Ok(dir) = state_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Record> = entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("before-") && n.ends_with(".json"))
        })
        .filter_map(|e| {
            let saved = e.metadata().ok()?.modified().ok()?;
            let record: BeforeState = serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?;
            record.snapshot.validate().ok()?;
            Some(Record {
                saved,
                changes: record.changes().len(),
                snapshot: record.snapshot,
            })
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.saved));
    out
}
