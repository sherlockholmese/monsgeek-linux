//! Activity log. Every device operation and failure is recorded with its full
//! error chain, kept in memory for the Logs page and appended as JSON lines to
//! `$XDG_STATE_HOME/monsgeek-linux/gui.log` for this session, including panics.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    pub level: Level,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<String>,
}

impl Entry {
    pub fn new(level: Level, message: impl Into<String>, detail: Vec<String>) -> Self {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        Self {
            at,
            level,
            message: message.into(),
            detail,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self::new(Level::Info, message, Vec::new())
    }

    /// An error with each cause of its chain on its own detail line.
    pub fn error(message: impl Into<String>, error: &anyhow::Error) -> Self {
        Self::new(
            Level::Error,
            message,
            error.chain().map(|c| c.to_string()).collect(),
        )
    }

    pub fn local_time(&self) -> chrono::DateTime<chrono::Local> {
        chrono::DateTime::from_timestamp_millis(self.at as i64)
            .unwrap_or_default()
            .with_timezone(&chrono::Local)
    }

    /// Plain-text form used for copying.
    pub fn text(&self) -> String {
        let level = match self.level {
            Level::Info => "INFO ",
            Level::Error => "ERROR",
        };
        let mut out = format!(
            "{} {level} {}",
            self.local_time().format("%Y-%m-%d %H:%M:%S%.3f"),
            self.message
        );
        for line in &self.detail {
            out.push_str("\n    ");
            out.push_str(line);
        }
        out
    }
}

fn path() -> Option<PathBuf> {
    crate::device::state_dir().ok().map(|d| d.join("gui.log"))
}

/// Start a fresh disk log. The Logs panel starts empty independently of disk.
pub fn start_session() {
    if cfg!(test) {
        return;
    }
    if let Some(path) = path() {
        // A logging failure must not prevent using the configurator.
        if let Err(error) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
        {
            eprintln!("Cannot start session log: {error}");
        }
    }
}

/// Append one entry. Logging never fails the operation being logged.
pub fn append(entry: &Entry) {
    if cfg!(test) {
        return;
    }
    let Some(path) = path() else { return };
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    {
        writeln!(file, "{line}").ok();
    }
}

pub fn clear() -> anyhow::Result<()> {
    if cfg!(test) {
        return Ok(());
    }
    if let Some(path) = path() {
        match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

/// Record panics before the process unwinds, then defer to the default hook.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "Unknown panic".into());
        let mut detail = vec![message];
        if let Some(loc) = info.location() {
            detail.push(format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));
        }
        append(&Entry::new(Level::Error, "Crashed", detail));
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_keeps_the_whole_chain() {
        let e = anyhow::anyhow!("Timed out").context("Cannot read lighting");
        let entry = Entry::error("Read failed", &e);
        assert_eq!(entry.detail, ["Cannot read lighting", "Timed out"]);
        assert!(
            entry
                .text()
                .contains("ERROR Read failed\n    Cannot read lighting")
        );
        let line = serde_json::to_string(&entry).unwrap();
        assert_eq!(serde_json::from_str::<Entry>(&line).unwrap(), entry);
    }
}
