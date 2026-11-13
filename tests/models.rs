use monsgeek_linux::{
    advanced::{self, Change as Advanced},
    firmware::Image,
    models::{self, Family, MODELS},
    pictures, profiles,
    protocol::{Change, Lighting, identify, packet},
    transaction::{self, BeforeState},
};

#[test]
fn every_catalog_keyboard_has_mapped_geometry_and_round_trippable_defaults() {
    assert_eq!(MODELS.len(), 52);
    let mut missing = Vec::new();
    for model in MODELS.iter() {
        let snapshot = model.example_snapshot();
        snapshot
            .validate()
            .unwrap_or_else(|e| panic!("{} {}: {e:#}", model.id, model.name));
        assert!(snapshot.restore_changes(&snapshot).unwrap().is_empty());
        assert!(model.geometry.width > 0. && model.geometry.height > 0.);
        for (code, rect) in &model.geometry.layout {
            if rect.kind == "key" && model.slot_for_code(code).is_none() {
                missing.push(format!("{} {}: {code}", model.id, model.name));
            }
        }
        let mut wanted = snapshot.clone();
        // Last map and final slot must survive even on 20-map HE boards.
        let map = wanted.keymaps.last_mut().unwrap();
        map[508..512].copy_from_slice(&[0xa3, 0x55, 0xcc, 0x17]);
        let changes = snapshot.restore_changes(&wanted).unwrap();
        assert_eq!(changes.len(), 1, "{}", model.id);
        assert_eq!(wanted.inverse(&changes[0]).unwrap(), changes[0]);
        assert_eq!(
            snapshot.inverse(&changes[0]).unwrap(),
            Change::Remap {
                layer: model.base_maps() - 1,
                fn_layer: false,
                slot: 127,
                binding: model.defaults(false)[127],
            }
        );
        for change in changes {
            change.packets_for(model).unwrap();
        }
    }
    assert!(
        missing.is_empty(),
        "Missing physical keys:\n{}",
        missing.join("\n")
    );
}

#[test]
fn identity_uses_the_allowlist_and_model_selection_rejects_ambiguous_names() {
    for model in MODELS.iter() {
        let mut reply = [0; 64];
        reply[0] = 0x8f;
        reply[1..5].copy_from_slice(&model.id.to_le_bytes());
        assert_eq!(identify(&reply).unwrap(), model.id);
    }
    assert!(identify(&packet(&[0x8f], 7)).is_err());
    assert!(models::get(9999).is_err());
    assert!(models::select("FUN60 PRO").is_err());
    assert_eq!(models::select("M7W").unwrap().id, 1215);
    assert_eq!(models::select("4211").unwrap().id, 4211);
}

#[test]
fn families_have_distinct_wire_layouts_and_complete_advanced_output_maps() {
    let m1w = models::get(983).unwrap();
    let he = models::get(1604).unwrap();
    let tmr = models::get(2247).unwrap();
    assert_eq!(he.base_maps(), 20);
    assert_eq!(tmr.base_maps(), 16);
    assert_eq!(
        &Change::Profile { layer: 4 }.packets_for(he).unwrap()[0][..2],
        &[5, 16]
    );
    let change = Change::Remap {
        layer: 14,
        fn_layer: false,
        slot: 127,
        binding: [1, 2, 3, 4],
    };
    let p = change.packets_for(tmr).unwrap()[0];
    assert_eq!(&p[..7], &[10, 3, 127, 0, 0, 1, 2]);
    assert_eq!(&p[8..12], &[1, 2, 3, 4]);
    let p = Change::Remap {
        layer: 3,
        fn_layer: true,
        slot: 9,
        binding: [0, 0, 4, 0],
    }
    .packets_for(tmr)
    .unwrap()[0];
    assert_eq!(&p[..5], &[16, 1, 1, 9, 0]);
    assert!(change.packets_for(m1w).is_err());
    assert_eq!(m1w.source_slot("knob-press").unwrap(), 6);
    assert_eq!(tmr.source_slot("knob-press").unwrap(), 92);
    assert!(
        models::get(1215)
            .unwrap()
            .source_slot("knob-press")
            .is_err()
    );
    assert!(!models::get(4211).unwrap().is_remappable(92));
}

