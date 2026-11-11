use monsgeek_linux::{
    config::SleepTimers,
    device::{Onboard, Snapshot},
    host::{Action, Behavior, Chord, Config, Engine, Output, Rule},
    profiles::{Profile, Selection},
    protocol::Lighting,
};

fn snapshot() -> Snapshot {
    Snapshot {
        format_version: 2,
        device_id: 983,
        usb_revision: 0x108,
        active_layer: 0,
        lighting: Lighting::new(13, 2, 4, 0, false, [255; 3]).unwrap(),
        keymaps: vec![vec![0; 512]; 4],
        fn_keymaps: vec![vec![0; 512]; 4],
        onboard: Some(Onboard {
            debounce: 2,
            sleep: SleepTimers {
                time_bt: 120,
                time_24: 120,
                deep_time_bt: 900,
                deep_time_24: 900,
            },
            options: [0, 0, 0],
            auto_os: false,
            rgb: vec![0; 384],
            macros: vec![vec![0; 256]; 50],
        }),
    }
}
fn key(code: u16, value: i32) -> Output {
    Output::Key { code, value }
}
fn config(rules: Vec<Rule>) -> Config {
    Config {
        version: 1,
        devices: vec!["/dev/input/event0".into()],
        rules,
        chords: vec![],
        focus_command: None,
    }
}
fn tap_hold() -> Rule {
    Rule {
        key: 58,
        app: None,
        behavior: Behavior::TapHold {
            tap: Action::Keys { keys: vec![1] },
            hold: vec![29],
            timeout_ms: 200,
        },
    }
}

