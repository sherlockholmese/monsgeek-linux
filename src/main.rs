use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use hidapi::HidApi;
use monsgeek_linux::{
    access,
    config::{Binding, Macro, SleepTimers, source_slot},
    device::{Keyboard, Snapshot},
    firmware::{self, Image, Progress},
    protocol::{self, Change, Lighting},
    transaction::{self, BeforeState, picture},
    transport::{self, Kind},
};
use std::{ffi::CString, fs, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Original M1W configurator. Bluetooth is experimental. Changes preview unless --apply is given."
)]
struct Args {
    #[arg(long, global = true)]
    device: Option<String>,
    /// Restrict discovery to USB, receiver or experimental Bluetooth.
    #[arg(long, global = true, value_enum)]
    transport: Option<Kind>,
    #[arg(long, global = true, requires_all = ["allow_experimental_writes", "before"])]
    apply: bool,
    #[arg(long, global = true, requires = "apply")]
    allow_experimental_writes: bool,
    /// New owner-only before-state/undo file. Never overwritten.
    #[arg(long, global = true, requires = "apply")]
    before: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Experimental USB firmware update. Preview only unless --apply is given.
    Firmware {
        /// Decompressed vendor firmwareFile.bin for original M1W (ID983).
        file: PathBuf,
        /// Expected SHA-256 from the offline preview. Mandatory when applying.
        #[arg(long)]
        sha256: Option<String>,
        /// Accept experimental flashing and the risk of bricking the keyboard.
        #[arg(long)]
        allow_experimental_firmware: bool,
    },
    List,
    /// Install the udev rule that lets this login open the keyboard without root
    InstallRule,
    /// Wireless battery (Bluetooth is experimental)
    Battery,
    Inspect,
    /// Export all maps, settings, RGB and 50 raw macro slots.
    Snapshot {
        file: PathBuf,
    },
    Profile {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=3))]
        layer: u8,
    },
    Light {
        #[arg(long)]
        mode: u8,
        #[arg(long, default_value = "2")]
        speed: u8,
        #[arg(long, default_value = "4")]
        brightness: u8,
        #[arg(long, default_value = "0")]
        option: u8,
        #[arg(long)]
        rainbow: bool,
        #[arg(long, default_value = "ffffff", value_parser = rgb)]
        color: [u8; 3],
    },
    Remap {
        #[arg(long)]
        from: String,
        #[arg(long, value_parser = key)]
        to: u8,
        #[arg(long, default_value = "0")]
        layer: u8,
        #[arg(long)]
        fn_layer: bool,
    },
    /// Assign a JSON Binding (keyboard combo, consumer, macro, function, disabled or raw).
    Bind {
        #[arg(long)]
        from: String,
        #[arg(long)]
        binding: String,
        #[arg(long, default_value = "0")]
        layer: u8,
        #[arg(long)]
        fn_layer: bool,
    },
    Debounce {
        value: u8,
    },
    Sleep {
        #[arg(long)]
        bt: u16,
        #[arg(long)]
        receiver: u16,
        #[arg(long)]
        deep_bt: u16,
        #[arg(long)]
        deep_receiver: u16,
    },
    /// Raw-preserving option edit from a snapshot. Known fields only, all other bits retained.
    Option {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long, value_parser = ["win-lock", "wasd-swap", "led-off", "keyboard-lock", "power-save"])]
        name: String,
        #[arg(long, action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Automatic OS detection (verified setting round trip on revision0108).
    AutoOs {
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Read raw/decoded macro, or upload semantic JSON with --file, or clear with --clear.
    Macro {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=49))]
        slot: u8,
        #[arg(long, conflicts_with = "clear")]
        file: Option<PathBuf>,
        #[arg(long)]
        clear: bool,
    },
    /// Read RGB, or replace all 128 RGB triples from JSON with --file.
    Rgb {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Edit one key's RGB using a saved snapshot; all other slots are retained.
    Paint {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        key: String,
        #[arg(long, value_parser = rgb)]
        color: [u8; 3],
    },
    /// Preview/apply an ordered JSON array of Changes (macros before their bindings).
    Plan {
        file: PathBuf,
    },
    /// Restore changed fields only; --current permits a fully offline comparison.
    Restore {
        file: PathBuf,
        #[arg(long)]
        current: Option<PathBuf>,
    },
    Undo {
        file: PathBuf,
    },
    /// List source slots and concrete vendor defaults, not current assignments.
    Keys,
}

