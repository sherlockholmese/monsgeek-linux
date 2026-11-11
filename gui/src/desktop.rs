//! Desktop integration stays optional; tray actions stage profiles in the GUI.
use anyhow::{Context, Result};
use ksni::blocking::TrayMethods;
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};

#[derive(Debug)]
pub enum Event {
    Open,
    Profile(PathBuf),
    StopHost,
    Quit,
}
#[derive(Debug)]
pub struct Tray {
    pub title: String,
    pub profiles: Vec<(PathBuf, String)>,
    sender: Sender<Event>,
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "monsgeek-linux".into()
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn icon_name(&self) -> String {
        "input-keyboard".into()
    }
    fn activate(&mut self, _: i32, _: i32) {
        let _ = self.sender.send(Event::Open);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let mut items = vec![
            StandardItem {
                label: "Open MonsGeek M1W".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sender.send(Event::Open);
                }),
                ..Default::default()
            }
            .into(),
        ];
        items.push(
            SubMenu {
                label: "Load profile for review".into(),
                submenu: self
                    .profiles
                    .iter()
                    .map(|(path, name)| {
                        let path = path.clone();
                        StandardItem {
                            label: name.clone(),
                            activate: Box::new(move |this: &mut Self| {
                                let _ = this.sender.send(Event::Profile(path.clone()));
                            }),
                            ..Default::default()
                        }
                        .into()
                    })
                    .collect(),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Stop host remapping".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sender.send(Event::StopHost);
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(ksni::MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sender.send(Event::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}
pub struct Bridge {
    pub handle: ksni::blocking::Handle<Tray>,
    pub events: Receiver<Event>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}
pub fn tray(profiles: Vec<(PathBuf, String)>) -> Result<Bridge> {
    let (sender, events) = mpsc::channel();
    let handle = Tray {
        title: "MonsGeek M1W".into(),
        profiles,
        sender,
    }
    .spawn()?;
    Ok(Bridge { handle, events })
}
pub fn notify(battery: u8) -> Result<()> {
    notify_rust::Notification::new()
        .appname("MonsGeek M1W")
        .summary("Keyboard battery is low")
        .body(&format!(
            "{battery}% remaining. Connect the USB cable to charge."
        ))
        .icon("input-keyboard")
        .show()?;
    Ok(())
}
fn autostart_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("HOME is not set")?;
    Ok(base.join("autostart/monsgeek-linux.desktop"))
}
pub fn autostart_enabled() -> bool {
    autostart_path().is_ok_and(|p| p.is_file())
}
pub fn set_autostart(enabled: bool, executable: &Path) -> Result<()> {
    let path = autostart_path()?;
    if !enabled {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    let executable = executable
        .to_str()
        .context("Executable path is not UTF-8")?;
    anyhow::ensure!(
        !executable.chars().any(char::is_control),
        "Invalid executable path"
    );
    let escaped = executable
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    std::fs::create_dir_all(path.parent().context("No autostart directory")?)?;
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=MonsGeek M1W\nExec=\"{escaped}\"\nIcon=input-keyboard\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    let tmp = path.with_extension("desktop.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub struct HostProcess(
    pub std::process::Child,
    pub std::sync::Arc<std::sync::Mutex<String>>,
);
pub fn spawn_host(binary: &Path, config: &Path) -> Result<HostProcess> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        sync::{Arc, Mutex},
    };
    let mut child = Command::new(binary)
        .arg("host")
        .arg("run")
        .arg(config)
        .arg("--activate")
        .env("MONSGEEK_HOST_PARENT", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        let captured = output.clone();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.take(16384).read_to_string(&mut text);
            *captured.lock().unwrap_or_else(|e| e.into_inner()) = text;
        });
    }
    Ok(HostProcess(child, output))
}
impl Drop for HostProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(None)) {
            return;
        }
        monsgeek_linux::host::linux::terminate(self.0.id());
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if !matches!(self.0.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
