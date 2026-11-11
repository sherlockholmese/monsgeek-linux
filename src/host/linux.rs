//! evdev/uinput transport. Explicit devices only; no root GUI or silent grabs.
use super::{Config, Engine, Output};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    mem::size_of,
    os::fd::{AsRawFd, RawFd},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Event {
    // evdev's native kernel-long fields remain 32-bit on 32-bit time64
    // userspace. libc::timeval can have a different layout there.
    time: [libc::c_ulong; 2],
    kind: u16,
    code: u16,
    value: i32,
}
impl Event {
    fn new(kind: u16, code: u16, value: i32) -> Self {
        Self {
            time: [0; 2],
            kind,
            code,
            value,
        }
    }
    fn bytes(&self) -> &[u8] {
        // repr(C), initialized timestamp and scalar fields; input_event has no
        // uninitialized padding on supported Linux 32/64-bit ABIs.
        unsafe { std::slice::from_raw_parts((self as *const Self).cast(), size_of::<Self>()) }
    }
}
fn ioctl(fd: RawFd, request: libc::c_ulong, value: libc::c_int) -> Result<()> {
    // These requests take scalar integer arguments, not pointers.
    ensure!(
        unsafe { libc::ioctl(fd, request, value) } >= 0,
        "Linux input ioctl failed: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
struct Source {
    file: File,
    grabbed: bool,
}
impl Drop for Source {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = ioctl(self.file.as_raw_fd(), 0x40044590, 0);
        }
    }
}
fn open_source(path: &str) -> Result<Source> {
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot resolve input device {path}"))?;
    ensure!(
        canonical.parent() == Some(Path::new("/dev/input"))
            && canonical
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("event")),
        "Select an evdev event device"
    );
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&canonical)
        .with_context(|| {
            format!(
                "Cannot read {}. Grant this user input access; do not run the GUI as root",
                canonical.display()
            )
        })?;
    // Refuse the app's virtual keyboard and any keys held during the grab.
    let mut name = [0u8; 128];
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            0x80804506u64 as libc::c_ulong,
            name.as_mut_ptr(),
        )
    };
    ensure!(
        result >= 0 && !String::from_utf8_lossy(&name).starts_with("MonsGeek host remapper"),
        "Cannot use this input device (or it is the remapper itself)"
    );
    let mut kinds = [0u8; 4];
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            0x80044520u64 as libc::c_ulong,
            kinds.as_mut_ptr(),
        )
    };
    ensure!(
        result >= 0 && kinds[0] & (1 << 1) != 0 && kinds[0] & ((1 << 3) | (1 << 5)) == 0,
        "Select a keyboard or relative mouse device, not a touch/tablet/switch device"
    );
    ensure_released(&file)?;
    Ok(Source {
        file,
        grabbed: false,
    })
}
fn ensure_released(file: &File) -> Result<()> {
    let mut held = [0u8; 96];
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            0x80604518u64 as libc::c_ulong,
            held.as_mut_ptr(),
        )
    };
    ensure!(
        result >= 0 && held.iter().all(|b| *b == 0),
        "Release all keys and mouse buttons before starting the service"
    );
    Ok(())
}

