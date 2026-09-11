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

#[test]
fn model_animation_fields_follow_native_offsets_and_signed_counts() {
    let mut bytes = [0xa5; 180];
    for axis in 0..3 {
        let at = 0x18 + axis * 12;
        bytes[at..at + 2].copy_from_slice(&(-180i16 + axis as i16).to_le_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&(90i16 + axis as i16).to_le_bytes());
        bytes[at + 4..at + 6].copy_from_slice(&(-1i16).to_le_bytes());
        bytes[at + 6..at + 8].copy_from_slice(&(400u16 + axis as u16).to_le_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&(0x7fc0_1234u32 + axis as u32).to_le_bytes());
        let at = 0x3c + axis * 20;
        bytes[at..at + 4].copy_from_slice(&(0x8000_0000u32 + axis as u32).to_le_bytes());
        bytes[at + 12..at + 14].copy_from_slice(&(-2i16).to_le_bytes());
        bytes[at + 14..at + 16].copy_from_slice(&(60u16 + axis as u16).to_le_bytes());
    }
    bytes[0x7c..0x82].copy_from_slice(&[10, 20, 30, 200, 210, 220]);
    bytes[0x84..0x86].copy_from_slice(&(-3i16).to_le_bytes());
    bytes[0x88..0x8a].copy_from_slice(&0x1234u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&(-100i16).to_le_bytes());
    bytes[0x96..0x98].copy_from_slice(&200i16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&50u16.to_le_bytes());
    bytes[0x9a..0x9c].copy_from_slice(&(-4i16).to_le_bytes());
    bytes[0x9c..0xa0].copy_from_slice(&3.0f32.to_bits().to_le_bytes());
    let parsed = ModelEffectDefinition::parse(&bytes).unwrap();
    for axis in 0..3 {
        let rotation = &parsed.rotation[axis];
        assert_eq!(rotation.start_degrees, -180 + axis as i16);
        assert_eq!(rotation.end_degrees, 90 + axis as i16);
        assert_eq!(rotation.repetitions, -1);
        assert_eq!(rotation.duration, 400 + axis as u16);
        assert_eq!(
            rotation.state_rate_multiplier_bits,
            0x7fc0_1234 + axis as u32
        );
        assert_eq!(parsed.scale[axis].start_bits, 0x8000_0000 + axis as u32);
        assert_eq!(parsed.scale[axis].repetitions, -2);
        assert_eq!(parsed.scale[axis].duration, 60 + axis as u16);
    }
    assert_eq!(parsed.color.start_rgb, [10, 20, 30]);
    assert_eq!(parsed.color.end_rgb, [200, 210, 220]);
    assert_eq!(parsed.color.repetitions, -3);
    assert_eq!(parsed.opacity.start, 0x1234);
    assert_eq!(
        (
            parsed.uv.u_period,
            parsed.uv.v_period,
            parsed.uv.cycle_steps
        ),
        (-100, 200, 50)
    );
    assert_eq!(parsed.uv.repetitions, -4);
    assert_eq!(parsed.uv.state_step_bits, 3.0f32.to_bits());
    assert_eq!(parsed.unknown_12, [0xa5; 2]);
    assert_eq!(parsed.to_bytes(), bytes);
}

#[test]
fn model_animation_edits_touch_only_the_named_field_bytes() {
    let source = pattern::<180>();
    let mut parsed = ModelEffectDefinition::parse(&source).unwrap();
    parsed.rotation[1].end_degrees = -90;
    parsed.scale[2].start_bits = 0x7fa0_9876;
    parsed.color.end_rgb = [1, 2, 3];
    parsed.uv.v_period = -400;
    parsed.visibility_flags = 0x0c;
    let mut expected = source;
    expected[0x26..0x28].copy_from_slice(&(-90i16).to_le_bytes());
    expected[0x64..0x68].copy_from_slice(&0x7fa0_9876u32.to_le_bytes());
    expected[0x7f..0x82].copy_from_slice(&[1, 2, 3]);
    expected[0x96..0x98].copy_from_slice(&(-400i16).to_le_bytes());
    expected[0xaa] = 0x0c;
    assert_eq!(parsed.to_bytes(), expected);
}

