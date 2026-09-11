use super::*;

#[test]
fn transmog_changes_and_restores_only_the_selected_armor_slot() {
    let catalog = EquipmentCatalog {
        equipment: [0, 2, 3, 4, 5]
            .into_iter()
            .map(|kind| Equipment {
                kind,
                id: 17,
                model_ids: [20, 30],
                weapon: None,
                name: String::new(),
            })
            .collect(),
        ..Default::default()
    };
    let mut transmogs = Transmogs::default();
    for kind in [0, 2, 3, 4, 5] {
        transmogs = transmogs
            .changed(kind, Some(17), &catalog.equipment)
            .unwrap();
    }
    assert_eq!(transmogs.armor, [17, 0, 17, 17, 17, 17]);
    let restored = transmogs.changed(3, None, &catalog.equipment).unwrap();
    assert_eq!(restored.armor, [17, 0, 17, 0, 17, 17]);
    assert_eq!(transmogs.armor[3], 17);
}

#[test]
fn transmog_rejects_invalid_slots_zero_unknown_and_wrong_slot_ids() {
    let catalog = EquipmentCatalog {
        equipment: vec![
            Equipment {
                kind: 2,
                id: 10,
                model_ids: [20, 30],
                weapon: None,
                name: String::new(),
            },
            Equipment {
                kind: 6,
                id: 11,
                model_ids: [40; 2],
                weapon: Some(0),
                name: String::new(),
            },
        ],
        ..Default::default()
    };
    let transmogs = Transmogs::default();
    for (kind, id) in [
        (1, Some(10)),
        (6, Some(11)),
        (7, None),
        (u8::MAX, None),
        (2, Some(0)),
        (2, Some(11)),
        (3, Some(10)),
    ] {
        assert!(transmogs.changed(kind, id, &catalog.equipment).is_err());
    }
    assert_eq!(
        transmogs.changed(2, None, &catalog.equipment),
        Ok(transmogs)
    );
}

fn appearance_options() -> [AppearanceOptions; 2] {
    [
        AppearanceOptions {
            faces: (0..3)
                .map(|id| Face {
                    id,
                    model_id: u16::from(id) + 10,
                })
                .collect(),
            hair: vec![0, 27, 150],
        },
        AppearanceOptions {
            faces: (0..2)
                .map(|id| Face {
                    id,
                    model_id: u16::from(id) + 10,
                })
                .collect(),
            hair: vec![0, 27],
        },
    ]
}

#[test]
fn appearance_commands_preserve_other_fields_and_use_the_current_gender() {
    let options = appearance_options();
    let original = Appearance {
        female: true,
        ..Default::default()
    };
    let next = original
        .changed(AppearanceChange::Face(1), &options)
        .unwrap()
        .changed(AppearanceChange::Hair(27), &options)
        .unwrap();
    assert_eq!(
        next,
        Appearance {
            female: true,
            face: 1,
            hair: 27
        }
    );
    assert!(next.changed(AppearanceChange::Face(2), &options).is_err());
    assert!(next.changed(AppearanceChange::Hair(150), &options).is_err());
    assert!(next.changed(AppearanceChange::Hair(17), &options).is_err());
}

#[test]
fn gender_change_retains_supported_styles_and_replaces_missing_styles() {
    let options = appearance_options();
    for (face, hair, expected) in [(1, 27, (1, 27)), (2, 150, (0, 0))] {
        let original = Appearance {
            face,
            hair,
            ..Default::default()
        };
        let next = original
            .changed(AppearanceChange::Gender(true), &options)
            .unwrap();
        assert!(next.female);
        assert_eq!((next.face, next.hair), expected);
        assert_eq!(
            next.changed(AppearanceChange::Gender(false), &options)
                .unwrap(),
            Appearance {
                female: false,
                ..next
            }
        );
    }
}

#[test]
fn gender_change_requires_both_appearance_directories() {
    let mut options = appearance_options();
    options[1].hair.clear();
    assert!(
        Appearance::default()
            .changed(AppearanceChange::Gender(true), &options)
            .is_err()
    );
    options[1].hair.push(0);
    options[1].faces.clear();
    assert!(
        Appearance::default()
            .changed(AppearanceChange::Gender(true), &options)
            .is_err()
    );
}