fn rgb(text: &str) -> Result<[u8; 3], String> {
    let text = text.strip_prefix('#').unwrap_or(text);
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("color must be six hex digits".into());
    }
    let n = u32::from_str_radix(text, 16).map_err(|e| e.to_string())?;
    Ok([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn key(text: &str) -> Result<u8, String> {
    let text = text.to_ascii_lowercase();
    let n = match text.as_str() {
        "esc" => 41,
        "enter" => 40,
        "space" => 44,
        "tab" => 43,
        "backspace" => 42,
        "capslock" => 57,
        "leftctrl" => 224,
        "leftshift" => 225,
        "leftalt" => 226,
        "leftmeta" => 227,
        "rightctrl" => 228,
        "rightshift" => 229,
        "rightalt" => 230,
        "rightmeta" => 231,
        "right" => 79,
        "left" => 80,
        "down" => 81,
        "up" => 82,
        _ if text.len() == 1 && text.as_bytes()[0].is_ascii_lowercase() => {
            text.as_bytes()[0] - b'a' + 4
        }
        _ => if let Some(n) = text.strip_prefix("0x") {
            u8::from_str_radix(n, 16)
        } else {
            text.parse()
        }
        .map_err(|_| "invalid keyboard usage".to_string())?,
    };
    if n < 4 {
        return Err("reserved keyboard usages 0..3 are not allowed".into());
    }
    Ok(n)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("cannot read {}", path.display()))?,
    )
    .with_context(|| format!("invalid JSON in {}", path.display()))
}

fn load_snapshot(path: &PathBuf) -> Result<Snapshot> {
    let s: Snapshot = read_json(path)?;
    s.validate()?;
    Ok(s)
}

fn offline_changes(cmd: &Command) -> Result<Option<Vec<Change>>> {
    let change = match cmd {
        Command::Profile { layer } => Change::Profile { layer: *layer },
        Command::Light {
            mode,
            speed,
            brightness,
            option,
            rainbow,
            color,
        } => Change::Lighting {
            settings: Lighting::new(*mode, *speed, *brightness, *option, *rainbow, *color)?,
        },
        Command::Remap {
            from,
            to,
            layer,
            fn_layer,
        } => Change::Remap {
            layer: *layer,
            fn_layer: *fn_layer,
            slot: source_slot(from)?,
            binding: [0, 0, *to, 0],
        },
        Command::Bind {
            from,
            binding,
            layer,
            fn_layer,
        } => Change::Remap {
            layer: *layer,
            fn_layer: *fn_layer,
            slot: source_slot(from)?,
            binding: serde_json::from_str::<Binding>(binding)?.encode()?,
        },
        Command::Debounce { value } => Change::Debounce { value: *value },
        Command::Sleep {
            bt,
            receiver,
            deep_bt,
            deep_receiver,
        } => Change::Sleep {
            timers: SleepTimers {
                time_bt: *bt,
                time_24: *receiver,
                deep_time_bt: *deep_bt,
                deep_time_24: *deep_receiver,
            },
        },
        Command::AutoOs { enabled } => Change::AutoOs { enabled: *enabled },
        Command::Option {
            snapshot,
            name,
            enabled,
        } => {
            let s = load_snapshot(snapshot)?;
            let mut bytes = s.onboard.context("snapshot lacks raw options")?.options;
            let (index, mask) = match name.as_str() {
                "win-lock" => (0, 1),
                "wasd-swap" => (0, 8),
                "led-off" => (0, 16),
                "keyboard-lock" => (0, 128),
                "power-save" => (2, 1),
                _ => unreachable!(),
            };
            if *enabled {
                bytes[index] |= mask;
            } else {
                bytes[index] &= !mask;
            }
            Change::Options { bytes }
        }
        Command::Macro {
            slot,
            file: Some(file),
            ..
        } => Change::Macro {
            slot: *slot,
            data: read_json::<Macro>(file)?.encode()?,
        },
        Command::Macro {
            slot, clear: true, ..
        } => Change::Macro {
            slot: *slot,
            data: vec![0; 256],
        },
        Command::Rgb { file: Some(file) } => {
            let colors: Vec<[u8; 3]> = read_json(file)?;
            ensure!(colors.len() == 128, "RGB file must contain128 RGB triples");
            Change::Rgb {
                data: colors.into_iter().flatten().collect(),
            }
        }
        Command::Paint {
            snapshot,
            key,
            color,
        } => {
            let s = load_snapshot(snapshot)?;
            let mut data = s.onboard.context("snapshot lacks RGB")?.rgb;
            let start = usize::from(source_slot(key)?) * 3;
            data[start..start + 3].copy_from_slice(color);
            Change::Rgb { data }
        }
        Command::Plan { file } => return Ok(Some(read_json(file)?)),
        Command::Undo { file } => {
            let r: BeforeState = read_json(file)?;
            r.snapshot.validate()?;
            return Ok(Some(r.changes()));
        }
        Command::Restore {
            file,
            current: Some(current),
        } => {
            return Ok(Some(
                load_snapshot(current)?.restore_changes(&load_snapshot(file)?)?,
            ));
        }
        _ => return Ok(None),
    };
    Ok(Some(vec![change]))
}

