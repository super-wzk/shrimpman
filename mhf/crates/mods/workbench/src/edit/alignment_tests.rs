use super::repack;
use crate::inspect::Kind;
use mhf_resource::container::{Entry, MhaArchive, SimpleArchive, StageArchive};

const STALE_EMPTY_OFFSET: usize = 0xffff_ff00;

fn word(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 4].copy_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
}

fn align(value: usize, boundary: usize) -> usize {
    value.div_ceil(boundary) * boundary
}

fn range(entry: Entry) -> std::ops::Range<usize> {
    entry.offset as usize..entry.offset as usize + entry.size as usize
}

/// Slots can alias physical members or retain an unallocated empty entry.
fn mha(payloads: &[&[u8]], slots: &[Option<usize>]) -> Vec<u8> {
    let mut bytes = vec![0; 24];
    bytes[..4].copy_from_slice(b"mha\x01");
    let mut locations = Vec::new();
    for payload in payloads {
        let offset = bytes.len();
        let allocation = align(payload.len() + 24, 512);
        bytes.extend_from_slice(payload);
        bytes.resize(offset + allocation, 0);
        locations.push((offset, payload.len(), allocation));
    }
    let names_offset = bytes.len();
    let mut name_offsets = Vec::new();
    for index in 0..slots.len() {
        name_offsets.push(bytes.len() - names_offset);
        bytes.extend_from_slice(format!("{index}.bin\0").as_bytes());
    }
    let table_offset = bytes.len();
    bytes.resize(table_offset + slots.len() * 20, 0);
    for (at, value) in [
        (4, table_offset),
        (8, slots.len()),
        (12, names_offset),
        (16, table_offset - names_offset),
        (20, 500 | slots.len() << 16),
    ] {
        word(&mut bytes, at, value);
    }
    for (index, slot) in slots.iter().enumerate() {
        let (offset, size, allocation) =
            slot.map_or((STALE_EMPTY_OFFSET, 0, 0), |slot| locations[slot]);
        let field = table_offset + index * 20;
        for (at, value) in [
            (field, name_offsets[index]),
            (field + 4, offset),
            (field + 8, size),
            (field + 12, allocation),
            (field + 16, 500 + index),
        ] {
            word(&mut bytes, at, value);
        }
    }
    MhaArchive::parse(&bytes, slots.len()).unwrap();
    bytes
}

fn directory(
    momo: bool,
    payloads: &[&[u8]],
    slots: &[Option<usize>],
    boundary: usize,
    pad_end: bool,
) -> Vec<u8> {
    let table = if momo { 8 } else { 4 };
    let mut bytes = vec![0; table + slots.len() * 8];
    if momo {
        bytes[..4].copy_from_slice(b"MOMO");
    }
    word(&mut bytes, table - 4, slots.len());
    let mut locations = Vec::new();
    for payload in payloads {
        bytes.resize(align(bytes.len(), boundary), 0);
        locations.push((bytes.len(), payload.len()));
        bytes.extend_from_slice(payload);
    }
    if pad_end {
        bytes.resize(align(bytes.len(), boundary), 0);
    }
    for (index, slot) in slots.iter().enumerate() {
        let (offset, size) = slot.map_or((STALE_EMPTY_OFFSET, 0), |slot| locations[slot]);
        word(&mut bytes, table + index * 8, offset);
        word(&mut bytes, table + index * 8 + 4, size);
    }
    SimpleArchive::parse(&bytes, slots.len()).unwrap();
    bytes
}

