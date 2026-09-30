use super::*;
use crate::sdt;

fn fixture(base: u32) -> Vec<u8> {
    let mut bytes = vec![0; 0xa00];
    for (offset, value) in [
        (389 * 4, base + 0x700),
        (0x700 + 7 * 8, 2),
        (0x704 + 7 * 8, base + 0x800),
        (0x818, 2),
        (0x81c, base + 0x900),
        (0x828, 2),
        (0x82c, base + 0x940),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, words) in [
        (0x900, [3_u16, 47, 0, 4, 0, 8]),
        (0x90c, [4, 49, 2, 4, 12, 3]),
        (0x940, [0, 1, 0, 0, 17, 8]),
        (0x94c, [1, 0xfb07, 12, 3, 0xffee, 7]),
    ] {
        for (index, word) in words.into_iter().enumerate() {
            bytes[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn relocated_and_file_offsets_preserve_original_records_and_source_spans() {
    let bytes = fixture(0);
    let definition = Definition::parse(&bytes, 0, 7, 1).unwrap();
    assert_eq!(
        definition,
        Definition::parse(&fixture(0x2000_0000), 0x2000_0000, 7, 1).unwrap()
    );
    assert_eq!(definition.offset, 0x818);
    assert_eq!(weapon_actions(&bytes, 0, 7).unwrap().records, 0x800..0x830);
    assert_eq!(definition.steps[0].0, [3, 47, 0, 4, 0, 8]);
    for (index, step) in definition.steps.iter().enumerate() {
        assert_eq!(
            &step.to_bytes(),
            &bytes[definition.step_span(index).unwrap()]
        );
    }
    for (index, event) in definition.events.iter().enumerate() {
        assert_eq!(
            &event.to_bytes(),
            &bytes[definition.event_span(index).unwrap()]
        );
    }
    assert_eq!(definition.events[1].phase, -5);
    assert_eq!(definition.events[1].timing, 7);
    assert_eq!(definition.events[1].operation, 0xffee);
    assert!(definition.events[1].attack_reference(7).is_none());
    assert_eq!(
        definition.step_path("mhfdat.bin", 1).unwrap().to_string(),
        "mhfdat.bin#389/7/1/steps/1"
    );
    assert_eq!(
        definition.event_path("mhfdat.bin", 1).unwrap().to_string(),
        "mhfdat.bin#389/7/1/events/1"
    );
    assert!(definition.event_span(2).is_none());
    assert!(definition.step_path("mhfdat.bin", 2).is_none());
}

#[test]
fn invalid_extents_and_unavailable_actions_fail_without_reinterpreting_empty_tables() {
    let mut bytes = fixture(0x2000_0000);
    assert!(Definition::parse(&bytes[..0x94f], 0x2000_0000, 7, 1).is_err());
    assert!(Definition::parse(&bytes, 0x2000_0000, 7, 2).is_err());
    assert!(Definition::parse(&bytes, 0x2000_0000, 14, 0).is_err());
    bytes[0x82c..0x830].copy_from_slice(&0x1fff_ffff_u32.to_le_bytes());
    assert!(Definition::parse(&bytes, 0x2000_0000, 7, 1).is_err());
    bytes[0x828..0x82c].copy_from_slice(&0_u32.to_le_bytes());
    let definition = Definition::parse(&bytes, 0x2000_0000, 7, 1).unwrap();
    assert!(definition.events.is_empty());
    assert_eq!(definition.events_range, 0..0);
    assert!(definition.event_path("mhfdat.bin", 0).is_none());
    bytes[0x818..0x81c].copy_from_slice(&4097_u32.to_le_bytes());
    assert!(Definition::parse(&bytes, 0x2000_0000, 7, 1).is_err());
}

#[test]
fn native_motion_refs_keep_banks_styles_and_unresolved_selectors_separate() {
    let motion = NativeMotionRef {
        id: 1405,
        weapon: 7,
        style: Some(0),
    };
    assert_eq!(
        motion.resource_path().unwrap().to_string(),
        "motion/w07.mot#4/5"
    );
    let tonfa = NativeMotionRef {
        weapon: 11,
        style: Some(3),
        ..motion
    };
    assert_eq!(
        tonfa.resource_path().unwrap().to_string(),
        "motion/w11goku.mot#4/5"
    );
    assert!(
        NativeMotionRef {
            style: None,
            ..tonfa
        }
        .resource_path()
        .is_none()
    );
    assert_eq!(
        NativeMotionRef { id: 3405, ..motion }
            .resource_path()
            .unwrap()
            .to_string(),
        "motion/plface_m-pc.mot#4/5"
    );
    assert_eq!(
        NativeMotionRef { id: 4405, ..motion }
            .resource_path()
            .unwrap()
            .to_string(),
        "motion/plface_f-pc.mot#4/5"
    );
    for id in [405, 2405, 5405] {
        let unresolved = NativeMotionRef { id, ..motion };
        assert!(unresolved.resource_path().is_none());
        assert_eq!(unresolved.id, id);
        assert_eq!(unresolved.record(), 4);
        assert_eq!(unresolved.slot(), 5);
    }
}

fn sdt_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x100 + 10 * sdt::TableKind::Attack.stride()];
    for (index, subtype, category) in [(0, 0_u16, 999_u16), (1, 2, 100), (2, 1, 100), (3, 1, 100)] {
        let offset = index * sdt::DIRECTORY_STRIDE;
        bytes[offset..offset + 2].copy_from_slice(&subtype.to_le_bytes());
        bytes[offset + 2..offset + 4].copy_from_slice(&category.to_le_bytes());
        if category == 100 {
            bytes[offset + 4..offset + 6].copy_from_slice(&10_u16.to_le_bytes());
            bytes[offset + 8..offset + 12].copy_from_slice(&0x100_u32.to_le_bytes());
        }
    }
    let end = 4 * sdt::DIRECTORY_STRIDE;
    bytes[end + 2..end + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes
}

#[test]
fn attack_refs_select_native_sorted_keys_and_keep_original_directory_indices() {
    let bytes = sdt_fixture();
    let file = sdt::Sdt::parse(&bytes).unwrap();
    let directory = AttackDirectory::from_sdt("mhfsdt.bin", &file).unwrap();
    let event = ActionEvent {
        step: 0,
        timing: 1,
        phase: 0,
        frame: 0,
        count: 0,
        operation: 4,
        argument: 8,
    };
    let reference = event.attack_reference(11).unwrap();
    assert_eq!(
        reference,
        AttackReference {
            category: 100,
            subtype: None,
            record: 8
        }
    );
    assert_eq!(
        directory.resolve(reference).unwrap().unwrap().to_string(),
        "mhfsdt.bin#2/attacks/8"
    );
    assert_eq!(
        directory
            .resolve(AttackReference {
                subtype: Some(2),
                ..reference
            })
            .unwrap()
            .unwrap()
            .to_string(),
        "mhfsdt.bin#1/attacks/8"
    );
    assert_eq!(
        directory
            .resolve(AttackReference {
                subtype: Some(1),
                ..reference
            })
            .unwrap()
            .unwrap()
            .to_string(),
        "mhfsdt.bin#2/attacks/8",
        "equal keys retain the first physical entry after native stable sorting"
    );
    assert!(
        directory
            .resolve(AttackReference {
                category: 0,
                ..reference
            })
            .unwrap()
            .is_none()
    );
    assert!(
        directory
            .resolve(AttackReference {
                record: 10,
                ..reference
            })
            .is_err()
    );
    assert_eq!(event.attack_reference(12).unwrap().category, 106);
    assert!(
        event.attack_reference(7).is_none(),
        "weapon callback opcodes are not global"
    );
}

#[test]
fn owned_attack_directory_survives_source_drop_and_isolates_bad_tables() {
    let directory = {
        let mut bytes = sdt_fixture();
        let pointer = sdt::DIRECTORY_STRIDE + 8;
        bytes[pointer..pointer + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let file = sdt::Sdt::parse(&bytes).unwrap();
        AttackDirectory::from_sdt("mhfsdt.bin", &file).unwrap()
    };
    let reference = AttackReference {
        category: 100,
        subtype: None,
        record: 8,
    };
    assert_eq!(
        directory.resolve(reference).unwrap().unwrap().to_string(),
        "mhfsdt.bin#2/attacks/8"
    );
    assert!(
        directory
            .resolve(AttackReference {
                subtype: Some(2),
                ..reference
            })
            .is_err()
    );
    assert!(
        directory
            .resolve(AttackReference {
                record: 10,
                ..reference
            })
            .is_err()
    );
    assert!(
        directory
            .resolve(AttackReference {
                category: 999,
                ..reference
            })
            .unwrap()
            .is_none()
    );
    assert!(
        directory
            .resolve(AttackReference {
                category: 0,
                ..reference
            })
            .unwrap()
            .is_none()
    );
}
