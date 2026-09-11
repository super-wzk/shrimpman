use mhf_resource::effect::{
    AttachmentDefinition, AttachmentGroup, ModelEffectBinding, ModelEffectDefinition,
};

fn pattern<const N: usize>() -> [u8; N] {
    std::array::from_fn(|index| (index.wrapping_mul(73).wrapping_add(19)) as u8)
}

#[test]
fn all_four_records_round_trip_unknown_fields_and_float_bit_patterns() {
    let group = pattern::<18>();
    assert_eq!(AttachmentGroup::parse(&group).unwrap().to_bytes(), group);
    let binding = pattern::<24>();
    assert_eq!(
        ModelEffectBinding::parse(&binding).unwrap().to_bytes(),
        binding
    );
    let mut bits = pattern::<128>();
    for (index, value) in [0x7fc0_1234_u32, 0x8000_0000, 0xff80_0000]
        .into_iter()
        .enumerate()
    {
        bits[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let parsed = AttachmentDefinition::parse(&bits).unwrap();
    assert!(parsed.local_position()[0].is_nan());
    assert_eq!(parsed.local_position()[1].to_bits(), 0x8000_0000);
    assert_eq!(parsed.to_bytes(), bits);
    let mut bits = pattern::<180>();
    bits[..4].copy_from_slice(&0x7fa0_9876_u32.to_le_bytes());
    assert_eq!(
        ModelEffectDefinition::parse(&bits).unwrap().to_bytes(),
        bits
    );
}

#[test]
fn changing_a_binding_field_patches_only_its_actual_record_bytes() {
    let original = pattern::<128>();
    let mut parsed = AttachmentDefinition::parse(&original).unwrap();
    parsed.node_index = 17;
    let mut expected = original;
    expected[13] = 17;
    assert_eq!(parsed.to_bytes(), expected);

    let original = pattern::<180>();
    let mut parsed = ModelEffectDefinition::parse(&original).unwrap();
    parsed.node_index = 17;
    parsed.draw_group = 3;
    let mut expected = original;
    expected[15] = 17;
    expected[13] = 3;
    assert_eq!(parsed.to_bytes(), expected);

    let original = pattern::<24>();
    let mut parsed = ModelEffectBinding::parse(&original).unwrap();
    parsed.model_id = 0x1234;
    let mut expected = original;
    expected[6..8].copy_from_slice(&0x1234_u16.to_le_bytes());
    assert_eq!(parsed.to_bytes(), expected);
}

#[test]
fn zero_ends_loading_but_does_not_discard_later_file_slots() {
    let group = AttachmentGroup {
        part_code: 0xffff,
        definition_ids: [7, 8, 0, 9, 10, 11, 12, 13],
    };
    assert_eq!(group.active_definition_ids(), &[7, 8]);
    assert_eq!(AttachmentGroup::parse(&group.to_bytes()).unwrap(), group);
    let binding = ModelEffectBinding {
        part_code: 0xffff,
        weapon_class: 42,
        variant: 99,
        model_id: 3,
        definition_ids: group.definition_ids,
    };
    assert_eq!(binding.active_definition_ids(), &[7, 8]);
    assert_eq!(
        ModelEffectBinding::parse(&binding.to_bytes()).unwrap(),
        binding
    );
}

#[test]
fn record_boundaries_table_indices_and_partial_records_are_checked() {
    for length in [0, 17, 19] {
        assert!(AttachmentGroup::parse(&vec![0; length]).is_err());
    }
    assert!(AttachmentDefinition::parse(&[0; 127]).is_err());
    assert!(ModelEffectBinding::parse(&[0; 23]).is_err());
    assert!(ModelEffectDefinition::parse(&[0; 179]).is_err());
    let first = pattern::<18>();
    let second = [0xa5; 18];
    let bytes = [first, second].concat();
    let parsed = AttachmentGroup::parse_table(&bytes).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].to_bytes(), first);
    assert_eq!(parsed[1].to_bytes(), second);
    assert!(AttachmentGroup::parse_table(&bytes[..35]).is_err());
}