#[test]
fn mha_rebuilds_allocations_across_size_plus_24_boundaries() {
    // At 488 the extra 24 bytes exactly fill a sector. Both 489 and 512
    // require two sectors, although align_up(size, 512) would use only one.
    for (old_size, new_size, allocation) in [
        (488, 489, 1024),
        (489, 488, 512),
        (488, 512, 1024),
        (512, 0, 512),
    ] {
        let original = vec![0x41; old_size];
        let replacement = vec![0x42; new_size];
        let source = mha(
            &[&original, b"retained sibling", &[]],
            &[Some(0), Some(0), Some(1), Some(2), None],
        );
        let before = MhaArchive::parse(&source, 5).unwrap();
        let updated = repack::replace(
            Kind::Mha,
            &source,
            range(before.entries[0].entry),
            &replacement,
        )
        .unwrap();
        let after = MhaArchive::parse(&updated, 5).unwrap();
        for index in [0, 1] {
            assert_eq!(after.entries[index].entry.offset, 24);
            assert_eq!(after.entries[index].padded_size as usize, allocation);
            assert_eq!(
                after.entries[index].entry.payload(&updated).unwrap(),
                replacement
            );
        }
        assert_eq!(after.entries[2].entry.offset as usize, 24 + allocation);
        assert_eq!(
            after.entries[2].entry.payload(&updated).unwrap(),
            b"retained sibling"
        );
        assert_eq!(
            after.entries[3].entry.offset as usize,
            24 + allocation + 512
        );
        assert_eq!(after.entries[3].entry.size, 0);
        assert_eq!(after.entries[3].padded_size, 512);
        assert_eq!(after.entries[4].entry.offset as usize, STALE_EMPTY_OFFSET);
        assert_eq!(after.entries[4].padded_size, 0);
        assert_eq!(after.header.names_offset as usize, 24 + allocation + 1024);
        assert_eq!(
            after.header.entries_offset,
            after.header.names_offset + after.header.names_size
        );
        assert_eq!(updated.len(), after.header.entries_offset as usize + 100);
        assert_eq!(after.header.names_size, before.header.names_size);
        assert_eq!(&updated[20..24], &source[20..24]);
        for (old, new) in before.entries.iter().zip(&after.entries) {
            assert_eq!(
                (new.name, new.name_offset, new.file_id),
                (old.name, old.name_offset, old.file_id)
            );
            if new.padded_size != 0 {
                let padding =
                    range(new.entry).end..new.entry.offset as usize + new.padded_size as usize;
                assert!(updated[padding].iter().all(|&byte| byte == 0));
            }
        }
    }
}

#[test]
fn mha_can_replace_an_allocated_empty_member_without_moving_its_start() {
    let source = mha(&[&[], b"sibling"], &[Some(0), Some(1)]);
    let updated = repack::replace(Kind::Mha, &source, 24..24, &[0x51; 489]).unwrap();
    let parsed = MhaArchive::parse(&updated, 2).unwrap();
    assert_eq!(parsed.entries[0].entry.offset, 24);
    assert_eq!(parsed.entries[0].padded_size, 1024);
    assert_eq!(
        parsed.entries[0].entry.payload(&updated).unwrap(),
        &[0x51; 489]
    );
    assert_eq!(parsed.entries[1].entry.offset, 1048);
    assert_eq!(
        parsed.entries[1].entry.payload(&updated).unwrap(),
        b"sibling"
    );
    assert_eq!(parsed.header.names_offset, 1560);
}

fn assert_momo_layout(bytes: &[u8], offsets: [u32; 3], eof: usize) {
    let parsed = SimpleArchive::parse(bytes, 5).unwrap();
    // Logical order differs from physical order, and slots 1 and 3 alias.
    assert_eq!(parsed.entries[1].offset, offsets[0]);
    assert_eq!(parsed.entries[3].offset, offsets[0]);
    assert_eq!(parsed.entries[3].size, parsed.entries[1].size);
    assert_eq!(parsed.entries[2].offset, offsets[1]);
    assert_eq!(parsed.entries[0].offset, offsets[2]);
    assert_eq!(parsed.entries[4].offset as usize, STALE_EMPTY_OFFSET);
    assert_eq!(bytes.len(), eof);
    let mut end = parsed.table_offset + parsed.entries.len() * 8;
    for index in [1, 2, 0] {
        let member = range(parsed.entries[index]);
        assert!(bytes[end..member.start].iter().all(|&byte| byte == 0));
        end = member.end;
    }
    assert!(bytes[end..].iter().all(|&byte| byte == 0));
}