#[repr(C)]
struct InputId {
    bus: u16,
    vendor: u16,
    product: u16,
    version: u16,
}
#[repr(C)]
struct Setup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}
struct VirtualDevice(File);
impl VirtualDevice {
    fn new() -> Result<Self> {
        let file = OpenOptions::new().write(true).open("/dev/uinput").context(
            "Cannot open /dev/uinput. Grant this user uinput access before starting host remapping",
        )?;
        for kind in [1, 2] {
            ioctl(file.as_raw_fd(), 0x40045564, kind)?;
        }
        for key in 1..768 {
            ioctl(file.as_raw_fd(), 0x40045565, key)?;
        }
        for rel in 0..16 {
            ioctl(file.as_raw_fd(), 0x40045566, rel)?;
        }
        let mut setup = Setup {
            id: InputId {
                bus: 3,
                vendor: 0,
                product: 0,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name = b"MonsGeek host remapper";
        setup.name[..name.len()].copy_from_slice(name);
        ensure!(
            unsafe { libc::ioctl(file.as_raw_fd(), 0x405c5503u64 as libc::c_ulong, &setup) } >= 0,
            "Cannot set up virtual keyboard: {}",
            std::io::Error::last_os_error()
        );
        ioctl(file.as_raw_fd(), 0x5501, 0)?;
        Ok(Self(file))
    }
    fn event(&mut self, event: Event) -> Result<()> {
        self.0.write_all(event.bytes())?;
        Ok(())
    }
    fn outputs(&mut self, outputs: Vec<Output>) -> Result<()> {
        if outputs.is_empty() {
            return Ok(());
        }
        for output in outputs {
            match output {
                Output::Key { code, value } => {
                    self.event(Event::new(1, code, value))?;
                    self.event(Event::new(0, 0, 0))?;
                }
                Output::Launch { argv } => {
                    // No shell interpolation. Children get no input stream or
                    // inherited output; detached reapers prevent zombies.
                    let mut child = Command::new(&argv[0])
                        .args(&argv[1..])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .with_context(|| format!("Cannot launch {}", argv[0]))?;
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
            }
        }
        Ok(())
    }
}
impl Drop for VirtualDevice {
    fn drop(&mut self) {
        let _ = ioctl(self.0.as_raw_fd(), 0x5502, 0);
    }
}

#[derive(Debug, Serialize)]
pub struct Device {
    pub path: PathBuf,
    pub name: String,
}
pub fn devices() -> Result<Vec<Device>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/sys/class/input")? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("event") {
            let name = std::fs::read_to_string(entry.path().join("device/name"))
                .unwrap_or_default()
                .trim()
                .to_owned();
            out.push(Device {
                path: Path::new("/dev/input").join(entry.file_name()),
                name,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Run a small focus-query utility with a hard deadline. stdout is bounded by
/// the pipe; supported providers return small responses except Sway's tree.
fn command_output(argv: &[String]) -> Result<String> {
    ensure!(!argv.is_empty(), "Empty focus command");
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().context("No focus-query output")?;
    let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        let _ = child.kill();
        let _ = child.wait();
        bail!("Cannot read focus query without blocking");
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0u8; 8192];
        match stdout.read(&mut chunk) {
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.into());
            }
        }
        if bytes.len() > 1024 * 1024 || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Focus query exceeded its time or output limit");
        }
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Focus query failed");
            // Drain only immediately available data; inherited stdout handles
            // in grandchildren cannot hold this worker open indefinitely.
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.into()),
                }
                ensure!(bytes.len() <= 1024 * 1024, "Focus response too large");
            }
            return Ok(String::from_utf8(bytes)?.trim().to_owned());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct FocusWatch {
    app: std::sync::Arc<std::sync::Mutex<String>>,
    stop: std::sync::Arc<AtomicBool>,
}
impl FocusWatch {
    fn start(command: Option<Vec<String>>) -> Result<Self> {
        let initial = focused_app(command.as_deref())?;
        let app = std::sync::Arc::new(std::sync::Mutex::new(initial));
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let state = app.clone();
        let stopped = stop.clone();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let current = focused_app(command.as_deref()).unwrap_or_default();
                *state.lock().unwrap_or_else(|e| e.into_inner()) = current;
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        Ok(Self { app, stop })
    }
}
impl Drop for FocusWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn terminate(pid: u32) {
    if pid > 1
        && let Ok(pid) = libc::pid_t::try_from(pid)
    {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

fn strings(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|s| (*s).into()).collect()
}
fn find_focused(value: &serde_json::Value) -> Option<String> {
    if value.get("focused").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(id) = value.get("app_id").and_then(|v| v.as_str()) {
            return Some(id.into());
        }
        if let Some(id) = value
            .pointer("/window_properties/class")
            .and_then(|v| v.as_str())
        {
            return Some(id.into());
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(nodes) = value.get(key).and_then(|v| v.as_array()) {
            for node in nodes {
                if let Some(id) = find_focused(node) {
                    return Some(id);
                }
            }
        }
    }
    None
}
pub fn focused_app(custom: Option<&[String]>) -> Result<String> {
    if let Some(argv) = custom {
        return command_output(argv);
    }
    if std::env::var_os("NIRI_SOCKET").is_some() {
        let value: serde_json::Value = serde_json::from_str(&command_output(&strings(&[
            "niri",
            "msg",
            "--json",
            "focused-window",
        ]))?)?;
        return Ok(value
            .get("app_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into());
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        let value: serde_json::Value = serde_json::from_str(&command_output(&strings(&[
            "hyprctl",
            "activewindow",
            "-j",
        ]))?)?;
        return Ok(value
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into());
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        let value: serde_json::Value = serde_json::from_str(&command_output(&strings(&[
            "swaymsg", "-t", "get_tree", "-r",
        ]))?)?;
        return Ok(find_focused(&value).unwrap_or_default());
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some() {
        let active = command_output(&strings(&["xprop", "-root", "_NET_ACTIVE_WINDOW"]))?;
        let id = active
            .split_whitespace()
            .last()
            .context("No active X11 window")?;
        ensure!(
            id.starts_with("0x") && id[2..].chars().all(|c| c.is_ascii_hexdigit()),
            "Invalid X11 window ID"
        );
        let class = command_output(&strings(&["xprop", "-id", id, "WM_CLASS"]))?;
        return Ok(class.split('"').nth(3).unwrap_or_default().into());
    }
    bail!("This desktop needs an explicit focus_command for application-specific rules")
}

pub fn run(config: Config) -> Result<()> {
    config.validate()?;
    if let Some(expected) = std::env::var_os("MONSGEEK_HOST_PARENT") {
        let expected: libc::pid_t = expected.to_str().context("Invalid parent PID")?.parse()?;
        ensure!(
            unsafe { libc::getppid() } == expected,
            "Host service parent changed before startup"
        );
        ensure!(
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } == 0,
            "Cannot bind host service to the GUI lifetime"
        );
        ensure!(
            unsafe { libc::getppid() } == expected,
            "GUI exited during host startup"
        );
    }
    let has_app_rules = config.rules.iter().any(|r| r.app.is_some())
        || config.chords.iter().any(|r| r.app.is_some());
    // Focus-provider failures never activate an app-specific mapping globally.
    let focus = if has_app_rules {
        Some(FocusWatch::start(config.focus_command.clone())?)
    } else {
        None
    };
    let mut sources = config
        .devices
        .iter()
        .map(|p| open_source(p))
        .collect::<Result<Vec<_>>>()?;
    let canonical: BTreeSet<_> = config
        .devices
        .iter()
        .map(std::fs::canonicalize)
        .collect::<std::io::Result<_>>()?;
    ensure!(
        canonical.len() == sources.len(),
        "Several paths point to the same event device"
    );
    let mut virtual_device = VirtualDevice::new()?;
    for source in &mut sources {
        ioctl(source.file.as_raw_fd(), 0x40044590, 1)?;
        source.grabbed = true;
        ensure_released(&source.file)?;
    }
    STOP.store(false, Ordering::Relaxed);
    // sig_atomic-like handler does nothing except set an atomic flag.
    let old_int = unsafe { libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t) };
    let old_term = unsafe { libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t) };
    let mut engine = Engine::new(config.clone())?;
    let start = Instant::now();
    let mut physical = std::collections::BTreeSet::new();

    let result = (|| -> Result<()> {
        eprintln!("Host remapping active. Ctrl+Alt+Esc stops it. No keystrokes are logged.");
        while !STOP.load(Ordering::Relaxed) {
            let app = focus
                .as_ref()
                .map(|f| f.app.lock().unwrap_or_else(|e| e.into_inner()).clone())
                .unwrap_or_default();
            let mut fds: Vec<_> = sources
                .iter()
                .map(|s| libc::pollfd {
                    fd: s.file.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let polled = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 5) };
            if polled < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                bail!("Input poll failed");
            }
            for (source_index, (source, fd)) in sources.iter_mut().zip(fds).enumerate() {
                ensure!(
                    fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) == 0,
                    "Input device disconnected; host remapping stopped"
                );
                if fd.revents & libc::POLLIN == 0 {
                    continue;
                }
                loop {
                    if STOP.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut event = Event::new(0, 0, 0);
                    let n = unsafe {
                        libc::read(
                            source.file.as_raw_fd(),
                            (&mut event as *mut Event).cast(),
                            size_of::<Event>(),
                        )
                    };
                    if n < 0
                        && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock
                    {
                        break;
                    }
                    ensure!(
                        n == size_of::<Event>() as isize,
                        "Incomplete evdev event; host remapping stopped"
                    );
                    let now = start.elapsed().as_millis() as u64;
                    match event.kind {
                        0 if event.code == 3 => bail!(
                            "Input events were dropped; host remapping stopped to release keys"
                        ),
                        1 => {
                            let was_held = physical.iter().any(|(_, code)| *code == event.code);
                            if event.value == 1 {
                                physical.insert((source_index, event.code));
                            }
                            if event.value == 0 {
                                physical.remove(&(source_index, event.code));
                            }
                            let held = |code| physical.iter().any(|(_, key)| *key == code);
                            if held(1) && (held(29) || held(97)) && (held(56) || held(100)) {
                                STOP.store(true, Ordering::Relaxed);
                                break;
                            }
                            if event.value == 1 && was_held || event.value == 0 && held(event.code)
                            {
                                continue;
                            }
                            virtual_device.outputs(engine.event(
                                event.code,
                                event.value,
                                now,
                                &app,
                            ))?;
                        }
                        2 => {
                            virtual_device.event(event)?;
                            virtual_device.event(Event::new(0, 0, 0))?;
                        }
                        _ => {}
                    }
                }
            }
            virtual_device.outputs(engine.tick(start.elapsed().as_millis() as u64))?;
        }
        Ok(())
    })();
    let released = virtual_device.outputs(engine.release_all());
    unsafe {
        libc::signal(libc::SIGINT, old_int);
        libc::signal(libc::SIGTERM, old_term);
    }
    result.and(released)
}

use std::collections::BTreeSet;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_commands_are_literal_and_bounded() {
        assert_eq!(
            command_output(&strings(&[
                "/usr/bin/printf",
                "%s",
                "literal $(not-executed)"
            ]))
            .unwrap(),
            "literal $(not-executed)"
        );
        let start = Instant::now();
        assert!(command_output(&strings(&["/usr/bin/sleep", "2"])).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn sway_focus_uses_window_application_identity() {
        let value = serde_json::json!({"nodes":[{"focused":false,"app_id":"other"},{"focused":true,"window_properties":{"class":"Editor"}}]});
        assert_eq!(find_focused(&value).as_deref(), Some("Editor"));
        assert_eq!(find_focused(&serde_json::json!({"nodes":[]})), None);
    }
}
