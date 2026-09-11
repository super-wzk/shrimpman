use mhf_resource::motion::{KeyEncoding, Keyframe, Motion, MotionArchive, ObservedMotionDirectory};

fn block(kind: u32, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [kind, count, (12 + payload.len()) as u32] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(payload);
    bytes
}

fn motion(channels: &[Vec<u8>]) -> Vec<u8> {
    let empty_track = block(0x8000_0000, 0, &[]);
    let mut track_payload = channels.concat();
    track_payload.extend([0xe1, 0xe2, 0xe3]);
    let populated_track = block(0xf000_01ff, channels.len() as u32, &track_payload);
    let mut payload = Vec::new();
    payload.extend(0x1234_5678_u32.to_le_bytes());
    payload.extend(0x8765_4321_u32.to_le_bytes());
    payload.extend(empty_track);
    payload.extend(populated_track);
    payload.extend([0xa1, 0xa2]);
    block(0xc000_0002, 2, &payload)
}

fn examples() -> [(u32, Keyframe); 6] {
    [
        (
            0x8011_0001,
            Keyframe::I16Pair {
                value: -17,
                frame: 19,
            },
        ),
        (
            0xe012_0008,
            Keyframe::I16Quad {
                value: -7,
                frame: -3,
                parameters: [-231, 307],
            },
        ),
        (
            0x8013_0040,
            Keyframe::Mixed12 {
                unknown_00: 0xf123_4567,
                value: 31,
                frame: 127,
                parameters: [10, -100],
            },
        ),
        (
            0x8021_0002,
            Keyframe::F32Pair {
                value_bits: 0x7fc0_9876,
                frame_bits: 0x8000_0000,
            },
        ),
        (
            0x8022_0020,
            Keyframe::F32Quad {
                value_bits: 0xff80_0000,
                frame_bits: 17.5_f32.to_bits(),
                parameter_bits: [0x7fa0_1234, 0x8000_0000],
            },
        ),
        (
            0x8023_0100,
            Keyframe::F32Five {
                unknown_00: 0x8765_4321,
                value_bits: 0x7f80_0000,
                frame_bits: (-17.25_f32).to_bits(),
                parameter_bits: [0x7fc0_fedc, 0xffff_ffff],
            },
        ),
    ]
}

#[test]
fn standalone_probe_keeps_zero_channel_tracks_and_requires_complete_known_records() {
    let key = Keyframe::F32Pair {
        value_bits: 0x7fc0_1234,
        frame_bits: 0x8000_0000,
    };
    let channel = block(0x8021_0001, 1, &key.to_bytes());
    let mut payload = vec![0; 8];
    payload.extend(block(0x8000_0000, 0, &[]));
    payload.extend(block(0x8000_0001, 1, &channel));
    let bytes = block(0x8000_0002, 2, &payload);
    let parsed = Motion::probe(&bytes).unwrap();
    assert_eq!(parsed.as_bytes(), bytes);
    assert_eq!(parsed.tracks.len(), 2);
    assert!(parsed.tracks[0].channels.is_empty());
    assert_eq!(parsed.tracks[0].offset, 20);
    assert_eq!(parsed.tracks[1].offset, 32);
    assert_eq!(parsed.tracks[1].channels[0].offset, 44);
    assert_eq!(parsed.tracks[1].channels[0].key(0).unwrap(), key);
    for (offset, value) in [
        (0, 3_u32),
        (44, 0x8099_0001),
        (44, 0x8021_0003),
        (48, 0x10001),
    ] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Motion::parse(&invalid).is_ok());
        assert!(Motion::probe(&invalid).is_err());
    }
    let mut with_tail = bytes.clone();
    with_tail.extend([1, 2]);
    let size = with_tail.len() as u32;
    with_tail[8..12].copy_from_slice(&size.to_le_bytes());
    assert_eq!(Motion::parse(&with_tail).unwrap().trailing_bytes, [1, 2]);
    assert!(Motion::probe(&with_tail).is_err());
}