#[test]
fn lighting_respects_model_speed_directions_and_picture_counts() {
    let old = models::get(983).unwrap();
    let new = models::get(2247).unwrap();
    assert_eq!(old.picture_count(), 3);
    assert_eq!(new.picture_count(), 5);
    assert_eq!(
        Lighting::new_for(old, 4, 2, 4, 0, true, [1, 2, 3])
            .unwrap()
            .wire_speed,
        3
    );
    assert!(Lighting::new_for(new, 4, 2, 4, 0, true, [1, 2, 3]).is_err());
    assert_eq!(
        Lighting::new_for(new, 4, 2, 4, 2, true, [1, 2, 3])
            .unwrap()
            .wire_speed,
        2
    );
    assert!(Lighting::new_for(old, 13, 2, 4, 4, false, [0; 3]).is_err());
    assert!(Lighting::new_for(new, 13, 2, 4, 4, false, [0; 3]).is_ok());
    assert!(Lighting::new_for(old, 24, 2, 4, 0, false, [0; 3]).is_err());
    let backup = pictures::Backup {
        format_version: 1,
        snapshot: new.example_snapshot(),
        pictures: vec![vec![0; 384]; 5],
    };
    backup.validate().unwrap();
}

#[test]
fn profile_undo_and_picture_backups_reject_same_revision_other_model() {
    let a = models::get(983).unwrap().example_snapshot();
    let b = models::get(1933).unwrap().example_snapshot();
    assert_eq!(a.usb_revision, b.usb_revision);
    assert!(a.restore_changes(&b).is_err());
    assert!(
        profiles::Profile::new("Example".into(), a.clone())
            .unwrap()
            .draft(&b, profiles::Selection::default())
            .is_err()
    );
    assert!(
        transaction::check_undo(
            &BeforeState {
                snapshot: a.clone(),
                undo: None,
                undo_changes: Vec::new()
            },
            &b
        )
        .is_err()
    );
    let backup = pictures::Backup {
        format_version: 1,
        snapshot: a,
        pictures: vec![vec![0; 384]; 3],
    };
    assert!(backup.picture_draft(&b, 0).is_err());
}

#[test]
fn magnetic_edits_preserve_other_keys_and_encode_dks_planes_without_truncation() {
    for id in [1604, 2247] {
        let model = models::get(id).unwrap();
        let mut settings = model.example_snapshot().advanced.unwrap();
        let untouched = settings.magnetic[&0][..16].to_vec();
        settings.set_value(model, 0, 127, 2.3).unwrap();
        assert!((settings.value(model, 0, 127).unwrap() - 2.3).abs() < 0.001);
        assert_eq!(&settings.magnetic[&0][..16], &untouched);
        settings.pair(9, 10).unwrap();
        settings.pair(9, 11).unwrap();
        assert_eq!(settings.magnetic[&7][10] & 127, 0);
        assert_eq!(settings.magnetic[&9][9], 11);
        assert_eq!(settings.magnetic[&9][11], 9);
        let mut dks = vec![0; 512];
        for plane in 0..4 {
            dks[plane * 128 + 127] = 0x10 + plane as u8;
        }
        let packets = Advanced::Magnetic {
            parameter: 10,
            data: dks,
        }
        .packets(model)
        .unwrap();
        assert_eq!(packets.len(), 128);
        assert_eq!(&packets[127][..5], &[101, 8, 0, 127, 1]);
        assert_eq!(&packets[127][8..12], &[0x10, 0x11, 0x12, 0x13]);
        let bytes = advanced::magnetic_bytes(model, 0).unwrap();
        assert_eq!(
            bytes,
            if model.family == Family::Ry5088 {
                256
            } else {
                128
            }
        );
    }
}

