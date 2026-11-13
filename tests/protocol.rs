use monsgeek_linux::protocol::{Change, Lighting, decode_feature, identify, key_slot, packet};

#[test]
fn original_m1w_lighting_wire_contract() {
    // 07 04 01 03 27 12 34 56 sums to D2; checksum is 2D, at byte 8.
    let light = Lighting::new(4, 4, 3, 2, true, [0x12, 0x34, 0x56]).unwrap();
    assert_eq!(
        &light.packet()[..9],
        &[7, 4, 1, 3, 0x27, 0x12, 0x34, 0x56, 0x2d]
    );
    let white = Lighting::new(1, 0, 4, 0, false, [255; 3]).unwrap();
    assert_eq!(&white.packet()[..8], &[7, 1, 5, 4, 8, 250, 255, 250]);
    let picture = Lighting::new(13, 2, 4, 2, false, [1, 2, 3]).unwrap();
    assert_eq!(&picture.packet()[..8], &[7, 13, 3, 4, 32, 0, 200, 200]);
    assert!(Lighting::new(13, 2, 4, 3, false, [0; 3]).is_err());
    assert!(Lighting::new(4, 5, 4, 0, false, [0; 3]).is_err());
    assert!(Lighting::new(4, 2, 5, 0, false, [0; 3]).is_err());
    assert!(Lighting::new(7, 2, 4, 2, false, [0; 3]).is_err());
}

#[test]
fn key_remap_uses_matrix_slot_not_hid_usage_and_keeps_payload_outside_checksum() {
    assert_eq!(key_slot(57).unwrap(), 3); // Caps Lock
    assert_eq!(key_slot(4).unwrap(), 9); // A
    assert_eq!(key_slot(224).unwrap(), 5); // Left Control
    assert!(key_slot(0).is_err());
    assert!(key_slot(255).is_err());
    for (fn_layer, opcode, checksum) in [(false, 0x13, 0xe6), (true, 0x15, 0xe4)] {
        let change = Change::Remap {
            layer: 3,
            fn_layer,
            slot: 3,
            binding: [0, 0, 224, 0],
        };
        assert_eq!(
            &change.packet().unwrap()[..12],
            &[opcode, 3, 3, 0, 0, 0, 0, checksum, 0, 0, 224, 0]
        );
    }
    assert!(Change::Profile { layer: 4 }.packet().is_err());
    assert!(
        Change::Remap {
            layer: 0,
            fn_layer: false,
            slot: 128,
            binding: [0; 4]
        }
        .packet()
        .is_err()
    );
}

#[test]
fn hid_feature_framing_preserves_first_and_last_page_bytes() {
    let mut page = [0; 64];
    page[0] = 0x87;
    page[63] = 0xab;
    assert_eq!(decode_feature(&page).unwrap(), page);
    let mut wire = vec![0];
    wire.extend_from_slice(&page);
    assert_eq!(decode_feature(&wire).unwrap(), page);
    wire[0] = 1;
    assert!(decode_feature(&wire).is_err());
    assert!(decode_feature(&page[..63]).is_err());
    // Raw keymap pages may start with zero, not an opcode or report ID.
    page[0] = 0;
    assert_eq!(decode_feature(&page).unwrap()[63], 0xab);
}

#[test]
fn identity_gate_accepts_catalog_models_and_rejects_unknown_ids_and_echoes() {
    let mut reply = [0; 64];
    reply[..5].copy_from_slice(&[0x8f, 0xd7, 3, 0, 0]);
    assert_eq!(identify(&reply).unwrap(), 983);
    reply[1..5].copy_from_slice(&1604u32.to_le_bytes()); // M1W HE
    assert_eq!(identify(&reply).unwrap(), 1604);
    reply[1..5].copy_from_slice(&9999u32.to_le_bytes());
    assert!(identify(&reply).is_err());
    assert!(identify(&packet(&[0x8f], 7)).is_err());
    reply[0] = 0;
    assert!(identify(&reply).is_err());
}

#[test]
fn lighting_reads_keep_raw_flags_and_rgb_for_undo() {
    let mut reply = [0; 64];
    reply[..8].copy_from_slice(&[0x87, 4, 2, 3, 0x31, 11, 22, 33]);
    let light = Lighting::decode(&reply).unwrap();
    assert_eq!(&light.packet()[1..8], &reply[1..8]);
    reply[0] = 0;
    assert!(Lighting::decode(&reply).is_err());
}

#[test]
fn restore_preserves_unknown_bindings_and_last_two_slots() {
    use monsgeek_linux::device::Snapshot;
    let current = Snapshot {
        advanced: None,
        format_version: 1,
        device_id: 983,
        usb_revision: 264,
        active_layer: 0,
        lighting: Lighting::new(1, 2, 3, 0, false, [11, 22, 33]).unwrap(),
        keymaps: vec![vec![0; 512]; 4],
        fn_keymaps: vec![vec![0; 512]; 4],
        onboard: None,
    };
    let mut target = current.clone();
    target.keymaps[3][508..512].copy_from_slice(&[14, 5, 1, 3]);
    target.fn_keymaps[2][504..508].copy_from_slice(&[10, 23, 0, 9]);
    let changes = current.restore_changes(&target).unwrap();
    assert_eq!(
        changes,
        vec![
            Change::Remap {
                layer: 3,
                fn_layer: false,
                slot: 127,
                binding: [14, 5, 1, 3]
            },
            Change::Remap {
                layer: 2,
                fn_layer: true,
                slot: 126,
                binding: [10, 23, 0, 9]
            },
        ]
    );
    assert_eq!(
        current.inverse(&changes[0]).unwrap(),
        Change::Remap {
            layer: 3,
            fn_layer: false,
            slot: 127,
            binding: [0; 4]
        }
    );
    target.usb_revision = 265;
    assert!(current.restore_changes(&target).is_err());
    target.usb_revision = 264;
    target.fn_keymaps[1].pop();
    assert!(current.restore_changes(&target).is_err());
}