#[test]
fn momo_preserves_aliases_and_rebuilds_first_last_and_eof_padding() {
    let source = directory(
        true,
        &[b"first", b"sibling", b"end"],
        &[Some(2), Some(0), Some(1), Some(0), None],
        64,
        true,
    );
    assert_momo_layout(&source, [64, 128, 192], 256);
    let updated = repack::replace(Kind::Momo, &source, 64..69, &[0x41; 65]).unwrap();
    assert_momo_layout(&updated, [64, 192, 256], 320);
    let parsed = SimpleArchive::parse(&updated, 5).unwrap();
    assert_eq!(parsed.payload(1).unwrap(), &[0x41; 65]);
    assert_eq!(parsed.payload(2).unwrap(), b"sibling");
    assert_eq!(parsed.payload(0).unwrap(), b"end");
    let updated = repack::replace(Kind::Momo, &updated, 256..259, &[0x42; 65]).unwrap();
    assert_momo_layout(&updated, [64, 192, 256], 384);
    let updated = repack::replace(Kind::Momo, &updated, 64..129, b"x").unwrap();
    assert_momo_layout(&updated, [64, 128, 192], 320);
    let parsed = SimpleArchive::parse(&updated, 5).unwrap();
    assert_eq!(parsed.payload(1).unwrap(), b"x");
    assert_eq!(parsed.payload(3).unwrap(), b"x");
    assert_eq!(parsed.payload(2).unwrap(), b"sibling");
    assert_eq!(parsed.payload(0).unwrap(), &[0x42; 65]);
}

#[test]
fn aligned_batch_replacements_use_original_ranges_in_reverse_physical_order() {
    let source = directory(
        true,
        &[b"first", b"sibling", b"end"],
        &[Some(2), Some(0), Some(1), Some(0), None],
        64,
        true,
    );
    let first = [0x41; 65];
    let last = [0x42; 129];
    let updated =
        repack::replace_many(Kind::Momo, &source, &[(64..69, &first), (192..195, &last)]).unwrap();
    assert_momo_layout(&updated, [64, 192, 256], 448);
    let parsed = SimpleArchive::parse(&updated, 5).unwrap();
    assert_eq!(parsed.payload(1).unwrap(), first);
    assert_eq!(parsed.payload(0).unwrap(), last);
    assert_eq!(parsed.payload(2).unwrap(), b"sibling");

    let source = mha(
        &[&[0x41; 488], b"sibling", &[0x42; 488]],
        &[Some(0), Some(1), Some(2)],
    );
    let updated = repack::replace_many(
        Kind::Mha,
        &source,
        &[(24..512, &[0x51; 489]), (1048..1536, &[0x52; 489])],
    )
    .unwrap();
    let parsed = MhaArchive::parse(&updated, 3).unwrap();
    assert_eq!(parsed.entries[0].padded_size, 1024);
    assert_eq!(parsed.entries[1].entry.offset, 1048);
    assert_eq!(parsed.entries[2].entry.offset, 1560);
    assert_eq!(parsed.entries[2].padded_size, 1024);
    assert_eq!(parsed.header.names_offset, 2584);
    assert_eq!(
        parsed.entries[1].entry.payload(&updated).unwrap(),
        b"sibling"
    );
    assert_eq!(
        parsed.entries[2].entry.payload(&updated).unwrap(),
        &[0x52; 489]
    );
}

#[test]
fn aligned_directories_can_clear_the_last_member_or_all_members_in_a_batch() {
    let source = directory(true, &[b"one"], &[Some(0)], 64, true);
    let output = repack::replace(Kind::Momo, &source, 64..67, &[]).unwrap();
    let parsed = SimpleArchive::parse(&output, 1).unwrap();
    assert_eq!(parsed.entries[0].size, 0);
    assert_eq!(parsed.entries[0].offset, 64);
    assert_eq!(output.len(), 64);

    let source = directory(true, &[b"one", b"two"], &[Some(0), Some(1)], 64, true);
    let output =
        repack::replace_many(Kind::Momo, &source, &[(64..67, &[]), (128..131, &[])]).unwrap();
    let parsed = SimpleArchive::parse(&output, 2).unwrap();
    assert!(parsed.entries.iter().all(|entry| entry.size == 0));
    assert_eq!(output.len(), 64);
}

