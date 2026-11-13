use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_monsgeek-linux"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn change_previews_work_without_opening_a_device() {
    // An impossible device path must not affect an offline preview.
    let out = run(&[
        "--device",
        "/no/such/hid",
        "remap",
        "--from",
        "capslock",
        "--to",
        "leftctrl",
        "--layer",
        "3",
        "--fn-layer",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["change"]["slot"], 3);
    assert_eq!(
        value["change"]["binding"],
        serde_json::json!([0, 0, 224, 0])
    );
    assert!(
        value["packet_hex"]
            .as_str()
            .unwrap()
            .starts_with("15 03 03 00 00 00 00 e4 00 00 e0 00")
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("No HID device was opened"));

    let light = run(&[
        "light",
        "--mode",
        "4",
        "--speed",
        "4",
        "--brightness",
        "3",
        "--option",
        "2",
        "--rainbow",
        "--color",
        "#123456",
    ]);
    assert!(
        light.status.success(),
        "{}",
        String::from_utf8_lossy(&light.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&light.stdout).unwrap();
    assert!(
        value["packet_hex"]
            .as_str()
            .unwrap()
            .starts_with("07 04 01 03 27 12 34 56 2d")
    );
}

#[test]
fn unsafe_or_invalid_invocations_fail_before_hardware_access() {
    for args in [
        vec!["profile", "4"],
        vec!["light", "--mode", "4", "--color", "notrgb"],
        vec!["light", "--mode", "4", "--brightness", "5"],
        vec!["remap", "--from", "0", "--to", "a"],
        vec!["remap", "--from", "255", "--to", "a"],
        vec!["--apply", "profile", "1"],
        vec!["--apply", "--before", "/no/such/file", "profile", "1"],
        vec!["--allow-experimental-writes", "profile", "1"],
        vec!["debounce", "0"],
        vec![
            "sleep",
            "--bt",
            "59",
            "--receiver",
            "120",
            "--deep-bt",
            "900",
            "--deep-receiver",
            "600",
        ],
        vec!["macro", "50"],
        vec![
            "bind",
            "--from",
            "knob-press",
            "--binding",
            r#"{"kind":"macro","slot":50,"mode":0}"#,
        ],
        vec![
            "bind",
            "--from",
            "slot:128",
            "--binding",
            r#"{"kind":"disabled"}"#,
        ],
        vec!["light", "--mode", "21"],
    ] {
        let out = run(&args);
        assert!(!out.status.success(), "unexpectedly accepted {args:?}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("no original M1W"),
            "validation was too late for {args:?}"
        );
    }
}

#[test]
fn rich_bindings_and_multipart_previews_do_not_open_hid() {
    let out = run(&[
        "--device",
        "/no/such/hid",
        "bind",
        "--from",
        "knob-press",
        "--binding",
        r#"{"kind":"keyboard","keys":[6]}"#,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["change"]["slot"], 6);
    assert_eq!(v["change"]["binding"], serde_json::json!([0, 0, 6, 0]));
    let out = run(&["--device", "/no/such/hid", "macro", "49", "--clear"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["packets_hex"][0].as_array().unwrap().len(), 5);
    assert!(
        v["packets_hex"][0][4]
            .as_str()
            .unwrap()
            .starts_with("16 31 04 20 01")
    );
}

#[test]
fn bluetooth_can_be_selected_without_changing_offline_previews() {
    let out = run(&[
        "--transport",
        "bluetooth",
        "--device",
        "/no/such/hid",
        "profile",
        "1",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["change"]["layer"], 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("No HID device was opened"));
    let help = run(&["--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("Bluetooth is experimental"));
    assert!(help.contains("--transport"));
    assert!(!run(&["--transport", "invalid", "inspect"]).status.success());
}

#[test]
fn firmware_preview_and_flash_gates_stay_offline() {
    let dir = std::env::temp_dir().join(format!(
        "monsgeek-firmware-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("firmwareFile.bin");
    let before = dir.join("before.json");
    let mut bytes = vec![0; 65536];
    bytes.extend([1; 65]);
    std::fs::write(&file, &bytes).unwrap();
    let file = file.to_str().unwrap();
    let out = run(&["--device", "/no/such/hid", "firmware", file]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["firmware"]["experimental"], true);
    assert_eq!(value["firmware"]["image_model_verified"], false);
    assert_eq!(value["firmware"]["blocks"], 2);
    assert_eq!(value["firmware"]["payload_bytes"], 65);
    assert!(String::from_utf8_lossy(&out.stderr).contains("can brick"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No HID device was opened"));
    let hash = value["firmware"]["sha256"].as_str().unwrap();
    for (args, message) in [
        (
            vec!["firmware", file, "--sha256", &"0".repeat(64)],
            "SHA-256 mismatch",
        ),
        (
            vec!["--transport", "bluetooth", "firmware", file],
            "require --transport usb",
        ),
        (
            vec![
                "--apply",
                "--allow-experimental-writes",
                "--before",
                before.to_str().unwrap(),
                "firmware",
                file,
                "--sha256",
                hash,
            ],
            "requires --allow-experimental-firmware",
        ),
        (
            vec![
                "--apply",
                "--allow-experimental-writes",
                "--before",
                before.to_str().unwrap(),
                "firmware",
                file,
                "--allow-experimental-firmware",
            ],
            "requires --sha256",
        ),
    ] {
        let out = run(&args);
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!before.exists());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn modern_models_preview_native_packets_and_infer_snapshot_identity() {
    use monsgeek_linux::models;
    let dir = std::env::temp_dir().join(format!("monsgeek-model-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model = models::get(2247).unwrap();
    let snapshot = model.example_snapshot();
    let path = dir.join("snapshot.json");
    std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let out = run(&[
        "--device",
        "/no/such/hid",
        "option",
        "--snapshot",
        path.to_str().unwrap(),
        "--name",
        "wasd-swap",
        "--enabled",
        "true",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["device_id"], 2247);
    assert_eq!(
        v["change"]["change"]["bytes"],
        serde_json::json!([0, 0, 0, 1])
    );
    assert!(
        v["packet_hex"]
            .as_str()
            .unwrap()
            .starts_with("09 00 00 00 00 01")
    );
    let out = run(&[
        "magnetic",
        "--snapshot",
        path.to_str().unwrap(),
        "--from",
        "a",
        "--setting",
        "actuation",
        "--value",
        "1.5",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["device_id"], 2247);
    assert_eq!(v["changes"].as_array().unwrap().len(), 1);
    for name in ["win-lock", "led-off", "keyboard-lock", "power-save"] {
        let out = run(&[
            "option",
            "--snapshot",
            path.to_str().unwrap(),
            "--name",
            name,
            "--enabled",
            "true",
        ]);
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("not a named option"));
    }
    assert!(
        !run(&[
            "--model",
            "983",
            "magnetic",
            "--snapshot",
            path.to_str().unwrap(),
            "--from",
            "a",
            "--setting",
            "actuation",
            "--value",
            "1.5"
        ])
        .status
        .success()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
