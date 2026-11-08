use monsgeek_linux::config::{
    Binding, Macro, MacroEvent, SleepTimers, macro_packets, rgb_packets, source_slot,
};

fn key(down: bool, delay_ms: u16) -> MacroEvent {
    MacroEvent::Keyboard {
        usage: 6,
        down,
        delay_ms,
    }
}

#[test]
fn binding_bytes_and_raw_preservation() {
    for (keys, expected) in [
        (vec![6], [0, 0, 6, 0]),
        (vec![224, 6], [0, 0, 224, 6]),
        (vec![224, 225, 41], [0, 224, 225, 41]),
        (vec![41, 225, 224], [0, 41, 225, 224]),
    ] {
        assert_eq!(Binding::Keyboard { keys }.encode().unwrap(), expected);
    }
    assert_eq!(
        Binding::Consumer { usage: 0x192 }.encode().unwrap(),
        [3, 0, 0x92, 1]
    );
    assert_eq!(
        Binding::System { usage: 130 }.encode().unwrap(),
        [2, 0, 130, 0]
    );
    for button in 0xf0..=0xf4 {
        assert_eq!(
            Binding::Mouse { button }.encode().unwrap(),
            [1, 0, button, 0]
        );
    }
    assert_eq!(
        Binding::Pointer {
            action: "move-up".into()
        }
        .encode()
        .unwrap(),
        [1, 0, 247, 251]
    );
    assert_eq!(
        Binding::Pointer {
            action: "scroll-down".into()
        }
        .encode()
        .unwrap(),
        [1, 0, 245, 255]
    );
    assert!(
        Binding::Pointer {
            action: "invented".into()
        }
        .encode()
        .is_err()
    );
    assert_eq!(
        Binding::Macro { slot: 49, mode: 2 }.encode().unwrap(),
        [9, 2, 49, 0]
    );
    assert_eq!(Binding::Disabled.encode().unwrap(), [0; 4]);
    assert_eq!(
        Binding::Function {
            value: [10, 14, 0, 0]
        }
        .encode()
        .unwrap(),
        [10, 14, 0, 0]
    );
    for raw in [[0, 0, 3, 0], [255; 4], [0, 0, 6, 0], [10, 1, 0, 0]] {
        assert_eq!(Binding::decode(raw).encode().unwrap(), raw);
    }
    for keys in [vec![], vec![1], vec![240], vec![4; 4]] {
        assert!(Binding::Keyboard { keys }.encode().is_err());
    }
    assert!(Binding::Mouse { button: 1 }.encode().is_err());
    assert!(Binding::Macro { slot: 50, mode: 0 }.encode().is_err());
    assert!(Binding::Macro { slot: 0, mode: 3 }.encode().is_err());
    let binding = Binding::Keyboard { keys: vec![224, 6] };
    let json = serde_json::to_value(&binding).unwrap();
    assert_eq!(json["kind"], "keyboard");
    assert_eq!(serde_json::from_value::<Binding>(json).unwrap(), binding);
}

#[test]
fn source_slots_match_official_default_matrix() {
    for (name, expected) in [
        ("knob-left", 94),
        ("knob-press", 6),
        ("knob-right", 95),
        ("fn", 65),
        ("slot:127", 127),
        ("A", 9),
        ("capslock", 3),
        ("224", 5),
        ("0xe0", 5),
    ] {
        assert_eq!(source_slot(name).unwrap(), expected);
    }
    for name in ["slot:128", "slot:-1", "slot:256", "0", "255", "not-a-key"] {
        assert!(source_slot(name).is_err());
    }
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../docs/m1w-reference.json")).unwrap();
    let slots = reference["matrices"]["defaultMatrix"]["slots"]
        .as_array()
        .unwrap();
    assert_eq!(slots[6], serde_json::json!([10, 14, 0, 0]));
    assert_eq!(slots[65], serde_json::json!([10, 1, 0, 0]));
    assert_eq!(slots[94], serde_json::json!([3, 0, 234, 0]));
    assert_eq!(slots[95], serde_json::json!([3, 0, 233, 0]));
}