#[test]
fn decoded_field_batch_rebuilds_nested_aligned_containers_without_moving_fixed_allocations() {
    use crate::{
        field::{Binding, FieldType, Patch},
        inspect,
    };
    use mhf_resource::jkr::Jkr;

    // A literal plus a long LZ back-reference produces 100 bytes from five
    // encoded bytes. Editing one byte makes the stored JKR representation grow
    // across a MOMO boundary, while its outer MHA allocation still fits.
    let mut compressed = b"JKR\x1a\x08\x01\x03\0".to_vec();
    compressed.extend_from_slice(&16u32.to_le_bytes());
    compressed.extend_from_slice(&100u32.to_le_bytes());
    compressed.extend_from_slice(&[0x70, b'A', 0, 0, 73]);
    assert_eq!(
        &**Jkr::parse(&compressed).unwrap().decode(100).unwrap(),
        &[b'A'; 100]
    );
    let inner = directory(
        true,
        &[&compressed, &compressed],
        &[Some(0), Some(1)],
        64,
        true,
    );
    let source = mha(&[&inner, b"outer sibling"], &[Some(0), Some(1)]);
    let document = inspect::inspect("nested.abn", source.clone().into());
    let leaves: Vec<_> = document
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| document.bytes(*index) == Some(&[b'A'; 100]))
        .collect();
    assert_eq!(leaves.len(), 2);
    let patches: Vec<_> = leaves
        .iter()
        .enumerate()
        .map(|(index, (_, node))| Patch {
            binding: Binding {
                buffer: node.buffer,
                range: 0..1,
                endian: Default::default(),
                format: FieldType::Bytes,
            },
            before: vec![b'A'],
            after: vec![b'B' + index as u8],
        })
        .collect();
    let updated = super::apply_many(&document, &patches).unwrap();
    let old = MhaArchive::parse(&source, 2).unwrap();
    let outer = MhaArchive::parse(&updated.buffers[0], 2).unwrap();
    assert_eq!(outer.header.names_offset, old.header.names_offset);
    assert_eq!(outer.header.entries_offset, old.header.entries_offset);
    assert_eq!(outer.entries[0].padded_size, 512);
    assert_eq!(outer.entries[0].entry.size, 320);
    assert_eq!(
        outer.entries[1].entry.payload(&updated.buffers[0]).unwrap(),
        b"outer sibling"
    );
    let inner = SimpleArchive::parse(
        outer.entries[0].entry.payload(&updated.buffers[0]).unwrap(),
        2,
    )
    .unwrap();
    assert_eq!(inner.entries[0].offset, 64);
    assert_eq!(inner.entries[1].offset, 192);
    for index in 0..2 {
        let decoded = Jkr::parse(inner.payload(index).unwrap())
            .unwrap()
            .decode(100)
            .unwrap();
        assert_eq!(decoded[0], b'B' + index as u8);
        assert_eq!(&decoded[1..], &[b'A'; 99]);
    }
}

fn counted_words(count: usize) -> Vec<u8> {
    let mut bytes = vec![0x5a; 8 + count * 4];
    word(&mut bytes, 4, count);
    bytes
}

fn object_package(boundary: usize) -> Vec<u8> {
    // Valid descriptor: two members of the counted-word kind.
    directory(
        false,
        &[&[1, 0, 2, 0, 14, 14], &counted_words(1), &counted_words(0)],
        &[Some(0), Some(1), Some(2)],
        boundary,
        false,
    )
}

#[test]
fn object_packages_keep_their_own_tight_or_minimum_16_layout() {
    for (boundary, expected_offsets, expected_eof) in
        [(1, [28, 34, 54], 62), (16, [32, 48, 80], 88)]
    {
        let source = object_package(boundary);
        mhf_resource::stage::ObjectPackage::probe(&source, 3).unwrap();
        let before = SimpleArchive::parse(&source, 3).unwrap();
        let replacement = counted_words(3);
        let updated = repack::replace(
            Kind::StageObjectPackage,
            &source,
            range(before.entries[1]),
            &replacement,
        )
        .unwrap();
        let after = SimpleArchive::parse(&updated, 3).unwrap();
        assert_eq!(
            after
                .entries
                .iter()
                .map(|entry| entry.offset)
                .collect::<Vec<_>>(),
            expected_offsets
        );
        assert_eq!(updated.len(), expected_eof);
        assert_eq!(after.payload(0).unwrap(), before.payload(0).unwrap());
        assert_eq!(after.payload(1).unwrap(), replacement);
        assert_eq!(after.payload(2).unwrap(), before.payload(2).unwrap());
        if boundary == 16 {
            assert!(updated[28..32].iter().all(|&byte| byte == 0));
            assert!(updated[38..48].iter().all(|&byte| byte == 0));
            assert!(updated[68..80].iter().all(|&byte| byte == 0));
        }
    }
}

fn placements(count: usize) -> Vec<u8> {
    let mut bytes = vec![0; 16 + count * 60];
    word(&mut bytes, 0, 2);
    word(&mut bytes, 4, count);
    word(&mut bytes, 8, u32::MAX as usize);
    for index in 0..count {
        bytes[16 + index * 60 + 54..16 + index * 60 + 56].copy_from_slice(&77u16.to_le_bytes());
    }
    bytes
}