#[test]
fn selective_profile_keeps_unselected_settings_and_rejects_wrong_revision() {
    let base = snapshot();
    let mut target = base.clone();
    target.keymaps[1][8..12].copy_from_slice(&[0, 0, 4, 0]);
    target.onboard.as_mut().unwrap().debounce = 7;
    let profile = Profile::new("Coding".into(), target).unwrap();
    let selected = Selection {
        performance: false,
        ..Default::default()
    };
    let draft = profile.draft(&base, selected).unwrap();
    assert_eq!(draft.onboard, base.onboard);
    assert_ne!(draft.keymaps, base.keymaps);
    let mut incompatible = base.clone();
    incompatible.usb_revision = 0x109;
    assert!(profile.draft(&incompatible, selected).is_err());
}
#[test]
fn profile_does_not_mix_rgb_picture_identity() {
    let base = snapshot();
    let mut target = base.clone();
    target.lighting.flags = 0x10;
    let profile = Profile::new("Other picture".into(), target).unwrap();
    assert!(profile.draft(&base, Selection::default()).is_err());
    let only_keys = Selection {
        lighting: false,
        ..Default::default()
    };
    assert!(profile.draft(&base, only_keys).is_ok());
}
#[test]
fn review_names_key_and_rgb_values_instead_of_hiding_same_counts() {
    let base = snapshot();
    let mut draft = base.clone();
    draft.keymaps[0][12..16].copy_from_slice(&[0, 0, 224, 0]);
    draft.onboard.as_mut().unwrap().rgb[9..12].copy_from_slice(&[255, 20, 30]);
    let rows = monsgeek_linux::review::rows(&base, &draft).unwrap();
    assert!(
        rows.iter()
            .any(|r| r.setting.contains("Caps Lock") && r.after == "Left Ctrl")
    );
    assert!(
        rows.iter()
            .any(|r| r.before == "#000000" && r.after == "#FF141E")
    );
}
#[test]
fn picture_restore_is_staged_for_the_selected_picture_only() {
    let base = snapshot();
    let backup = monsgeek_linux::pictures::Backup {
        format_version: 1,
        snapshot: base.clone(),
        pictures: vec![vec![10; 384], vec![20; 384], vec![30; 384]],
    };
    assert!(backup.picture_draft(&base, 1).is_err());
    let draft = backup.picture_draft(&base, 0).unwrap();
    assert_eq!(draft.onboard.unwrap().rgb, vec![10; 384]);
    assert_eq!(base.onboard.unwrap().rgb, vec![0; 384]);
}
#[test]
fn diagnostics_omit_configuration_and_input() {
    let report =
        monsgeek_linux::diagnostics::report(Some(&snapshot()), None, 3, vec!["Read failed".into()]);
    let json = serde_json::to_string(&report).unwrap();
    for forbidden in ["keymaps", "macros", "rgb", "serial", "events"] {
        assert!(!json.contains(forbidden));
    }
    assert!(json.contains("0108"));
}
#[test]
fn tap_hold_tap_timeout_and_interrupted_hold_release_cleanly() {
    let mut engine = Engine::new(config(vec![tap_hold()])).unwrap();
    assert!(engine.event(58, 1, 0, "").is_empty());
    assert_eq!(engine.event(58, 0, 100, ""), vec![key(1, 1), key(1, 0)]);
    engine.event(58, 1, 200, "");
    assert_eq!(engine.tick(400), vec![key(29, 1)]);
    assert_eq!(engine.event(58, 0, 401, ""), vec![key(29, 0)]);
    engine.event(58, 1, 500, "");
    assert_eq!(engine.event(30, 1, 550, ""), vec![key(29, 1), key(30, 1)]);
    assert_eq!(engine.release_all(), vec![key(29, 0), key(30, 0)]);
    assert!(engine.event(58, 0, 600, "").is_empty());
}
#[test]
fn overlapping_output_modifier_is_not_released_early() {
    let mut engine = Engine::new(config(vec![tap_hold()])).unwrap();
    assert_eq!(engine.event(29, 1, 0, ""), vec![key(29, 1)]);
    engine.event(58, 1, 10, "");
    assert!(engine.tick(210).is_empty());
    assert!(engine.event(58, 0, 220, "").is_empty());
    assert_eq!(engine.event(29, 0, 230, ""), vec![key(29, 0)]);
}
#[test]
fn tap_dance_fires_double_action_once_and_keeps_typing_order() {
    let rule = Rule {
        key: 87,
        app: None,
        behavior: Behavior::TapDance {
            actions: vec![
                Action::Keys { keys: vec![113] },
                Action::Keys { keys: vec![164] },
            ],
            timeout_ms: 200,
        },
    };
    let mut engine = Engine::new(config(vec![rule])).unwrap();
    engine.event(87, 1, 0, "");
    engine.event(87, 0, 10, "");
    engine.event(87, 1, 50, "");
    engine.event(87, 0, 60, "");
    assert_eq!(engine.tick(260), vec![key(164, 1), key(164, 0)]);
    assert!(engine.tick(500).is_empty());
    engine.event(87, 1, 600, "");
    engine.event(87, 0, 610, "");
    assert_eq!(
        engine.event(30, 1, 620, ""),
        vec![key(113, 1), key(113, 0), key(30, 1)]
    );
}
#[test]
fn chords_suppress_sources_and_release_on_first_key_up() {
    let mut c = config(vec![]);
    c.chords = vec![Chord {
        keys: vec![36, 37],
        action: Action::Keys { keys: vec![1] },
        app: None,
        window_ms: 45,
    }];
    let mut engine = Engine::new(c).unwrap();
    assert!(engine.event(36, 1, 0, "").is_empty());
    assert_eq!(engine.event(37, 1, 20, ""), vec![key(1, 1)]);
    assert_eq!(engine.event(36, 0, 30, ""), vec![key(1, 0)]);
    assert!(engine.event(37, 0, 40, "").is_empty());
    engine.event(36, 1, 100, "");
    assert_eq!(engine.tick(145), vec![key(36, 1)]);
    assert_eq!(engine.event(36, 0, 150, ""), vec![key(36, 0)]);
}
#[test]
fn app_specific_mapping_is_pinned_until_release_and_launch_is_data_only() {
    let rule = Rule {
        key: 30,
        app: Some("editor".into()),
        behavior: Behavior::Remap {
            action: Action::Keys { keys: vec![29, 20] },
        },
    };
    let mut engine = Engine::new(config(vec![rule])).unwrap();
    assert_eq!(
        engine.event(30, 1, 0, "editor"),
        vec![key(29, 1), key(20, 1)]
    );
    assert_eq!(
        engine.event(30, 0, 10, "browser"),
        vec![key(20, 0), key(29, 0)]
    );
    assert_eq!(engine.event(30, 1, 20, ""), vec![key(30, 1)]);
    engine.event(30, 0, 30, "");
    let launch = Rule {
        key: 183,
        app: None,
        behavior: Behavior::Remap {
            action: Action::Launch {
                argv: vec!["example-app".into(), "literal $(text)".into()],
            },
        },
    };
    let mut engine = Engine::new(config(vec![launch])).unwrap();
    assert_eq!(
        engine.event(183, 1, 0, ""),
        vec![Output::Launch {
            argv: vec!["example-app".into(), "literal $(text)".into()]
        }]
    );
    assert!(engine.event(183, 2, 10, "").is_empty());
    assert!(engine.event(183, 0, 20, "").is_empty());
}
#[test]
fn ambiguous_host_rules_and_bad_device_paths_are_refused() {
    let mut c = config(vec![tap_hold(), tap_hold()]);
    assert!(c.validate().is_err());
    c.rules.pop();
    c.devices = vec!["/dev/input/../../etc/passwd".into()];
    assert!(c.validate().is_err());
    assert!(Config::example().validate().is_ok());
}