#[test]
fn macro_delay_boundaries_have_independent_wire_expectations() {
    let value = Macro {
        repeat: 0x1234,
        events: vec![
            key(true, 127),
            key(false, 128),
            key(true, 0),
            key(false, 65535),
        ],
    };
    let data = value.encode().unwrap();
    assert_eq!(data.len(), 256);
    assert_eq!(
        &data[..16],
        &[
            0x34, 0x12, 6, 0xff, 6, 0, 128, 0, 6, 0x80, 0, 0, 6, 0, 255, 255
        ]
    );
    assert!(data[16..].iter().all(|b| *b == 0));
    assert_eq!(Macro::decode(&data).unwrap(), value);
    let value = Macro {
        repeat: 1,
        events: vec![key(true, 1), key(false, 1)],
    };
    assert_eq!(&value.encode().unwrap()[..6], &[1, 0, 6, 129, 6, 1]);
}

#[test]
fn macro_movement_is_signed_and_always_extended() {
    let value = Macro {
        repeat: 1,
        events: vec![
            MacroEvent::Move {
                dx: -128,
                dy: 127,
                delay_ms: 1,
            },
            MacroEvent::Move {
                dx: -1,
                dy: 0,
                delay_ms: 0,
            },
            MacroEvent::MouseButton {
                button: 0xf4,
                down: true,
                delay_ms: 127,
            },
            MacroEvent::MouseButton {
                button: 0xf4,
                down: false,
                delay_ms: 128,
            },
        ],
    };
    let data = value.encode().unwrap();
    assert_eq!(
        &data[..20],
        &[
            1, 0, 249, 0, 128, 127, 1, 0, 249, 0, 255, 0, 0, 0, 244, 255, 244, 0, 128, 0
        ]
    );
    assert_eq!(Macro::decode(&data).unwrap(), value);
    let mut vendor_short = [0; 256];
    vendor_short[..6].copy_from_slice(&[1, 0, 249, 10, 255, 2]);
    assert_eq!(
        Macro::decode(&vendor_short).unwrap().events,
        vec![MacroEvent::Move {
            dx: -1,
            dy: 2,
            delay_ms: 5
        }]
    );
    // Decode valid storage even when normalizing short movements would exceed capacity.
    for chunk in vendor_short[2..254].as_chunks_mut::<4>().0 {
        chunk.copy_from_slice(&[249, 10, 255, 2]);
    }
    assert_eq!(Macro::decode(&vendor_short).unwrap().events.len(), 63);
}

#[test]
fn macro_validation_truncation_and_exact_capacity() {
    for events in [
        vec![key(true, 1)],
        vec![key(false, 1)],
        vec![key(true, 1), key(true, 1), key(false, 1)],
        vec![MacroEvent::Keyboard {
            usage: 0,
            down: true,
            delay_ms: 1,
        }],
        vec![MacroEvent::MouseButton {
            button: 0xf5,
            down: true,
            delay_ms: 1,
        }],
    ] {
        assert!(Macro { repeat: 1, events }.encode().is_err());
    }
    assert!(
        Macro {
            repeat: 0,
            events: vec![]
        }
        .encode()
        .is_err()
    );
    assert!(Macro::decode(&[0; 256]).is_err());
    for size in [0, 1, 2, 255, 257] {
        assert!(Macro::decode(&vec![0; size]).is_err());
    }
    let mut bad = [0; 256];
    bad[0] = 1;
    bad[10] = 4; // zero event marker cannot hide later data
    assert!(Macro::decode(&bad).is_err());
    bad[2] = 0xf8;
    assert!(Macro::decode(&bad).is_err());
    // 62 short pairs = 248 bytes, plus one six-byte move exactly fills capacity.
    let mut events: Vec<_> = (0..62)
        .flat_map(|_| [key(true, 1), key(false, 1)])
        .collect();
    events.push(MacroEvent::Move {
        dx: 1,
        dy: -1,
        delay_ms: 0,
    });
    let mut value = Macro { repeat: 1, events };
    let full = value.encode().unwrap();
    assert_eq!(full.len(), 256);
    assert_eq!(Macro::decode(&full).unwrap(), value);
    value.events.push(MacroEvent::Move {
        dx: 0,
        dy: 0,
        delay_ms: 1,
    });
    assert!(value.encode().is_err());
    let mut truncated = full;
    truncated[250..].copy_from_slice(&[6, 129, 6, 1, 6, 128]);
    assert!(Macro::decode(&truncated).is_err());
}