fn preview(changes: &[Change]) -> Result<()> {
    let packets: Vec<_> = changes
        .iter()
        .map(|c| {
            c.packets().map(|packets| {
                packets
                    .iter()
                    .map(|p| {
                        p.iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Result<_>>()?;
    let mut out = serde_json::json!({"device_id":protocol::DEVICE_ID,"changes":changes,"packets_hex":packets});
    if changes.len() == 1 {
        out["change"] = serde_json::to_value(&changes[0])?;
        if packets[0].len() == 1 {
            out["packet_hex"] = serde_json::json!(packets[0][0]);
        }
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    if matches!(args.command, Command::Firmware { .. }) {
        return firmware_command(&args);
    }
    let mut changes = offline_changes(&args.command)?;
    if let Some(changes) = &changes {
        for c in changes {
            c.packets()?;
        }
        preview(changes)?;
        if !args.apply {
            eprintln!("Preview only. No HID device was opened and no command was sent.");
            return Ok(());
        }
    } else {
        ensure!(
            !args.apply || matches!(args.command, Command::Restore { .. }),
            "--apply requires a setting change or restore"
        );
    }
    if matches!(args.command, Command::Keys) {
        println!("{}", include_str!("../docs/m1w-reference.json"));
        return Ok(());
    }
    if matches!(args.command, Command::InstallRule) {
        return install_rule();
    }
    if !matches!(args.command, Command::List) {
        offer_rule();
    }
    let api = HidApi::new().context("cannot enumerate HID devices")?;
    if matches!(args.command, Command::List) {
        for d in api.device_list().filter(|d| d.vendor_id() == protocol::VID) {
            println!(
                "{} {:04x}:{:04x} interface={} usage={:04x}:{:04x} {}{}",
                d.path().to_string_lossy(),
                d.vendor_id(),
                d.product_id(),
                d.interface_number(),
                d.usage_page(),
                d.usage(),
                d.product_string().unwrap_or(""),
                match transport::kind_of(d) {
                    Some(Kind::Usb) => " [M1W candidate; identity not checked]",
                    Some(Kind::Dongle) => " [2.4 GHz receiver]",
                    Some(Kind::Bluetooth) =>
                        " [Bluetooth candidate, experimental; identity not checked]",
                    None => "",
                }
            );
        }
        return Ok(());
    }
    let path = args.device.as_deref().map(CString::new).transpose()?;
    if matches!(args.command, Command::Battery) && path.is_none() && args.transport.is_none() {
        match monsgeek_linux::device::wireless_status(&api)? {
            Some((_, Some(battery))) => println!("{battery}%"),
            Some((_, None)) => println!("keyboard asleep or out of range"),
            None => anyhow::bail!("no wireless keyboard found"),
        }
        return Ok(());
    }
    let keyboard =
        Keyboard::open_with_transport(&api, path.as_deref(), args.transport).map_err(|e| {
            if access::is_permission_error(&e) {
                e.context("no access to the keyboard; run `monsgeek-linux install-rule`")
            } else {
                e
            }
        })?;
    match &args.command {
        Command::Battery => {
            match keyboard.battery()? {
                Some(battery) => println!("{battery}%"),
                None if keyboard.kind() == Kind::Usb => println!("battery unavailable over USB"),
                None => println!("keyboard asleep or out of range"),
            }
            return Ok(());
        }
        Command::Inspect => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"device_id":protocol::DEVICE_ID,"active_layer":keyboard.active_layer()?,"lighting":keyboard.lighting()?,"debounce":keyboard.debounce()?,"sleep":keyboard.sleep_timers()?,"raw_options":keyboard.options()?,"auto_os":keyboard.auto_os()?})
                )?
            );
            return Ok(());
        }
        Command::Snapshot { file } => {
            transaction::save_new(file, &keyboard.snapshot()?)?;
            println!("Snapshot saved to {}", file.display());
            return Ok(());
        }
        Command::Macro {
            slot,
            file: None,
            clear: false,
        } => {
            let data = keyboard.macro_data(*slot)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"slot":slot,"raw":data,"decoded":Macro::decode(&data).ok()})
                )?
            );
            return Ok(());
        }
        Command::Rgb { file: None } => {
            let data = keyboard.rgb()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&data.as_chunks::<3>().0)?
            );
            return Ok(());
        }
        Command::Restore { file, current } => {
            let actual = keyboard.snapshot()?;
            if let Some(current) = current {
                ensure!(
                    actual == load_snapshot(current)?,
                    "--current is stale; regenerate restore preview"
                );
            }
            changes = Some(actual.restore_changes(&load_snapshot(file)?)?);
            if current.is_none() {
                preview(changes.as_ref().unwrap())?;
            }
            if !args.apply {
                eprintln!("Preview only; read current device state, no settings sent.");
                return Ok(());
            }
        }
        _ => {}
    }
    let changes = changes.context("missing changes")?;
    if changes.is_empty() {
        println!("No differences; no setting writes needed.");
        return Ok(());
    }
    let snapshot = keyboard.snapshot()?;
    transaction::check_picture(&changes, &snapshot)?;
    if let Command::Undo { file } = &args.command {
        transaction::check_undo(&read_json(file)?, &snapshot)?;
    }
    // A saved-baseline edit must never overwrite changes made since that baseline.
    if let Command::Option { snapshot: base, .. } | Command::Paint { snapshot: base, .. } =
        &args.command
    {
        let base = load_snapshot(base)?;
        let a = snapshot.onboard.as_ref().context("missing onboard state")?;
        let b = base
            .onboard
            .as_ref()
            .context("missing baseline onboard state")?;
        ensure!(
            snapshot.usb_revision == base.usb_revision,
            "baseline revision mismatch"
        );
        ensure!(
            if matches!(args.command, Command::Paint { .. }) {
                a.rgb == b.rgb && picture(&snapshot.lighting) == picture(&base.lighting)
            } else {
                a.options == b.options
            },
            "baseline is stale; capture a new snapshot before editing"
        );
    }
    let before = args.before.context("--before is required")?;
    eprintln!(
        "Before-state will be saved to {}. Sending {} changes; stop on first mismatch.",
        before.display(),
        changes.len()
    );
    transaction::commit(&keyboard, snapshot, &changes, &before)?;
    println!(
        "Requested settings match read-back. Physical behavior and power-cycle persistence are separate checks."
    );
    Ok(())
}