#[test]
fn an_unmatched_chord_key_does_not_reorder_ordinary_typing() {
    let mut c = config(vec![]);
    c.chords = vec![Chord {
        keys: vec![36, 37],
        action: Action::Keys { keys: vec![1] },
        app: None,
        window_ms: 45,
    }];
    let mut engine = Engine::new(c).unwrap();
    assert!(engine.event(36, 1, 0, "").is_empty());
    assert_eq!(engine.event(30, 1, 10, ""), vec![key(36, 1), key(30, 1)]);
}

#[test]
fn arbitrary_input_sequences_never_leave_output_keys_stuck() {
    let mut engine = Engine::new(Config::example()).unwrap();
    let codes = [58, 29, 30, 36, 37, 87, 88, 183, 42, 57];
    let mut held = std::collections::BTreeSet::new();
    let mut random = 17u64;
    for now in 0..5000u64 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let code = codes[(random >> 32) as usize % codes.len()];
        let value = ((random >> 48) % 3) as i32;
        let mut events = engine.event(
            code,
            value,
            now * 7,
            if random & 1 == 0 {
                "org.gnome.Nautilus"
            } else {
                ""
            },
        );
        if now % 300 == 299 {
            events.extend(engine.release_all());
        }
        for event in events {
            match event {
                Output::Key { code, value: 1 } => {
                    assert!(held.insert(code), "duplicate output down {code}")
                }
                Output::Key { code, value: 0 } => {
                    assert!(held.remove(&code), "output up without down {code}")
                }
                Output::Key { code, value: 2 } => assert!(held.contains(&code)),
                _ => {}
            }
        }
    }
    for event in engine.release_all() {
        if let Output::Key { code, value: 0 } = event {
            assert!(held.remove(&code));
        }
    }
    assert!(held.is_empty());
}

#[test]
fn new_cli_workflows_are_offline_and_profile_loading_is_selective() {
    use std::{fs, process::Command};
    let dir = std::env::temp_dir().join(format!(
        "m1w-features-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let base = snapshot();
    let mut target = base.clone();
    target.keymaps[0][12..16].copy_from_slice(&[0, 0, 224, 0]);
    target.onboard.as_mut().unwrap().debounce = 7;
    fs::write(dir.join("base.json"), serde_json::to_vec(&base).unwrap()).unwrap();
    fs::write(
        dir.join("target.json"),
        serde_json::to_vec(&target).unwrap(),
    )
    .unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_monsgeek-linux"))
            .arg("--device")
            .arg("/no/such/hid")
            .args(args)
            .current_dir(&dir)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    let saved = run(&["profiles", "save", "Coding", "target.json"]);
    let profile = String::from_utf8(saved.stdout).unwrap();
    let profile = profile.trim();
    run(&[
        "profiles",
        "draft",
        profile,
        "base.json",
        "draft.json",
        "--only",
        "keys",
    ]);
    let draft: Snapshot =
        serde_json::from_slice(&fs::read(dir.join("draft.json")).unwrap()).unwrap();
    assert_ne!(draft.keymaps, base.keymaps);
    assert_eq!(draft.onboard, base.onboard);
    let review = run(&["review", "base.json", "draft.json"]);
    assert!(String::from_utf8_lossy(&review.stdout).contains("Left Ctrl"));
    let diagnostics = run(&["diagnostics", "--snapshot", "base.json"]);
    let report: serde_json::Value = serde_json::from_slice(&diagnostics.stdout).unwrap();
    assert_eq!(report["firmware_revision"], "0108");
    assert!(report.get("keymaps").is_none());
    run(&["host", "example", "host.json"]);
    run(&["host", "check", "host.json"]);
    let preview = run(&["host", "run", "host.json"]);
    assert!(String::from_utf8_lossy(&preview.stdout).contains("No input was captured"));
    let picture = run(&["pictures", "pictures.json"]);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&picture.stdout).unwrap()["preview_only"],
        true
    );
    assert!(!dir.join("pictures.json").exists());
    fs::remove_dir_all(dir).unwrap();
}