#[test]
fn firmware_uses_the_model_prefix_and_bootloader_word_layout() {
    let normal = models::get(2247).unwrap();
    assert_eq!(normal.firmware_prefix, 20480);
    let image = Image::parse_for(&vec![1; normal.firmware_prefix + 65], normal).unwrap();
    assert_eq!(image.info().target_device_id, 2247);
    assert_eq!(image.info().blocks, 2);
    let wide = models::get(3245).unwrap();
    assert!(wide.firmware_wide);
    let image = Image::parse_for(&vec![1; wide.firmware_prefix + 65], wide).unwrap();
    assert_eq!(&image.prepare_packet()[8..12], &[2, 0, 0, 0]);
    assert_eq!(&image.finish_packet()[8..16], &[2, 0, 0, 0, 65, 0, 0, 0]);
}

#[test]
fn numbered_feature_reports_are_discovered_and_decoded_without_losing_payload() {
    use monsgeek_linux::transport::{decode_feature_report, parse_feature_report};
    let descriptor = [
        0x06, 0xff, 0xff, 0x09, 2, 0xa1, 1, 0x85, 5, 0x75, 8, 0x95, 64, 0xb1, 2, 0xc0,
    ];
    let report = parse_feature_report(&descriptor).unwrap();
    assert_eq!(report.id, 5);
    let mut wire = [0x77; 65];
    wire[0] = 5;
    assert_eq!(decode_feature_report(&wire, report).unwrap(), [0x77; 64]);
    wire[0] = 6;
    assert!(decode_feature_report(&wire, report).is_err());
    assert!(parse_feature_report(&descriptor[..13]).is_err());
}

#[test]
fn incomplete_modern_snapshots_and_unrepresentable_values_are_rejected() {
    let model = models::get(2247).unwrap();
    let snapshot = model.example_snapshot();
    for field in ["advanced", "options", "report_rate", "magnetic"] {
        let mut s = snapshot.clone();
        match field {
            "advanced" => s.advanced = None,
            "options" => s.advanced.as_mut().unwrap().options = None,
            "report_rate" => s.advanced.as_mut().unwrap().report_rate = None,
            "magnetic" => {
                s.advanced.as_mut().unwrap().magnetic.remove(&0);
            }
            _ => unreachable!(),
        }
        assert!(s.validate().is_err(), "accepted missing {field}");
    }
    let mut settings = snapshot.advanced.clone().unwrap();
    let slot = model.key_slot(4).unwrap();
    for value in [f32::NAN, f32::INFINITY, -1., 100., 1.234] {
        assert!(settings.set_value(model, 0, slot, value).is_err());
    }
    let mut draft = snapshot.clone();
    draft
        .advanced
        .as_mut()
        .unwrap()
        .set_value(model, 0, slot, 1.5)
        .unwrap();
    let rows = monsgeek_linux::review::rows(&snapshot, &draft).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].slot, Some(slot));
    assert_eq!(rows[0].scope.as_deref(), Some("Actuation"));
    assert!(rows[0].after.contains("1.5"));
    assert_eq!(advanced::Settings::default().units_per_mm, 100);
    assert_eq!(advanced::legacy_precision(0x0500, Some(0x0200)), 10);
    assert_eq!(advanced::legacy_precision(0x0500, Some(0)), 200);
    assert_eq!(advanced::legacy_precision(0x0200, Some(0x0300)), 100);
    let mut other_precision = snapshot.clone();
    other_precision.advanced.as_mut().unwrap().units_per_mm = 200;
    assert!(snapshot.restore_changes(&other_precision).is_err());
    assert!(
        profiles::Profile::new("Other precision".into(), other_precision)
            .unwrap()
            .draft(&snapshot, profiles::Selection::default())
            .is_err()
    );
    for model in MODELS.iter() {
        if let Some(matrix) = model.matrices.get("defaultFnMacMatrix") {
            for map in 0..model.fn_maps() {
                if model.fn_address(map).unwrap().0 == 1 {
                    for slot in 0..128u8 {
                        assert_eq!(
                            model.default_binding(true, map, slot),
                            matrix[usize::from(slot)]
                        );
                    }
                }
            }
        }
    }
}
