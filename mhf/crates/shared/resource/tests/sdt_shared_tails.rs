use mhf_resource::sdt::{HITBOX_GROUP_STRIDE, HITBOX_SLOTS, HITBOX_STRIDE, Sdt};

fn word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn dword(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn directory(size: usize, groups: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    word(&mut bytes, 2, 1000);
    word(&mut bytes, 4, 1);
    word(&mut bytes, 6, groups.try_into().unwrap());
    dword(&mut bytes, 8, 64);
    dword(&mut bytes, 16, 128);
    word(&mut bytes, 30, u16::MAX);
    bytes
}

#[test]
fn many_shared_suffixes_keep_each_slot_and_failed_tails_remain_unclaimed() {
    // Descending source references require the bulk pass to sort by physical
    // offset. This remains a small image despite its many overlapping lists.
    const LISTS: usize = 32_768;
    let groups = LISTS / HITBOX_SLOTS;
    let start = 128 + groups * HITBOX_GROUP_STRIDE;
    let terminal = start + LISTS * HITBOX_STRIDE;
    let mut bytes = directory(terminal + 2, groups);
    for slot in 0..LISTS {
        dword(
            &mut bytes,
            128 + slot * 4,
            (start + (LISTS - slot - 1) * HITBOX_STRIDE)
                .try_into()
                .unwrap(),
        );
    }
    word(&mut bytes, terminal, u16::MAX);
    let file = Sdt::probe(&bytes).unwrap();
    let entry = file.entry(0).unwrap();
    for (group_index, slot, expected_count) in [
        (0, 0, 1),
        (0, 7, 8),
        (groups - 1, 0, LISTS - 7),
        (groups - 1, 7, LISTS),
    ] {
        let group = file.hitbox_group(entry, group_index).unwrap();
        let list = file.hitboxes(&group, slot).unwrap();
        assert_eq!(list.count, expected_count);
        assert_eq!(list.terminator, terminal..terminal + 2);
        assert_eq!(list.range.start, group.slots[slot] as usize);
    }
    let gaps = file.unclaimed_ranges();
    assert!(gaps.iter().all(|range| range.end <= start));

    // A failed scan must also be reused: omitting the sentinel otherwise
    // rescans every suffix when the workbench asks for unclaimed bytes.
    word(&mut bytes, terminal, 0);
    assert!(Sdt::probe(&bytes).is_err());
    let file = Sdt::parse(&bytes).unwrap();
    let gaps = file.unclaimed_ranges();
    assert!(gaps.iter().any(|range| range == &(start..bytes.len())));
    for (group_index, slot) in [(0, 0), (groups - 1, 7)] {
        let group = file
            .hitbox_group(file.entry(0).unwrap(), group_index)
            .unwrap();
        let error = file.hitboxes(&group, slot).unwrap_err();
        assert_eq!(error.offset, group.offset + slot * 4);
    }
}

#[test]
fn interleaved_strides_and_disjoint_lists_do_not_share_the_wrong_terminator() {
    let start = 192;
    let mut bytes = directory(start + 202, 1);
    // Starts 0/40/80 share one sentinel, starts 4/44/84 share another,
    // and starts 120/160 resume the first stride beyond its old sentinel.
    let starts = [44, 120, 0, 4, 40, 84, 160, 80];
    let counts = [3, 2, 2, 4, 1, 2, 1, 0];
    let ends = [164, 200, 80, 164, 80, 164, 200, 80];
    for (slot, offset) in starts.into_iter().enumerate() {
        dword(&mut bytes, 128 + slot * 4, (start + offset) as u32);
    }
    for offset in [80, 164, 200] {
        word(&mut bytes, start + offset, u16::MAX);
    }
    let file = Sdt::probe(&bytes).unwrap();
    let group = file.hitbox_group(file.entry(0).unwrap(), 0).unwrap();
    for slot in 0..HITBOX_SLOTS {
        let list = file.hitboxes(&group, slot).unwrap();
        assert_eq!(list.range, start + starts[slot]..start + ends[slot]);
        assert_eq!(list.count, counts[slot]);
    }
    assert!(
        file.unclaimed_ranges()
            .iter()
            .all(|range| range.end <= start)
    );

    // Damage the later list on residue zero without changing the earlier
    // sentinel or the independently interleaved list on residue four.
    word(&mut bytes, start + 200, 0);
    let file = Sdt::parse(&bytes).unwrap();
    assert!(
        file.unclaimed_ranges()
            .iter()
            .any(|range| range == &(start + 166..bytes.len()))
    );
}

#[test]
fn recognition_rejects_duplicate_keys_before_reading_the_rest_of_the_directory() {
    let mut bytes = vec![0; 136];
    word(&mut bytes, 2, 1000);
    word(&mut bytes, 4, 1);
    dword(&mut bytes, 8, 96);
    word(&mut bytes, 30, 1000);
    // There is deliberately no directory terminator yet. Recognition should
    // stop at the duplicate, without allocating entries from the data area.
    let error = Sdt::probe(&bytes).unwrap_err();
    assert_eq!(error.offset, 28);
    assert!(error.message.contains("duplicate"));

    word(&mut bytes, 58, u16::MAX);
    let file = Sdt::parse(&bytes).unwrap();
    assert_eq!(file.entries().len(), 2);
    assert_eq!(file.entries()[0].kind, file.entries()[1].kind);
    assert_eq!(Sdt::probe(&bytes).unwrap_err().offset, 28);
}