#[test]
fn stage_keeps_extra_whole_padding_blocks_and_does_not_pad_the_file_end() {
    let object = object_package(1);
    let mut source = vec![0; 160];
    for (at, value) in [
        (0, 48),
        (4, 76),
        (8, STALE_EMPTY_OFFSET),
        (16, 7),
        (24, 1),
        (28, 77),
        (32, 160),
        (36, object.len()),
    ] {
        word(&mut source, at, value);
    }
    source[48..124].copy_from_slice(&placements(1));
    source.extend_from_slice(&object);
    StageArchive::probe(&source, 4).unwrap();
    let mut updated = source;
    for (count, object_offset) in [(2, 224), (0, 96)] {
        let before = StageArchive::parse(&updated, 4).unwrap();
        let payload = placements(count);
        updated = repack::replace(
            Kind::Stage,
            &updated,
            range(before.entries[0].entry),
            &payload,
        )
        .unwrap();
        let after = StageArchive::probe(&updated, 4).unwrap();
        assert_eq!(after.entries[0].entry.offset, 48);
        assert_eq!(after.entries[0].entry.payload(&updated).unwrap(), payload);
        assert_eq!(after.entries[1].entry.offset as usize, STALE_EMPTY_OFFSET);
        assert_eq!(after.entries[2].entry.offset, 7);
        assert_eq!(after.entries[3].resource_id, Some(77));
        assert_eq!(after.entries[3].entry.offset as usize, object_offset);
        assert_eq!(after.entries[3].entry.payload(&updated).unwrap(), object);
        assert_eq!(object_offset - align(48 + payload.len(), 16), 32);
        assert!(
            updated[48 + payload.len()..object_offset]
                .iter()
                .all(|&byte| byte == 0)
        );
        assert_eq!(updated.len(), object_offset + object.len());
        assert_ne!(updated.len() % 16, 0);
    }
}

#[test]
fn ordinary_and_txb_directories_keep_odd_tight_offsets() {
    let source = directory(false, &[b"abc", b"tail"], &[Some(0), Some(1)], 1, false);
    for kind in [Kind::Archive, Kind::Txb] {
        let updated = repack::replace(kind, &source, 20..23, b"12345").unwrap();
        let after = SimpleArchive::parse(&updated, 2).unwrap();
        assert_eq!(after.entries[1].offset, 25);
        assert_eq!(after.payload(0).unwrap(), b"12345");
        assert_eq!(after.payload(1).unwrap(), b"tail");
        assert_eq!(updated.len(), 29);
    }
}

#[test]
fn repacking_rejects_partial_payload_and_allocation_overlaps() {
    let mut source = directory(true, &[b"first", b"sibling"], &[Some(0), Some(1)], 64, true);
    word(&mut source, 16, 66);
    SimpleArchive::parse(&source, 2).unwrap();
    assert!(repack::replace(Kind::Momo, &source, 64..69, b"replacement").is_err());

    let mut source = mha(
        &[b"first", b"second", b"third"],
        &[Some(0), Some(1), Some(2)],
    );
    let table = MhaArchive::parse(&source, 3).unwrap().header.entries_offset as usize;
    word(&mut source, table + 12, 1024);
    MhaArchive::parse(&source, 3).unwrap();
    assert!(repack::replace(Kind::Mha, &source, 24..29, b"replacement").is_err());
}

#[test]
fn mha_rejects_payload_or_allocation_overlapping_tail_metadata() {
    let original = mha(&[b"first"], &[Some(0)]);
    let header = MhaArchive::parse(&original, 1).unwrap().header;
    let table = header.entries_offset as usize;
    let names = header.names_offset as usize;
    let mut source = original.clone();
    word(&mut source, table + 4, names);
    word(&mut source, table + 8, header.names_size as usize);
    word(&mut source, table + 12, header.names_size as usize);
    MhaArchive::parse(&source, 1).unwrap();
    assert!(repack::replace(Kind::Mha, &source, names..table, b"replacement").is_err());

    let mut source = original;
    let allocation = source.len() - 24;
    word(&mut source, table + 12, allocation);
    MhaArchive::parse(&source, 1).unwrap();
    assert!(repack::replace(Kind::Mha, &source, 24..29, b"replacement").is_err());
}