#[test]
fn macro_packets_keep_zero_gaps_and_send_clear() {
    let empty = macro_packets(2, &[0; 256]).unwrap();
    assert_eq!(empty.len(), 5);
    assert_eq!(&empty[0][..8], &[0x16, 2, 0, 56, 0, 0, 0, 0xaf]);
    assert_eq!(&empty[4][..8], &[0x16, 2, 4, 32, 1, 0, 0, 0xc2]);
    for (page, p) in empty.iter().enumerate() {
        assert_eq!(p[2], page as u8);
        assert_eq!(p[4], u8::from(page == 4));
        assert!(p[8..].iter().all(|b| *b == 0));
    }
    let mut raw = [0; 256];
    raw[0] = 1;
    raw[200] = 7;
    raw[255] = 8;
    let pages = macro_packets(49, &raw).unwrap();
    assert!(pages[1][8..].iter().all(|b| *b == 0));
    assert_eq!(pages[3][8 + 32], 7);
    assert_eq!(pages[4][8 + 31], 8);
    assert!(pages[4][40..].iter().all(|b| *b == 0));
    assert!(macro_packets(50, &raw).is_err());
    assert!(macro_packets(0, &raw[..255]).is_err());
}

#[test]
fn rgb_seven_pages_and_sleep_asymmetric_offsets() {
    let raw: Vec<_> = (0..384).map(|n| n as u8).collect();
    let pages = rgb_packets(&raw).unwrap();
    assert_eq!(pages.len(), 7);
    assert_eq!(&pages[0][..8], &[12, 0, 128, 1, 0, 0, 0, 0x72]);
    for (page, p) in pages.iter().enumerate() {
        assert_eq!(p[4], page as u8);
        let count = if page == 6 { 48 } else { 56 };
        assert_eq!(&p[8..8 + count], &raw[page * 56..page * 56 + count]);
    }
    assert_eq!(&pages[6][56..], &[0; 8]);
    assert!(rgb_packets(&raw[..383]).is_err());
    assert!(rgb_packets(&[0; 385]).is_err());
    let timers = SleepTimers {
        time_bt: 120,
        time_24: 300,
        deep_time_bt: 900,
        deep_time_24: 600,
    };
    let set = timers.packet().unwrap();
    assert_eq!(&set[..8], &[0x12, 0, 0, 0, 0, 0, 0, 0xed]);
    assert_eq!(&set[8..16], &[120, 0, 44, 1, 132, 3, 88, 2]);
    let mut reply = [0; 64];
    reply[..9].copy_from_slice(&[0x92, 120, 0, 44, 1, 132, 3, 88, 2]);
    assert_eq!(SleepTimers::decode(&reply).unwrap(), timers);
    assert!(SleepTimers::decode(&set).is_err());
    for v in [59, 3601] {
        assert!(
            SleepTimers {
                time_bt: v,
                ..timers.clone()
            }
            .validate()
            .is_err()
        );
    }
    for v in [599, 3601] {
        assert!(
            SleepTimers {
                deep_time_24: v,
                ..timers.clone()
            }
            .packet()
            .is_err()
        );
    }
    assert!(
        SleepTimers {
            time_bt: 60,
            time_24: 3600,
            deep_time_bt: 600,
            deep_time_24: 3600
        }
        .validate()
        .is_ok()
    );
}