#[test]
fn all_six_native_key_layouts_preserve_bits_order_and_unknown_tails() {
    let examples = examples();
    let channels: Vec<_> = examples
        .iter()
        .map(|(kind, key)| {
            let mut payload = key.to_bytes();
            payload.extend([0xd1, 0xd2]);
            block(*kind, 1, &payload)
        })
        .collect();
    let bytes = motion(&channels);
    let parsed = Motion::parse(&bytes).unwrap();
    assert_eq!(parsed.header.kind, 0xc000_0002);
    assert_eq!(parsed.metadata_present, 0x1234_5678);
    assert_eq!(parsed.metadata, 0x8765_4321);
    assert_eq!(parsed.tracks.len(), 2);
    assert!(parsed.tracks[0].channels.is_empty());
    assert_eq!(parsed.tracks[1].trailing_bytes, [0xe1, 0xe2, 0xe3]);
    assert_eq!(parsed.trailing_bytes, [0xa1, 0xa2]);
    for (index, (kind, key)) in examples.into_iter().enumerate() {
        let channel = &parsed.tracks[1].channels[index];
        assert_eq!(channel.header.kind, kind);
        assert_eq!(channel.key(0).unwrap(), key);
        assert_eq!(channel.trailing_bytes(), [0xd1, 0xd2]);
        assert_eq!(parsed.with_key(1, index, 0, key).unwrap(), bytes);
    }
    assert_eq!(parsed.as_bytes(), bytes);
}

#[test]
fn key_frame_and_native_target_are_read_from_confirmed_offsets() {
    let channels: Vec<_> = examples()
        .iter()
        .map(|(kind, key)| block(*kind, 1, &key.to_bytes()))
        .collect();
    let bytes = motion(&channels);
    let parsed = Motion::parse(&bytes).unwrap();
    assert_eq!(
        parsed.tracks[1]
            .channels
            .iter()
            .map(|channel| channel.target_slot())
            .collect::<Vec<_>>(),
        [Some(0), Some(3), Some(6), Some(1), Some(5), Some(8)]
    );
    let frames: Vec<_> = parsed.tracks[1]
        .channels
        .iter()
        .map(|channel| channel.key(0).unwrap().frame())
        .collect();
    assert_eq!(frames, [19.0, -3.0, 127.0, -0.0, 17.5, -17.25]);
    assert_eq!(frames[3].to_bits(), 0x8000_0000);
}

#[test]
fn unknown_and_disabled_channels_remain_inspectable_without_a_guessed_decoder() {
    let unknown = block(0x8099_0180, u32::MAX, &[0x19, 0x20, 0x31]);
    let disabled = block(0x0012_0008, 7, &[0x13]);
    let bytes = motion(&[unknown.clone(), disabled.clone()]);
    let parsed = Motion::parse(&bytes).unwrap();
    let channels = &parsed.tracks[1].channels;
    assert_eq!(channels[0].encoding(), KeyEncoding::Unknown(0x99));
    assert_eq!(channels[0].target_slot(), None);
    assert_eq!(channels[0].as_bytes(), unknown);
    assert_eq!(channels[1].encoding(), KeyEncoding::Disabled);
    assert_eq!(channels[1].as_bytes(), disabled);
    assert!(channels[0].key(0).is_err());
    assert!(channels[1].key(0).is_err());
    assert_eq!(parsed.as_bytes(), bytes);
}

#[test]
fn patching_one_key_preserves_every_other_byte_in_an_archive_member() {
    let key = Keyframe::I16Quad {
        value: 123,
        frame: 20,
        parameters: [-17, 42],
    };
    let channel = block(
        0x8012_0008,
        2,
        &[key.to_bytes(), key.to_bytes(), vec![0xee]].concat(),
    );
    let original = motion(&[channel]);
    let source = [vec![0x87; 23], original.clone(), vec![0xb7; 19]].concat();
    let parsed = Motion::parse_at(&source, 23).unwrap();
    let updated_key = Keyframe::I16Quad {
        value: -212,
        frame: 20,
        parameters: [-17, 42],
    };
    let mut expected = original.clone();
    let key_offset = parsed.tracks[1].channels[0].offset - parsed.offset + 12 + 8;
    expected[key_offset..key_offset + 2].copy_from_slice(&(-212_i16).to_le_bytes());
    assert_eq!(parsed.with_key(1, 0, 1, updated_key).unwrap(), expected);
    assert_eq!(parsed.with_key(1, 0, 0, key).unwrap(), original);
    assert!(parsed.with_key(1, 0, 2, key).is_err());
    assert!(
        parsed
            .with_key(1, 0, 0, Keyframe::I16Pair { value: 1, frame: 2 })
            .is_err()
    );
}

