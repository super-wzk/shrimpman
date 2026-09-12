use mhf_resource::{
    binary::Reader,
    container::{MhaArchive, MhaHeader},
};

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn archive(first_id: i16, id_count: u16, records: &[(u32, &[u8])]) -> Vec<u8> {
    let names: Vec<_> = (0..records.len()).map(|id| format!("{id}.bin\0")).collect();
    let names_offset = 24 + records.len() * 20;
    let names_size: usize = names.iter().map(String::len).sum();
    let mut bytes = vec![0; names_offset];
    bytes[..4].copy_from_slice(b"mha\x01");
    word(&mut bytes, 4, 24);
    word(&mut bytes, 8, records.len() as u32);
    word(&mut bytes, 12, names_offset as u32);
    word(&mut bytes, 16, names_size as u32);
    bytes[20..22].copy_from_slice(&first_id.to_le_bytes());
    bytes[22..24].copy_from_slice(&id_count.to_le_bytes());
    for name in &names {
        bytes.extend_from_slice(name.as_bytes());
    }
    let mut name_offset = 0;
    for (index, &(id, payload)) in records.iter().enumerate() {
        let entry = 24 + index * 20;
        let payload_offset = bytes.len();
        word(&mut bytes, entry, name_offset);
        word(&mut bytes, entry + 4, payload_offset as u32);
        word(&mut bytes, entry + 8, payload.len() as u32);
        word(&mut bytes, entry + 12, payload.len() as u32);
        word(&mut bytes, entry + 16, id);
        bytes.extend_from_slice(payload);
        name_offset += names[index].len() as u32;
    }
    bytes
}

#[test]
fn signed_low_word_ids_build_slots_without_interpreting_the_high_word() {
    let source = archive(
        -2,
        4,
        &[(0xabcd_fffe, b"a"), (0x1234_0001, b"b"), (u32::MAX, b"c")],
    );
    let parsed = MhaArchive::parse(&source, 3).unwrap();
    assert_eq!(parsed.header.first_file_id, -2);
    assert_eq!(parsed.header.file_id_count, 4);
    assert_eq!(parsed.entries[0].file_id, 0xabcd_fffe);
    assert_eq!(parsed.entries[0].native_file_id(), -2);
    assert_eq!(parsed.entries[1].native_file_id(), 1);
    assert_eq!(parsed.entries[2].file_id, u32::MAX);
    assert_eq!(parsed.entries[2].native_file_id(), -1);
    let index = parsed.file_id_index().unwrap();
    assert_eq!(index.first_file_id, -2);
    assert_eq!(index.slots, [Some(0), Some(2), None, Some(1)]);
    for (id, entry) in [
        (-3, None),
        (-2, Some(0)),
        (-1, Some(2)),
        (0, None),
        (1, Some(1)),
        (2, None),
    ] {
        assert_eq!(index.entry_index(id), entry);
    }
    assert_eq!(index.entry_index(i32::MIN), None);
    assert_eq!(index.entry_index(i32::MAX), None);
}

#[test]
fn later_ids_overwrite_earlier_entries_even_when_the_last_record_is_empty() {
    let mut source = archive(
        500,
        3,
        &[
            (500, b"first"),
            (502, b"unrelated"),
            (0xa55a_01f4, b"second"),
            (500, b""),
        ],
    );
    // A written zero/zero descriptor remains distinct from an unwritten slot.
    word(&mut source, 24 + 3 * 20 + 4, 0);
    let parsed = MhaArchive::parse(&source, 4).unwrap();
    let index = parsed.file_id_index().unwrap();
    assert_eq!(index.slots, [Some(3), None, Some(1)]);
    assert_eq!(index.entry_index(500), Some(3));
    assert_eq!(parsed.entries[3].entry.offset, 0);
    assert_eq!(parsed.entries[3].entry.size, 0);
    assert_eq!(parsed.entries[0].entry.payload(&source).unwrap(), b"first");
    assert_eq!(parsed.entries[2].entry.payload(&source).unwrap(), b"second");
    assert_eq!(parsed.entries[2].file_id, 0xa55a_01f4);
    assert_eq!(
        parsed.entries[1].entry.payload(&source).unwrap(),
        b"unrelated"
    );
}

#[test]
fn slot_count_uses_the_native_signed_bound_while_preserving_unsigned_storage() {
    for count in [0x8000, u16::MAX] {
        let source = archive(0, count, &[]);
        let parsed = MhaArchive::parse(&source, 0).unwrap();
        assert_eq!(parsed.header.file_id_count, count);
        let error = parsed.file_id_index().unwrap_err();
        assert_eq!(error.offset, 22);
        assert!(error.message.contains("signed 16-bit"));
    }
    let source = archive(
        i16::MIN,
        i16::MAX as u16,
        &[(0x8000, b"first"), (0xfffe, b"last")],
    );
    let parsed = MhaArchive::parse(&source, 2).unwrap();
    let index = parsed.file_id_index().unwrap();
    assert_eq!(index.slots.len(), 32767);
    assert_eq!(index.entry_index(-32768), Some(0));
    assert_eq!(index.entry_index(-2), Some(1));
    assert_eq!(index.entry_index(-1), None);
    assert!(index.slots[1..32766].iter().all(Option::is_none));
}

