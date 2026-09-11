use mhf_resource::effect_archive::MotionEvents;

fn event(motion_id: i16, frame: i16) -> [u8; 32] {
    let mut bytes = [
        0x45, 0x23, 0xa1, 0x7f, 0, 0, 0, 0x80, 0, 0, 0x80, 0xff, 0, 0, 0, 0, 0xfc, 0xff, 0xfb,
        0xff, 0x34, 0x12, 0xdc, 0xfe, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88,
    ];
    bytes[12..14].copy_from_slice(&motion_id.to_le_bytes());
    bytes[14..16].copy_from_slice(&frame.to_le_bytes());
    bytes
}

fn source(start: i16, end: i16, indices: &[u32], events: &[[u8; 32]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [0x9876, indices.len() as u16, events.len() as u16, 0xabcd] {
        bytes.extend(word.to_le_bytes());
    }
    if !indices.is_empty() {
        bytes.extend(start.to_le_bytes());
        bytes.extend(end.to_le_bytes());
        for index in indices {
            bytes.extend(index.to_le_bytes());
        }
    }
    for event in events {
        bytes.extend(event);
    }
    bytes
}

#[test]
fn signed_motion_ranges_holes_and_every_record_byte_are_preserved() {
    let records = [event(-2, -3), event(-2, 7), event(0, 4), event(1, 9)];
    let mut bytes = source(-2, 2, &[0, u32::MAX, 2, 3], &records);
    bytes.extend([0x99, 0x77, 0x55]);
    let parsed = MotionEvents::parse(&bytes).unwrap();
    assert_eq!(parsed.unknown_00, 0x9876);
    assert_eq!(parsed.lookup_count, 4);
    assert_eq!(parsed.event_count, 4);
    assert_eq!(parsed.unknown_06, 0xabcd);
    let lookup = parsed.lookup.as_ref().unwrap();
    assert_eq!(lookup.offset, 8);
    assert_eq!((lookup.start, lookup.end), (-2, 2));
    assert_eq!(lookup.event_indices, [Some(0), None, Some(2), Some(3)]);
    let first = parsed.events[0];
    assert_eq!(first.position_bits, [0x7fa1_2345, 0x8000_0000, 0xff80_0000]);
    assert_eq!(first.motion_id, -2);
    assert_eq!(first.frame, -3);
    assert_eq!(first.node_index, -4);
    assert_eq!(first.emitter_id, -5);
    assert_eq!(first.resource_id, 0x1234);
    assert_eq!(first.flags, 0xfedc);
    assert_eq!(
        first.unknown_18,
        [0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88]
    );
    for (i, record) in records.iter().enumerate() {
        assert_eq!(parsed.events[i].offset, 28 + 32 * i);
        assert_eq!(parsed.events[i].to_bytes(), *record);
    }
    assert_eq!(parsed.events_for_motion(-2).unwrap(), &parsed.events[..2]);
    assert_eq!(parsed.events_for_motion(0).unwrap(), &parsed.events[2..3]);
    assert_eq!(parsed.events_for_motion(1).unwrap(), &parsed.events[3..]);
    for id in [-3, -1, 2, i16::MIN, i16::MAX] {
        assert!(parsed.events_for_motion(id).unwrap().is_empty());
    }
    assert_eq!(parsed.trailing_bytes, [0x99, 0x77, 0x55]);
    assert_eq!(parsed.as_bytes(), bytes);

    let mut changed = first;
    changed.frame = 11;
    let mut expected = records[0];
    expected[14..16].copy_from_slice(&11_i16.to_le_bytes());
    assert_eq!(changed.to_bytes(), expected);
}

#[test]
fn lookup_aliases_and_invalid_indices_remain_raw_until_access() {
    let bytes = source(-2, 2, &[0, 0, u32::MAX - 1, 1], &[event(-2, 0)]);
    let parsed = MotionEvents::parse(&bytes).unwrap();
    assert_eq!(
        parsed.lookup.as_ref().unwrap().event_indices,
        [Some(0), Some(0), Some(u32::MAX - 1), Some(1)]
    );
    assert_eq!(parsed.events_for_motion(-2).unwrap(), parsed.events);
    // A mismatched first event stops traversal; an index exactly at the end
    // also yields no records, matching the native loop's upper bound.
    assert!(parsed.events_for_motion(-1).unwrap().is_empty());
    assert!(parsed.events_for_motion(1).unwrap().is_empty());
    let error = parsed.events_for_motion(0).unwrap_err();
    assert_eq!(error.offset, 20);
    assert_eq!(parsed.as_bytes(), bytes);
}

#[test]
fn event_query_stops_at_the_first_different_motion_without_reordering() {
    let bytes = source(4, 6, &[0, 1], &[event(4, 9), event(5, 0), event(4, 1)]);
    let parsed = MotionEvents::parse(&bytes).unwrap();
    assert_eq!(parsed.events_for_motion(4).unwrap(), &parsed.events[..1]);
    assert_eq!(parsed.events_for_motion(5).unwrap(), &parsed.events[1..2]);
    assert_eq!(parsed.events[2].frame, 1);
    assert_eq!(parsed.as_bytes(), bytes);
}

#[test]
fn empty_and_unindexed_event_tables_keep_their_declared_layout() {
    let empty = [0; 8];
    let parsed = MotionEvents::parse(&empty).unwrap();
    assert!(parsed.lookup.is_none());
    assert!(parsed.events.is_empty());
    assert!(parsed.trailing_bytes.is_empty());
    assert!(parsed.events_for_motion(0).unwrap().is_empty());
    assert_eq!(parsed.as_bytes(), empty);

    let record = event(3, 5);
    let bytes = source(0, 0, &[], &[record]);
    let parsed = MotionEvents::parse(&bytes).unwrap();
    assert!(parsed.lookup.is_none());
    assert_eq!(parsed.events.len(), 1);
    assert_eq!(parsed.events[0].offset, 8);
    assert_eq!(parsed.events[0].to_bytes(), record);
    assert!(parsed.events_for_motion(3).unwrap().is_empty());
}

#[test]
fn full_signed_range_preserves_all_sparse_slots() {
    let mut indices = vec![u32::MAX; usize::from(u16::MAX)];
    indices[0] = 0;
    indices[usize::from(u16::MAX) - 1] = 1;
    let bytes = source(
        i16::MIN,
        i16::MAX,
        &indices,
        &[event(i16::MIN, 0), event(i16::MAX - 1, 1)],
    );
    let parsed = MotionEvents::parse(&bytes).unwrap();
    assert_eq!(parsed.lookup_count, u16::MAX);
    assert_eq!(
        parsed.lookup.as_ref().unwrap().event_indices.len(),
        indices.len()
    );
    assert_eq!(
        parsed.events_for_motion(i16::MIN).unwrap(),
        &parsed.events[..1]
    );
    assert_eq!(
        parsed.events_for_motion(i16::MAX - 1).unwrap(),
        &parsed.events[1..]
    );
    assert!(parsed.events_for_motion(0).unwrap().is_empty());
    assert!(parsed.events_for_motion(i16::MAX).unwrap().is_empty());
}

#[test]
fn truncated_tables_and_inconsistent_ranges_fail_before_reading_neighbors() {
    let bytes = source(-2, 2, &[0, u32::MAX, 1, 2], &[event(-2, 1), event(0, 2)]);
    for length in 0..bytes.len() {
        assert!(
            MotionEvents::parse(&bytes[..length]).is_err(),
            "length {length}"
        );
    }
    assert!(MotionEvents::parse(&bytes).is_ok());
    for (offset, value) in [(2, u16::MAX), (4, u16::MAX), (8, 3), (10, 3)] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        assert!(MotionEvents::parse(&invalid).is_err(), "offset {offset}");
    }
}

