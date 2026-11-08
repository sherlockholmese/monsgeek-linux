//! Device access: the udev rule that lets the logged-in user open the
//! configuration interfaces without root, and its installation.
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

/// The rule shipped with this build.
pub const RULE: &str = include_str!("../udev/70-monsgeek-m1w.rules");
pub const PATH: &str = "/etc/udev/rules.d/70-monsgeek-m1w.rules";
/// A distribution package may install the same rule here instead.
const PACKAGED: &str = "/usr/lib/udev/rules.d/70-monsgeek-m1w.rules";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Installed,
    /// A different version is installed.
    Outdated,
    Missing,
}

pub fn status() -> Status {
    let read = |p: &str| fs::read_to_string(p).ok();
    match (read(PATH), read(PACKAGED)) {
        (Some(a), _) if a == RULE => Status::Installed,
        (None, Some(b)) if b == RULE => Status::Installed,
        (Some(_), _) | (None, Some(_)) => Status::Outdated,
        (None, None) => Status::Missing,
    }
}

/// Root shell script that installs `RULE` atomically and re-applies it to
/// already connected devices. The rule travels inside the script, so nothing
/// is read from a user-writable file.
pub fn script() -> String {
    format!(
        "set -e\numask 022\nmkdir -p /etc/udev/rules.d\n\
         cat > /etc/udev/rules.d/.70-monsgeek-m1w.rules.new <<'MONSGEEK_RULE'\n{RULE}MONSGEEK_RULE\n\
         mv -f /etc/udev/rules.d/.70-monsgeek-m1w.rules.new {PATH}\n\
         udevadm control --reload-rules\n\
         udevadm trigger --action=change --subsystem-match=hidraw\n"
    )
}

/// A command to paste into a terminal when no elevation method works.
pub fn manual_command() -> String {
    format!("sudo sh -c '{}'", script().replace('\'', r"'\''"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevate {
    /// Graphical polkit prompt; needs a running authentication agent.
    Pkexec,
    /// Terminal password prompt.
    Sudo,
}

#[derive(Debug)]
pub enum InstallError {
    /// The person closed or failed the password prompt.
    Cancelled,
    /// No polkit authentication agent is running in this session.
    NoAgent,
    Failed(anyhow::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Authorization cancelled"),
            Self::NoAgent => f.write_str("No password prompt is available in this session"),
            Self::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Install the rule as root. Blocks until the prompt is answered.
pub fn install(elevate: Elevate) -> Result<(), InstallError> {
    let program = match elevate {
        Elevate::Pkexec => "pkexec",
        Elevate::Sudo => "sudo",
    };
    let output = Command::new(program)
        .args(["/bin/sh", "-c", &script()])
        .stdin(match elevate {
            // sudo reads the password from the terminal.
            Elevate::Sudo => Stdio::inherit(),
            Elevate::Pkexec => Stdio::null(),
        })
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("cannot run {program}"))
        .map_err(InstallError::Failed)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(match (elevate, output.status.code()) {
        (Elevate::Pkexec, _) if stderr.contains("authentication agent") => InstallError::NoAgent,
        (Elevate::Pkexec, Some(126)) => InstallError::Cancelled,
        (Elevate::Sudo, Some(1)) if stderr.contains("incorrect password") => {
            InstallError::Cancelled
        }
        (_, code) => InstallError::Failed(anyhow::anyhow!(
            "{program} exited with {}: {stderr}",
            code.map_or("a signal".into(), |c| c.to_string())
        )),
    })
}

/// Open a terminal that asks for the password with sudo and installs the
/// rule. Returns once the terminal has started; poll `status` for the result.
pub fn install_in_terminal() -> Result<()> {
    let candidates = std::env::var("TERMINAL").ok().into_iter().chain(
        [
            "ghostty",
            "kitty",
            "foot",
            "alacritty",
            "wezterm",
            "konsole",
            "gnome-terminal",
            "xterm",
        ]
        .map(String::from),
    );
    let inner = format!(
        "sudo /bin/sh -c {} && echo 'Rule installed.' || echo 'Rule not installed.'; sleep 3",
        shell_quote(&script())
    );
    for terminal in candidates {
        let name = terminal.rsplit('/').next().unwrap_or(&terminal).to_string();
        let mut command = Command::new(&terminal);
        match name.as_str() {
            "gnome-terminal" => command.arg("--"),
            "wezterm" => command.args(["start", "--"]),
            _ => command.arg("-e"),
        };
        command
            .args(["/bin/sh", "-c", &inner])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if command.spawn().is_ok() {
            return Ok(());
        }
    }
    bail!("no terminal emulator found; set $TERMINAL")
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Errors from opening a node the user may not access.
pub fn is_permission_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

/// Remember that the person declined this exact rule version.
fn dismissal_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("monsgeek-linux").join("access-declined"))
}

fn fingerprint() -> String {
    // FNV-1a: stable across toolchains, unlike std's hasher.
    let hash = RULE.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

pub fn declined() -> bool {
    dismissal_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == fingerprint())
}

pub fn decline() -> Result<()> {
    let path = dismissal_path().context("HOME is not set")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = fs::File::create(path)?;
    writeln!(file, "{}", fingerprint())?;
    Ok(())
}

/// Whether to offer installation now.
pub fn should_offer() -> bool {
    status() != Status::Installed && !declined()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_embeds_the_rule_verbatim() {
        let s = script();
        assert!(s.contains(RULE));
        assert!(!RULE.contains("MONSGEEK_RULE"));
        assert!(
            RULE.ends_with('\n'),
            "heredoc terminator needs its own line"
        );
        assert!(manual_command().starts_with("sudo sh -c '"));
    }
}
