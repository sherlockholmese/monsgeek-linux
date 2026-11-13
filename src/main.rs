use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use hidapi::HidApi;
use monsgeek_linux::{
    access,
    config::{Binding, Macro, SleepTimers},
    device::{Keyboard, Snapshot},
    firmware::{self, Image, Progress},
    protocol::{Change, Lighting},
    transaction::{self, BeforeState, picture},
    transport::{self, Kind},
};
use std::{ffi::CString, fs, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "MonsGeek keyboard configurator. Bluetooth is experimental. Changes preview unless --apply is given."
)]
struct Args {
    #[arg(long, global = true)]
    device: Option<String>,
    /// Model name or numeric ID for offline previews; verified against hardware on Apply.
    #[arg(long, global = true)]
    model: Option<String>,
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
    /// List supported model IDs, protocol families and layouts without accessing HID.
    Models,
    /// Family-specific setting as JSON (report rate, options, side lighting, magnetic arrays).
    Advanced {
        change: String,
    },
    /// Edit one magnetic key from a saved snapshot, preserving every other key.
    Magnetic {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        from: String,
        #[arg(long, value_parser = ["actuation", "release", "rapid-press", "rapid-release", "bottom-dead-zone", "top-dead-zone", "dynamic-threshold", "tap-hold-ms", "switch-type", "mode", "rapid-trigger", "snap-pair"])]
        setting: String,
        #[arg(long)]
        value: Option<f32>,
        #[arg(long)]
        partner: Option<String>,
    },
    /// Read 128 live travel values (modern mm, legacy normalized calibration ranges).
    Travel,
    /// Begin or finish switch calibration. Requires the normal apply/backup flags.
    Calibrate {
        #[arg(value_parser = ["start", "finish"])]
        action: String,
    },
    /// Manage named local profiles without accessing hardware.
    Profiles {
        #[command(subcommand)]
        command: ProfilesCommand,
    },
    /// Human-readable offline differences between two snapshots.
    Review {
        current: PathBuf,
        target: PathBuf,
    },
    /// Export a privacy-preserving support report; no keymaps or macros.
    Diagnostics {
        #[arg(long)]
        snapshot: Option<PathBuf>,
    },
    /// Capture all three RGB pictures. Apply temporarily changes picture selection.
    Pictures {
        file: PathBuf,
    },
    /// Optional Linux-side tap-hold, tap dance, chords and application rules.
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    /// Experimental USB firmware update. Preview only unless --apply is given.
    Firmware {
        /// Decompressed vendor firmware file for the selected --model.
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
        #[arg(value_parser = clap::value_parser!(u8).range(0..=19))]
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

#[derive(Subcommand)]
enum ProfilesCommand {
    List,
    Save {
        name: String,
        snapshot: PathBuf,
    },
    Duplicate {
        file: PathBuf,
        name: String,
    },
    /// Produce a snapshot draft; use restore to preview/apply it separately.
    Draft {
        file: PathBuf,
        current: PathBuf,
        output: PathBuf,
        #[arg(long, value_delimiter = ',', value_parser = ["keys", "macros", "lighting", "performance", "options", "active-layer"])]
        only: Vec<String>,
    },
}
#[derive(Subcommand)]
enum HostCommand {
    Devices,
    Example {
        file: PathBuf,
    },
    Check {
        file: PathBuf,
    },
    /// Prints a summary unless --activate explicitly permits input grabbing.
    Run {
        file: PathBuf,
        #[arg(long)]
        activate: bool,
    },
    /// Query supported desktop focus providers without capturing input.
    Focus,
}

fn local_command(args: &Args) -> Option<Result<()>> {
    use monsgeek_linux::{diagnostics, host, pictures, profiles, review};
    Some((|| -> Result<()> {
        match &args.command {
            Command::Profiles { command } => {
                ensure!(
                    !args.apply,
                    "Profiles only stage local data; use restore to apply a generated draft"
                );
                let dir = profiles::directory()?;
                match command {
                    ProfilesCommand::List => {
                        for (path, p) in profiles::list(&dir)? {
                            println!("{}\t{}", p.name, path.display());
                        }
                    }
                    ProfilesCommand::Save { name, snapshot } => println!(
                        "{}",
                        profiles::save(
                            &dir,
                            &profiles::Profile::new(name.clone(), load_snapshot(snapshot)?)?
                        )?
                        .display()
                    ),
                    ProfilesCommand::Duplicate { file, name } => println!(
                        "{}",
                        profiles::save(
                            &dir,
                            &profiles::Profile::new(name.clone(), profiles::load(file)?.snapshot)?
                        )?
                        .display()
                    ),
                    ProfilesCommand::Draft {
                        file,
                        current,
                        output,
                        only,
                    } => {
                        let selected = if only.is_empty() {
                            profiles::Selection::default()
                        } else {
                            profiles::Selection {
                                keys: only.iter().any(|v| v == "keys"),
                                macros: only.iter().any(|v| v == "macros"),
                                lighting: only.iter().any(|v| v == "lighting"),
                                performance: only.iter().any(|v| v == "performance"),
                                options: only.iter().any(|v| v == "options"),
                                active_layer: only.iter().any(|v| v == "active-layer"),
                            }
                        };
                        transaction::save_new(
                            output,
                            &profiles::load(file)?.draft(&load_snapshot(current)?, selected)?,
                        )?;
                    }
                }
            }
            Command::Review { current, target } => {
                ensure!(!args.apply, "Review is offline");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&review::rows(
                        &load_snapshot(current)?,
                        &load_snapshot(target)?
                    )?)?
                );
            }
            Command::Diagnostics { snapshot } => {
                ensure!(!args.apply, "Diagnostics do not apply changes");
                let snapshot = snapshot.as_ref().map(load_snapshot).transpose()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&diagnostics::report(
                        snapshot.as_ref(),
                        None,
                        0,
                        Vec::new()
                    ))?
                );
            }
            Command::Pictures { file } => {
                if !args.apply {
                    println!(
                        "{}",
                        serde_json::json!({"preview_only":true,"pictures":args.model.as_deref().map(monsgeek_linux::models::select).transpose()?.unwrap_or_else(monsgeek_linux::models::original).picture_count(),"temporarily_changes_lighting":true,"output":file})
                    );
                } else {
                    let api = HidApi::new()?;
                    let path = args.device.as_deref().map(CString::new).transpose()?;
                    let keyboard =
                        Keyboard::open_with_transport(&api, path.as_deref(), args.transport)?;
                    ensure!(
                        args.model
                            .as_deref()
                            .map(monsgeek_linux::models::select)
                            .transpose()?
                            .is_none_or(|m| m.id == keyboard.model().id),
                        "selected model differs from keyboard"
                    );
                    pictures::capture(
                        &keyboard,
                        file,
                        args.before.as_ref().context("Before-state path required")?,
                    )?;
                    println!("Saved all model pictures; original lighting restored");
                }
            }
            Command::Host { command } => {
                ensure!(
                    !args.apply,
                    "Host remapping uses its own explicit --activate flag"
                );
                match command {
                    HostCommand::Devices => println!(
                        "{}",
                        serde_json::to_string_pretty(&host::linux::devices()?)?
                    ),
                    HostCommand::Focus => println!("{}", host::linux::focused_app(None)?),
                    HostCommand::Example { file } => {
                        transaction::save_new(file, &host::Config::example())?
                    }
                    HostCommand::Check { file }
                    | HostCommand::Run {
                        file,
                        activate: false,
                    } => {
                        let config: host::Config = read_json(file)?;
                        config.validate()?;
                        println!(
                            "Valid host configuration: {} devices, {} rules, {} chords. No input was captured.",
                            config.devices.len(),
                            config.rules.len(),
                            config.chords.len()
                        );
                    }
                    HostCommand::Run {
                        file,
                        activate: true,
                    } => host::linux::run(read_json(file)?)?,
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    })())
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

fn offline_changes(
    cmd: &Command,
    model: &monsgeek_linux::models::Model,
) -> Result<Option<Vec<Change>>> {
    let change = match cmd {
        Command::Magnetic {
            snapshot,
            from,
            setting,
            value,
            partner,
        } => {
            let baseline = load_snapshot(snapshot)?;
            let model = baseline.model()?;
            let slot = model.source_slot(from)?;
            let mut draft = baseline.clone();
            let settings = draft
                .advanced
                .as_mut()
                .context("snapshot lacks magnetic settings")?;
            if setting == "snap-pair" {
                settings.pair(
                    slot,
                    model.source_slot(partner.as_deref().context("--partner is required")?)?,
                )?;
            } else {
                let value = value.context("--value is required")?;
                match setting.as_str() {
                    "mode" => {
                        ensure!(
                            value.fract() == 0. && (0.0..=5.0).contains(&value),
                            "mode must be 0, 2, 3, 4 or 5"
                        );
                        let rapid = settings.magnetic.get(&7).context("mode not captured")?
                            [usize::from(slot)]
                            & 128
                            != 0;
                        settings.set_mode(slot, value as u8, rapid)?;
                    }
                    "rapid-trigger" => {
                        ensure!(value == 0. || value == 1., "rapid-trigger must be 0 or 1");
                        let modes = settings.magnetic.get_mut(&7).context("mode not captured")?;
                        modes[usize::from(slot)] =
                            (modes[usize::from(slot)] & 127) | if value == 1. { 128 } else { 0 };
                    }
                    _ => {
                        let parameter = match setting.as_str() {
                            "actuation" => 0,
                            "release" => 1,
                            "rapid-press" => 2,
                            "rapid-release" => 3,
                            "dynamic-threshold" => 4,
                            "tap-hold-ms" => 5,
                            "bottom-dead-zone" => 6,
                            "top-dead-zone" => 251,
                            "switch-type" => 252,
                            _ => unreachable!(),
                        };
                        if parameter == 252 {
                            ensure!(
                                model.switches.iter().any(|s| f32::from(s.code) == value),
                                "switch type unavailable on this model"
                            );
                        }
                        settings.set_value(model, parameter, slot, value)?;
                    }
                }
            }
            return Ok(Some(baseline.restore_changes(&draft)?));
        }
        Command::Advanced { change } => Change::Advanced {
            change: serde_json::from_str(change)?,
        },
        Command::Profile { layer } => Change::Profile { layer: *layer },
        Command::Light {
            mode,
            speed,
            brightness,
            option,
            rainbow,
            color,
        } => Change::Lighting {
            settings: Lighting::new_for(
                model,
                *mode,
                *speed,
                *brightness,
                *option,
                *rainbow,
                *color,
            )?,
        },
        Command::Remap {
            from,
            to,
            layer,
            fn_layer,
        } => Change::Remap {
            layer: *layer,
            fn_layer: *fn_layer,
            slot: model.source_slot(from)?,
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
            slot: model.source_slot(from)?,
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
            ensure!(
                s.device_id == model.id,
                "snapshot belongs to a different model"
            );
            if model.family == monsgeek_linux::models::Family::Ry5088 {
                ensure!(
                    name == "wasd-swap",
                    "{name} is not a named option on this model; use `advanced` for its supported options"
                );
                let mut bytes = s
                    .advanced
                    .context("snapshot lacks advanced settings")?
                    .options
                    .context("snapshot lacks modern options")?;
                bytes[3] = u8::from(*enabled);
                return Ok(Some(vec![Change::Advanced {
                    change: monsgeek_linux::advanced::Change::Options { bytes },
                }]));
            }
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
            let start = usize::from(s.model()?.source_slot(key)?) * 3;
            let mut data = s.onboard.context("snapshot lacks RGB")?.rgb;
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

fn preview(changes: &[Change], model: &monsgeek_linux::models::Model) -> Result<()> {
    let packets: Vec<_> = changes
        .iter()
        .map(|c| {
            c.packets_for(model).map(|packets| {
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
    let mut out = serde_json::json!({"device_id":model.id,"model":model.name,"changes":changes,"packets_hex":packets});
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
    let selected_model = args
        .model
        .as_deref()
        .map(monsgeek_linux::models::select)
        .transpose()?;
    let mut model = selected_model.unwrap_or_else(monsgeek_linux::models::original);
    if matches!(args.command, Command::Models) {
        for m in monsgeek_linux::models::MODELS.iter() {
            println!(
                "{}\t{}\t{:?}\t{:04x}:{:04x}{}",
                m.id,
                m.name,
                m.family,
                m.vid,
                m.pid,
                if m.magnetic { "\tmagnetic" } else { "" }
            );
        }
        return Ok(());
    }
    // Baseline-backed offline commands carry their own model identity.
    let source = match &args.command {
        Command::Paint { snapshot, .. }
        | Command::Option { snapshot, .. }
        | Command::Magnetic { snapshot, .. } => Some(load_snapshot(snapshot)?),
        Command::Restore { file, .. } => Some(load_snapshot(file)?),
        Command::Undo { file } => Some(read_json::<BeforeState>(file)?.snapshot),
        _ => None,
    };
    if let Some(source) = &source {
        ensure!(
            selected_model.is_none_or(|m| m.id == source.device_id),
            "selected model differs from snapshot"
        );
        model = source.model()?;
    }
    if matches!(
        args.command,
        Command::Profiles { .. }
            | Command::Review { .. }
            | Command::Diagnostics { .. }
            | Command::Pictures { .. }
            | Command::Host { .. }
    ) {
        return local_command(&args).expect("local command");
    }
    if matches!(args.command, Command::Firmware { .. }) {
        return firmware_command(&args);
    }
    if let Command::Calibrate { action } = &args.command {
        if !args.apply {
            println!(
                "{}",
                serde_json::json!({"preview_only": true,"action": action,"model": model.name,"warning":"Calibration changes sensor calibration; a configuration backup cannot restore the old calibration."})
            );
            return Ok(());
        }
        offer_rule();
        let api = HidApi::new()?;
        let path = args.device.as_deref().map(CString::new).transpose()?;
        let keyboard = Keyboard::open_with_transport(&api, path.as_deref(), Some(Kind::Usb))?;
        ensure!(
            selected_model.is_none_or(|m| m.id == keyboard.model().id),
            "selected model differs from keyboard"
        );
        ensure!(
            keyboard.model().magnetic,
            "calibration unavailable on this model"
        );
        transaction::save_new(
            args.before.as_ref().context("--before is required")?,
            &keyboard.snapshot()?,
        )?;
        keyboard.calibrate(action == "start")?;
        println!(
            "{}",
            if action == "start" {
                "Calibration started. Press every key fully, then run calibrate finish with a new before-state path."
            } else {
                "Calibration finish command sent. Inspect travel and test the keys."
            }
        );
        return Ok(());
    }
    let mut changes = if args.apply && selected_model.is_none() && source.is_none() {
        None
    } else {
        offline_changes(&args.command, model)?
    };
    if let Some(changes) = &changes {
        for c in changes {
            c.packets_for(model)?;
        }
        preview(changes, model)?;
        if !args.apply {
            eprintln!("Preview only. No HID device was opened and no command was sent.");
            return Ok(());
        }
    } else {
        ensure!(
            !args.apply
                || matches!(
                    args.command,
                    Command::Restore { .. }
                        | Command::Profile { .. }
                        | Command::Light { .. }
                        | Command::Remap { .. }
                        | Command::Bind { .. }
                        | Command::Debounce { .. }
                        | Command::Sleep { .. }
                        | Command::AutoOs { .. }
                        | Command::Macro { .. }
                        | Command::Rgb { .. }
                        | Command::Advanced { .. }
                        | Command::Plan { .. }
                ),
            "--apply requires a setting change or restore"
        );
    }
    if matches!(args.command, Command::Keys) {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"device_id":model.id,"model":model.name,"matrices":model.matrices})
            )?
        );
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
        for d in api.device_list().filter(|d| {
            monsgeek_linux::models::MODELS
                .iter()
                .any(|m| m.vid == d.vendor_id())
        }) {
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
                    Some(Kind::Usb) => " [keyboard candidate; identity not checked]",
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
    ensure!(
        selected_model.is_none_or(|m| m.id == keyboard.model().id),
        "selected model differs from connected keyboard"
    );
    if let Some(source) = &source {
        ensure!(
            source.device_id == keyboard.model().id,
            "snapshot keyboard model mismatch"
        );
    }
    model = keyboard.model();
    if args.apply {
        changes = offline_changes(&args.command, model)?;
    }
    match &args.command {
        Command::Travel => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"device_id": model.id,"unit": if model.family == monsgeek_linux::models::Family::Ry5088 { "mm" } else { "normalized-calibration-range" }, "values": keyboard.travel()?})
                )?
            );
            return Ok(());
        }
        Command::Battery => {
            match keyboard.battery()? {
                Some(battery) => println!("{battery}%"),
                None if keyboard.kind() == Kind::Usb => println!("battery unavailable over USB"),
                None => println!("keyboard asleep or out of range"),
            }
            return Ok(());
        }
        Command::Inspect => {
            println!("{}", serde_json::to_string_pretty(&keyboard.snapshot()?)?);
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
                preview(changes.as_ref().unwrap(), model)?;
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
    if let Command::Option { snapshot: base, .. }
    | Command::Paint { snapshot: base, .. }
    | Command::Magnetic { snapshot: base, .. } = &args.command
    {
        let base = load_snapshot(base)?;
        let a = snapshot.onboard.as_ref().context("missing onboard state")?;
        let b = base
            .onboard
            .as_ref()
            .context("missing baseline onboard state")?;
        ensure!(
            snapshot.device_id == base.device_id && snapshot.usb_revision == base.usb_revision,
            "baseline revision mismatch"
        );
        ensure!(
            if matches!(args.command, Command::Paint { .. }) {
                a.rgb == b.rgb && picture(&snapshot.lighting) == picture(&base.lighting)
            } else if matches!(args.command, Command::Magnetic { .. }) {
                snapshot.advanced == base.advanced
            } else {
                a.options == b.options
                    && snapshot.advanced.as_ref().and_then(|s| s.options)
                        == base.advanced.as_ref().and_then(|s| s.options)
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
    let image = Image::load_for(
        file,
        args.model
            .as_deref()
            .map(monsgeek_linux::models::select)
            .transpose()?
            .unwrap_or_else(monsgeek_linux::models::original),
    )?;
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
        "Bootloader confirmed completion; target keyboard reidentified. Firmware {:04x} -> {:04x}. Configuration was not restored automatically.",
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