#[test]
fn invalid_ids_report_the_record_field_and_remain_available_for_repair() {
    for id in [99, 102] {
        let source = archive(100, 2, &[(100, b"valid"), (id, b"invalid")]);
        let parsed = MhaArchive::parse(&source, 2).unwrap();
        let error = parsed.file_id_index().unwrap_err();
        assert_eq!(error.offset, 24 + 20 + 16);
        assert!(error.message.contains("outside native ID range 100..102"));
        assert_eq!(
            parsed.entries[1].entry.payload(&source).unwrap(),
            b"invalid"
        );

        let id_field = Reader::new(&source).read_at::<i16>(error.offset).unwrap();
        let mut repaired = source.clone();
        id_field.write(&mut repaired, 101).unwrap();
        let fixed = MhaArchive::parse(&repaired, 2).unwrap();
        assert_eq!(fixed.file_id_index().unwrap().slots, [Some(0), Some(1)]);
        assert_eq!(&repaired[error.offset + 2..], &source[error.offset + 2..]);
    }
}

#[test]
fn empty_id_ranges_accept_empty_directories_but_not_even_zero_size_records() {
    let source = archive(-3, 0, &[]);
    let parsed = MhaArchive::parse(&source, 0).unwrap();
    let index = parsed.file_id_index().unwrap();
    assert!(index.slots.is_empty());
    assert_eq!(index.entry_index(-3), None);

    let source = archive(-3, 0, &[(0xfffd, b"")]);
    let parsed = MhaArchive::parse(&source, 1).unwrap();
    assert_eq!(parsed.file_id_index().unwrap_err().offset, 40);
}

#[test]
fn declared_ids_do_not_wrap_at_the_signed_id_limit() {
    let source = archive(i16::MAX, 2, &[(0x7fff, b"last signed ID")]);
    let index = MhaArchive::parse(&source, 1)
        .unwrap()
        .file_id_index()
        .unwrap();
    assert_eq!(index.slots, [Some(0), None]);
    assert_eq!(index.entry_index(32767), Some(0));
    assert_eq!(index.entry_index(32768), None);
    assert_eq!(index.entry_index(-32768), None);

    let source = archive(i16::MAX, 2, &[(0x8000, b"negative ID")]);
    assert!(
        MhaArchive::parse(&source, 1)
            .unwrap()
            .file_id_index()
            .is_err()
    );
}

#[test]
fn header_fields_remain_readable_when_the_directory_is_outside_the_source() {
    let mut source = archive(-7, 8, &[]);
    word(&mut source, 4, u32::MAX);
    let header = MhaHeader::parse(&source).unwrap();
    assert_eq!(header.first_file_id, -7);
    assert_eq!(header.file_id_count, 8);
    assert_eq!(header.entries_offset, u32::MAX);
    assert!(MhaArchive::parse(&source, 0).is_err());
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original MHA archives without writing them"]
fn original_mha_ids_follow_signed_ranges_and_last_record_selection() {
    use std::{collections::BTreeMap, fs, path::PathBuf};

    let root = PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut directories = vec![root.join("dat")];
    let mut archives = 0;
    let mut records = 0;
    while let Some(directory) = directories.pop() {
        for file in fs::read_dir(directory).unwrap() {
            let file = file.unwrap();
            let kind = file.file_type().unwrap();
            let path = file.path();
            if kind.is_dir() {
                directories.push(path);
                continue;
            }
            if !kind.is_file()
                || path
                    .extension()
                    .is_none_or(|extension| !extension.eq_ignore_ascii_case("abn"))
            {
                continue;
            }
            let source = fs::read(&path).unwrap();
            if !source.starts_with(b"mha\x01") {
                continue;
            }
            let parsed = MhaArchive::parse(&source, source.len())
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let index = parsed
                .file_id_index()
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(index.slots.len(), usize::from(parsed.header.file_id_count));

            let expected: BTreeMap<_, _> = parsed
                .entries
                .iter()
                .enumerate()
                .map(|(entry, item)| (i32::from(item.native_file_id()), entry))
                .collect();
            for (slot, &selected) in index.slots.iter().enumerate() {
                let id = i32::from(index.first_file_id) + slot as i32;
                assert_eq!(
                    selected,
                    expected.get(&id).copied(),
                    "{}: ID {id}",
                    path.display()
                );
                assert_eq!(index.entry_index(id), selected);
            }
            assert_eq!(index.entry_index(i32::from(index.first_file_id) - 1), None);
            assert_eq!(
                index.entry_index(i32::from(index.first_file_id) + index.slots.len() as i32),
                None
            );
            archives += 1;
            records += parsed.entries.len();
        }
    }
    assert!(archives > 0, "no original MHA archives found below dat");
    eprintln!(
        "validated native ID indexes for {archives} MHA archives and {records} directory records"
    );
}