#[test]
#[ignore = "requires locally decoded kind-2 effect payloads"]
fn local_kind2_payloads_preserve_all_slots_and_serialize_byte_identically() {
    let directory = std::env::var_os("MHF_EFFECT_SAMPLE_DIR")
        .expect("set MHF_EFFECT_SAMPLE_DIR to the decoded sample directory");
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("effect-kind2-") && name.ends_with(".bin"))
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no effect-kind2-*.bin samples found");
    let mut total_slots = 0;
    let mut total_events = 0;
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let parsed = MotionEvents::parse(&bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(parsed.as_bytes(), bytes, "{}", path.display());
        let mut rebuilt = Vec::new();
        for word in [
            parsed.unknown_00,
            parsed.lookup_count,
            parsed.event_count,
            parsed.unknown_06,
        ] {
            rebuilt.extend(word.to_le_bytes());
        }
        if let Some(lookup) = &parsed.lookup {
            assert_eq!(lookup.offset, rebuilt.len());
            rebuilt.extend(lookup.start.to_le_bytes());
            rebuilt.extend(lookup.end.to_le_bytes());
            for (slot, index) in lookup.event_indices.iter().enumerate() {
                rebuilt.extend(index.unwrap_or(u32::MAX).to_le_bytes());
                let id = (i32::from(lookup.start) + slot as i32) as i16;
                let events = parsed
                    .events_for_motion(id)
                    .unwrap_or_else(|error| panic!("{}, motion {id}: {error}", path.display()));
                if index.is_none() {
                    assert!(events.is_empty());
                } else {
                    assert_eq!(events.first().map(|event| event.motion_id), Some(id));
                }
            }
            total_slots += lookup.event_indices.len();
        }
        for event in &parsed.events {
            assert_eq!(event.offset, rebuilt.len());
            let serialized = event.to_bytes();
            assert_eq!(
                serialized,
                bytes[event.offset..event.offset + serialized.len()],
                "{}, event at {:#x}",
                path.display(),
                event.offset
            );
            rebuilt.extend(serialized);
        }
        rebuilt.extend(parsed.trailing_bytes);
        assert_eq!(rebuilt, bytes, "{}", path.display());
        total_events += parsed.events.len();
    }
    println!(
        "{} kind-2 payloads: {total_slots} lookup slots and {total_events} events preserved",
        paths.len()
    );
}