#[test]
fn attachment_animation_uses_its_own_layout_and_preserves_unclassified_bytes() {
    let mut source = pattern::<128>();
    source[0x10..0x12].copy_from_slice(&188u16.to_le_bytes());
    source[0x12..0x14].copy_from_slice(&30u16.to_le_bytes());
    source[0x14..0x16].copy_from_slice(&12i16.to_le_bytes());
    source[0x16..0x18].copy_from_slice(&15i16.to_le_bytes());
    source[0x18..0x1a].copy_from_slice(&(-1i16).to_le_bytes());
    source[0x1a..0x1c].copy_from_slice(&4i16.to_le_bytes());
    for axis in 0..3 {
        let at = 0x1c + axis * 8;
        source[at..at + 2].copy_from_slice(&(-90i16 + axis as i16).to_le_bytes());
        source[at + 2..at + 4].copy_from_slice(&(90i16 + axis as i16).to_le_bytes());
        source[at + 4..at + 6].copy_from_slice(&(-2i16).to_le_bytes());
        source[at + 6..at + 8].copy_from_slice(&(20u16 + axis as u16).to_le_bytes());
    }
    source[0x34..0x38].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    source[0x38..0x3c].copy_from_slice(&0x7fa0_1234u32.to_le_bytes());
    source[0x3e..0x40].copy_from_slice(&(-3i16).to_le_bytes());
    source[0x4a..0x50].copy_from_slice(&[1, 2, 3, 200, 210, 220]);
    source[0x56..0x58].copy_from_slice(&0x1234u16.to_le_bytes());
    source[0x62..0x64].copy_from_slice(&(-400i16).to_le_bytes());
    source[0x6c..0x70].copy_from_slice(&10.0f32.to_bits().to_le_bytes());
    source[0x70..0x74].copy_from_slice(&[2, 40, 50, 60]);
    let mut parsed = AttachmentDefinition::parse(&source).unwrap();
    assert_eq!(parsed.resource_id, 188);
    assert_eq!(parsed.start_delay, 30);
    assert_eq!(
        (
            parsed.sequence.start_id,
            parsed.sequence.end_id,
            parsed.sequence.repetitions,
            parsed.sequence.interval
        ),
        (12, 15, -1, 4)
    );
    for (axis, channel) in parsed.rotation.iter().enumerate() {
        assert_eq!(channel.start_degrees, -90 + axis as i16);
        assert_eq!(channel.end_degrees, 90 + axis as i16);
        assert_eq!(channel.repetitions, -2);
        assert_eq!(channel.duration, 20 + axis as u16);
    }
    assert_eq!(parsed.scale.start_bits, 0x8000_0000);
    assert_eq!(parsed.scale.end_bits, 0x7fa0_1234);
    assert_eq!(parsed.scale.repetitions, -3);
    assert_eq!(parsed.color.start_rgb, [1, 2, 3]);
    assert_eq!(parsed.color.end_rgb, [200, 210, 220]);
    assert_eq!(parsed.opacity.start, 0x1234);
    assert_eq!(parsed.uv.u_period, -400);
    assert_eq!(parsed.view_offset_bits, 10.0f32.to_bits());
    assert_eq!(parsed.trail_mode, 2);
    assert_eq!(parsed.trail_rgb, [40, 50, 60]);
    assert_eq!(parsed.to_bytes(), source);
    parsed.rotation[1].end_degrees = -45;
    parsed.trail_rgb = [11, 22, 33];
    let mut expected = source;
    expected[0x26..0x28].copy_from_slice(&(-45i16).to_le_bytes());
    expected[0x71..0x74].copy_from_slice(&[11, 22, 33]);
    assert_eq!(parsed.to_bytes(), expected);
}
