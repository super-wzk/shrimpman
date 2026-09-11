use mhf_resource::effect_archive::{EffectArchive, EffectBank, EffectResource};

fn simple_archive(members: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend((members.len() as u32).to_le_bytes());
    let mut offset = 4 + members.len() * 8;
    for member in members {
        bytes.extend((offset as u32).to_le_bytes());
        bytes.extend((member.len() as u32).to_le_bytes());
        offset += member.len();
    }
    for member in members {
        bytes.extend(member);
    }
    bytes
}

fn bank() -> Vec<u8> {
    let mut bytes = Vec::new();
    // Six fixed tables, one lookup table, one event, and uninterpreted table 9.
    for word in [
        4_u16, 1, 1, 1, 1, 1, 1, 3, 1, 2, 0x1234, 0x5678, 0xabcd, 0xef01,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    for (table, stride) in [112, 24, 16, 16, 56, 140].into_iter().enumerate() {
        bytes.extend(
            (0..stride).map(|index| (index as u8).wrapping_mul(29).wrapping_add(table as u8)),
        );
    }
    bytes.extend((-1_i16).to_le_bytes());
    bytes.extend(2_i16.to_le_bytes());
    for index in [u32::MAX, 0, u32::MAX] {
        bytes.extend(index.to_le_bytes());
    }
    let mut event = [0; 32];
    event[..4].copy_from_slice(&0x7fc0_1234_u32.to_le_bytes());
    event[12..14].copy_from_slice(&0_i16.to_le_bytes());
    event[24..].fill(0xab);
    bytes.extend(event);
    bytes.extend([0xde, 0xad, 0xbe, 0xef, 1, 2, 3]);
    bytes
}

#[test]
fn bank_tables_follow_native_order_and_records_reencode_exactly() {
    let bytes = bank();
    let parsed = EffectBank::parse(&bytes).unwrap();
    assert_eq!(parsed.version, 4);
    assert_eq!(parsed.counts, [1, 1, 1, 1, 1, 1, 3, 1, 2]);
    assert_eq!(
        parsed.unknown_14,
        [0x34, 0x12, 0x78, 0x56, 0xcd, 0xab, 1, 0xef]
    );
    assert_eq!(
        parsed.table_offsets,
        [28, 140, 164, 180, 196, 252, 392, 408, 440]
    );
    for (index, record) in [
        parsed.emitters[0].to_bytes().as_slice(),
        parsed.vector_keys[0].to_bytes().as_slice(),
        parsed.color_keys[0].to_bytes().as_slice(),
        parsed.integer_keys[0].to_bytes().as_slice(),
        parsed.definitions_56[0].to_bytes().as_slice(),
        parsed.definitions_140[0].to_bytes().as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = parsed.table_offsets[index];
        assert_eq!(record, &bytes[offset..offset + record.len()]);
    }
    let lookup = parsed.motion_lookup.as_ref().unwrap();
    assert_eq!((lookup.start, lookup.end), (-1, 2));
    assert_eq!(lookup.event_indices, [None, Some(0), None]);
    assert_eq!(parsed.motion_events[0].offset, 408);
    assert_eq!(parsed.motion_events[0].position_bits[0], 0x7fc0_1234);
    assert_eq!(parsed.motion_events[0].to_bytes(), bytes[408..440]);
    assert_eq!(parsed.trailing_bytes, [0xde, 0xad, 0xbe, 0xef, 1, 2, 3]);
    assert_eq!(parsed.as_bytes(), bytes);
}

#[test]
fn bank_checks_all_declared_extents_but_keeps_uninterpreted_tails() {
    let bytes = bank();
    for length in 0..440 {
        assert!(
            EffectBank::parse(&bytes[..length]).is_err(),
            "length {length}"
        );
    }
    let mut invalid = bytes.clone();
    invalid[..2].copy_from_slice(&3_u16.to_le_bytes());
    assert!(EffectBank::parse(&invalid).is_err());
    for header_offset in (2..=16).step_by(2) {
        let mut invalid = bytes.clone();
        invalid[header_offset..header_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(
            EffectBank::parse(&invalid).is_err(),
            "count at {header_offset}"
        );
    }
    let mut empty = [0; 28];
    empty[..2].copy_from_slice(&4_u16.to_le_bytes());
    let parsed = EffectBank::parse(&empty).unwrap();
    assert!(parsed.emitters.is_empty());
    assert!(parsed.motion_lookup.is_none());
    assert!(parsed.trailing_bytes.is_empty());
}

#[test]
fn archive_preserves_descriptors_aliases_unknown_members_and_raw_offsets() {
    let mut index = Vec::new();
    for word in [0x1234_u16, 3, 1, 152, 2, 103, 99, 777] {
        index.extend(word.to_le_bytes());
    }
    index.extend([0x81, 0x82]);
    let mut bytes = simple_archive(&[index, bank(), vec![0; 8], vec![7, 8, 9]]);
    let mut parsed = EffectArchive::parse(&bytes, 4).unwrap();
    assert_eq!(parsed.index.unknown_00, 0x1234);
    assert_eq!(parsed.index.count, 3);
    assert_eq!(parsed.index.trailing_bytes, [0x81, 0x82]);
    assert_eq!(parsed.members[0].reference.resource_id, 152);
    assert!(matches!(
        parsed.members[0].resource().unwrap(),
        EffectResource::Bank(_)
    ));
    let EffectResource::MotionEvents(events) = parsed.members[1].resource().unwrap() else {
        panic!()
    };
    assert!(events.lookup.is_none());
    assert!(events.events.is_empty());
    assert!(matches!(
        parsed.members[2].resource().unwrap(),
        EffectResource::Unknown([7, 8, 9])
    ));
    assert_eq!(parsed.as_bytes(), bytes);

    // Different descriptor IDs can alias one data member. Parsing the archive
    // retains that relationship without allocating its decoded bank twice.
    let index_offset = parsed.directory.entries[0].offset as usize;
    bytes.copy_within(12..20, 28);
    bytes[index_offset + 12..index_offset + 14].copy_from_slice(&1_u16.to_le_bytes());
    parsed = EffectArchive::parse(&bytes, 4).unwrap();
    assert_eq!(parsed.members[0].offset, parsed.members[2].offset);
    assert_eq!(parsed.members[0].as_bytes(), parsed.members[2].as_bytes());
    assert_eq!(parsed.members[2].reference.resource_id, 777);
    assert!(matches!(
        parsed.members[2].resource().unwrap(),
        EffectResource::Bank(_)
    ));
}

#[test]
fn malformed_descriptors_and_payloads_report_their_own_boundary() {
    let mut index = Vec::new();
    for word in [1_u16, 1, 1, 152] {
        index.extend(word.to_le_bytes());
    }
    let bytes = simple_archive(&[index, bank()]);
    assert!(EffectArchive::parse(&bytes, 1).is_err());
    let mut wrong_count = bytes.clone();
    wrong_count[22..24].copy_from_slice(&2_u16.to_le_bytes());
    assert!(EffectArchive::parse(&wrong_count, 2).is_err());
    let mut short_index = bytes.clone();
    short_index[8..12].copy_from_slice(&4_u32.to_le_bytes());
    assert!(EffectArchive::parse(&short_index, 2).is_err());
    let mut short_bank = bytes;
    short_bank[16..20].copy_from_slice(&27_u32.to_le_bytes());
    let parsed = EffectArchive::parse(&short_bank, 2).unwrap();
    assert!(parsed.members[0].resource().is_err());
    assert!(EffectArchive::parse(&0_u32.to_le_bytes(), 2).is_err());
}