#[test]
fn archive_preserves_holes_aliases_empty_groups_and_native_directory_addressing() {
    let clip = motion(&[]);
    let mut archive = Vec::new();
    // The low two table-offset bits are retained; native lookup drops them.
    for word in [3_u32, 19, 0, 28, u32::MAX, 28, 28] {
        archive.extend(word.to_le_bytes());
    }
    archive.extend(&clip);
    let parsed = MotionArchive::parse(&archive, 2).unwrap();
    assert_eq!(parsed.groups[0].offsets_offset, 19);
    assert_eq!(parsed.groups[0].motion_offsets, [None, Some(28), Some(28)]);
    assert!(parsed.groups[1].motion_offsets.is_empty());
    assert!(parsed.motion(0, 0).unwrap().is_none());
    assert_eq!(
        parsed.motion_by_native_id(1).unwrap().unwrap().as_bytes(),
        clip
    );
    assert_eq!(parsed.motion(0, 2).unwrap().unwrap().offset, 28);
    assert_eq!(parsed.as_bytes(), archive);
    assert!(parsed.motion(0, 3).is_err());
    assert!(parsed.motion_by_native_id(100).is_err());
    assert!(MotionArchive::parse(&archive, usize::MAX).is_err());
    archive[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(MotionArchive::parse(&archive, 2).is_err());
}

#[test]
fn archive_preserves_shared_and_partially_overlapping_offset_tables() {
    let clip = motion(&[]);
    let mut archive = Vec::new();
    // Groups 0 and 1 share a table despite different low bits. Group 2 uses
    // the last two entries of that same table.
    for word in [3_u32, 25, 3, 27, 2, 28, u32::MAX, 36, 36] {
        archive.extend(word.to_le_bytes());
    }
    archive.extend(&clip);
    let parsed = MotionArchive::parse(&archive, 3).unwrap();
    assert_eq!(parsed.groups[0].offsets_offset, 25);
    assert_eq!(parsed.groups[1].offsets_offset, 27);
    assert_eq!(parsed.groups[0].motion_offsets, [None, Some(36), Some(36)]);
    assert_eq!(
        parsed.groups[1].motion_offsets,
        parsed.groups[0].motion_offsets
    );
    assert_eq!(parsed.groups[2].motion_offsets, [Some(36), Some(36)]);
    assert!(parsed.motion(1, 0).unwrap().is_none());
    for (group, slot) in [(0, 1), (1, 2), (2, 0)] {
        assert_eq!(
            parsed.motion(group, slot).unwrap().unwrap().as_bytes(),
            clip
        );
    }
    assert_eq!(parsed.as_bytes(), archive);
}

#[test]
fn observed_directory_preserves_empty_records_holes_and_motion_aliases() {
    let clip = motion(&[]);
    let mut bytes = Vec::new();
    // Three observed records; the final empty record does not tell us whether
    // the native caller consumes two or three groups. Low table bits survive.
    for word in [2_u32, 25, 1, 32, 0, 36, u32::MAX, 36, 36] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(&clip);
    let observed = ObservedMotionDirectory::probe_with_budget(&bytes, 3).unwrap();
    assert_eq!(observed.record_count(), 3);
    assert_eq!(observed.directory.groups[0].offsets_offset, 25);
    assert_eq!(
        observed.directory.groups[0].motion_offsets,
        [None, Some(36)]
    );
    assert_eq!(observed.directory.groups[1].motion_offsets, [Some(36)]);
    assert!(observed.directory.groups[2].motion_offsets.is_empty());
    assert_eq!(observed.directory.as_bytes(), bytes);
    assert_eq!(
        observed.directory.motion(1, 0).unwrap().unwrap().as_bytes(),
        clip
    );
    // An explicit native context can consume a shorter prefix of the same file.
    assert_eq!(MotionArchive::parse(&bytes, 2).unwrap().groups.len(), 2);
    assert!(ObservedMotionDirectory::probe_with_budget(&bytes, 2).is_err());
}

#[test]
fn observed_directory_validates_every_distinct_motion_and_all_regions() {
    let clip = motion(&[]);
    let second = 32 + clip.len() as u32;
    let mut valid = Vec::new();
    for word in [2_u32, 16, 2, 24, 32, second, 32, second] {
        valid.extend(word.to_le_bytes());
    }
    valid.extend(&clip);
    valid.extend(&clip);
    assert!(ObservedMotionDirectory::probe_with_budget(&valid, 4).is_ok());
    assert!(ObservedMotionDirectory::probe_with_budget(&valid, 3).is_err());
    let mut shared_table = valid.clone();
    shared_table[12..16].copy_from_slice(&16_u32.to_le_bytes());
    let shared = ObservedMotionDirectory::probe_with_budget(&shared_table, 4).unwrap();
    assert_eq!(
        shared.directory.groups[0].motion_offsets,
        shared.directory.groups[1].motion_offsets
    );
    for (offset, value) in [
        (4, 12),                         // First offset alone is not a record count.
        (12, 8),                         // A later table points into the record region.
        (16, 24),                        // A motion points into an offset table.
        (second as usize, 3),            // An unrecognized second motion must fail too.
        (second as usize + 8, u32::MAX), // Second motion exceeds the file.
        (40, (2 * clip.len()) as u32),   // Individually valid motions overlap.
    ] {
        let mut invalid = valid.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(ObservedMotionDirectory::probe_with_budget(&invalid, 4).is_err());
    }
    let mut absent = valid;
    absent[16..32].fill(0xff);
    assert!(ObservedMotionDirectory::probe_with_budget(&absent, 4).is_ok());
    assert!(ObservedMotionDirectory::probe_with_budget(&[0; 24], 4).is_err());
    assert!(ObservedMotionDirectory::probe_with_budget(&[1; 7], 4).is_err());
}

#[test]
fn observed_directory_accepts_declared_empty_slots_but_not_only_empty_records() {
    // Matches the em171 member's layout: six complete 100-slot tables followed
    // by an empty record. The native consumed group count remains unspecified.
    let mut bytes = Vec::new();
    for group in 0..6_u32 {
        bytes.extend(100_u32.to_le_bytes());
        bytes.extend((56 + 400 * group).to_le_bytes());
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(2456_u32.to_le_bytes());
    bytes.resize(2456, 0xff);
    let observed = ObservedMotionDirectory::probe_with_budget(&bytes, 600).unwrap();
    assert_eq!(observed.record_count(), 7);
    assert_eq!(observed.directory.as_bytes(), bytes);
    assert_eq!(
        observed
            .directory
            .groups
            .iter()
            .map(|group| group.motion_offsets.len())
            .sum::<usize>(),
        600
    );
    assert!(
        observed
            .directory
            .groups
            .iter()
            .all(|group| group.motion_offsets.iter().all(Option::is_none))
    );
    assert!(ObservedMotionDirectory::probe_with_budget(&bytes, 599).is_err());
    assert!(ObservedMotionDirectory::probe_with_budget(&bytes[..2455], 600).is_err());
    let mut invalid = bytes;
    invalid[12..16].copy_from_slice(&48_u32.to_le_bytes());
    assert!(ObservedMotionDirectory::probe_with_budget(&invalid, 600).is_err());

    let only_records: Vec<_> = [0_u32, 16, 0, 16]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(MotionArchive::parse(&only_records, 2).is_ok());
    assert!(ObservedMotionDirectory::probe_with_budget(&only_records, 600).is_err());
}

#[test]
fn overlapping_tables_share_one_aggregate_slot_budget() {
    let group_count = 64;
    let slot_count = 256;
    let mut archive = Vec::new();
    for group in 0..group_count {
        archive.extend((slot_count as u32).to_le_bytes());
        archive.extend(((group_count * 8 + group * 4) as u32).to_le_bytes());
    }
    // Each table fits, but their overlapping windows would expand this small
    // source into group_count * slot_count allocated and traversed slots.
    archive.resize(archive.len() + (slot_count + group_count - 1) * 4, 0xff);
    let error = MotionArchive::parse(&archive, group_count).unwrap_err();
    assert_eq!(error.offset, 8);

    let total_slots = group_count * slot_count;
    let error =
        MotionArchive::parse_with_budget(&archive, group_count, total_slots - 1).unwrap_err();
    assert_eq!(error.offset, (group_count - 1) * 8);
    let parsed = MotionArchive::parse_with_budget(&archive, group_count, total_slots).unwrap();
    assert_eq!(parsed.groups.len(), group_count);
    for group in &parsed.groups {
        assert_eq!(group.motion_offsets.len(), slot_count);
        assert!(group.motion_offsets.iter().all(Option::is_none));
    }
    assert!(
        parsed
            .motion(group_count - 1, slot_count - 1)
            .unwrap()
            .is_none()
    );
    assert_eq!(parsed.as_bytes(), archive);
}

#[test]
fn malformed_parent_lengths_and_known_key_counts_fail_before_reading_neighbors() {
    let key = Keyframe::I16Quad {
        value: 1,
        frame: 2,
        parameters: [3, 4],
    };
    let bytes = motion(&[block(0x8012_0008, 1, &key.to_bytes())]);
    let parsed = Motion::parse(&bytes).unwrap();
    let track_offset = parsed.tracks[1].offset;
    let channel_offset = parsed.tracks[1].channels[0].offset;
    for length in 0..20 {
        assert!(Motion::parse(&bytes[..length]).is_err());
    }
    for (offset, value) in [
        (4, u32::MAX),
        (8, 19),
        (track_offset + 8, 11),
        (track_offset + 8, u32::MAX),
        (channel_offset + 4, 2),
        (channel_offset + 8, 11),
        (channel_offset + 8, u32::MAX),
    ] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Motion::parse(&invalid).is_err(),
            "offset {offset:#x}, value {value}"
        );
    }
    let mut archive = Vec::new();
    for word in [1_u32, 8, u32::MAX - 1] {
        archive.extend(word.to_le_bytes());
    }
    assert!(MotionArchive::parse(&archive, 1).is_err());
}