fn firmware_command(args: &Args) -> Result<()> {
    let Command::Firmware {
        file,
        sha256,
        allow_experimental_firmware,
    } = &args.command
    else {
        unreachable!()
    };
    ensure!(
        !*allow_experimental_firmware || args.apply,
        "--allow-experimental-firmware requires --apply"
    );
    ensure!(
        args.transport.is_none_or(|kind| kind == Kind::Usb),
        "firmware updates require --transport usb"
    );
    let image = Image::load(file)?;
    if let Some(hash) = sha256 {
        image.check_sha256(hash)?;
    }
    eprintln!("Warning: {}", firmware::WARNING);
    let hex = |packet: [u8; 64]| {
        packet
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "firmware": image.info(),
            "packets_hex": {
                "boot": hex(firmware::boot_packet()),
                "prepare": hex(image.prepare_packet()),
                "finish": hex(image.finish_packet()),
            },
        }))?
    );
    if !args.apply {
        eprintln!("Preview only. No HID device was opened and no command was sent.");
        return Ok(());
    }
    ensure!(
        *allow_experimental_firmware,
        "flashing requires --allow-experimental-firmware"
    );
    let hash = sha256
        .as_deref()
        .context("flashing requires --sha256 from the preview")?;
    let before = args.before.as_ref().context("--before is required")?;
    let path = args.device.as_deref().map(CString::new).transpose()?;
    offer_rule();
    let mut api = HidApi::new().context("cannot enumerate HID devices")?;
    let outcome = firmware::flash(
        &mut api,
        &image,
        hash,
        path.as_deref(),
        before,
        None,
        |state| match state {
            Progress::BackingUp => eprintln!("Saving configuration to {}", before.display()),
            Progress::EnteringBoot => eprintln!("Entering USB bootloader. Do not disconnect."),
            Progress::Preparing => eprintln!("Preparing firmware"),
            Progress::Writing { done, total } if done == total || done == 1 || done % 128 == 0 => {
                eprintln!("Firmware {done}/{total} blocks")
            }
            Progress::Writing { .. } => {}
            Progress::Verifying => eprintln!("Checking bootloader completion"),
            Progress::Reconnecting => eprintln!("Waiting for the keyboard"),
        },
    )?;
    println!(
        "Bootloader confirmed completion; original M1W reidentified. Firmware {:04x} -> {:04x}. Configuration was not restored automatically.",
        outcome.old_revision, outcome.new_revision
    );
    Ok(())
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Install the access rule, asking for the password in this terminal.
fn install_rule() -> Result<()> {
    let elevate = if in_path("sudo") {
        access::Elevate::Sudo
    } else {
        access::Elevate::Pkexec
    };
    eprintln!("Installing {}:\n\n{}", access::PATH, access::RULE);
    access::install(elevate)?;
    eprintln!("Installed. Reconnect the keyboard if access is still denied.");
    Ok(())
}

/// Offer the rule once per run in an interactive terminal until it is
/// installed or declined with `never`.
fn offer_rule() {
    use std::io::IsTerminal;
    if !access::should_offer()
        || !std::io::stdin().is_terminal()
        || !std::io::stderr().is_terminal()
    {
        return;
    }
    let state = match access::status() {
        access::Status::Outdated => "is out of date",
        _ => "is not installed",
    };
    eprint!(
        "The device access rule {} {state}. Install it now? [y/N/never] ",
        access::PATH
    );
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return;
    }
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => {
            if let Err(e) = install_rule() {
                eprintln!("Rule not installed: {e:#}");
            }
        }
        "never" => {
            if let Err(e) = access::decline() {
                eprintln!("Cannot record the choice: {e:#}");
            }
        }
        _ => {}
    }
}
