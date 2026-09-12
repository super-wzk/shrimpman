//! Optional regression checks against original client resources. Replacements
//! and all resulting archives remain in memory; client files are never written.

use std::{collections::BTreeMap, ops::Range, path::PathBuf};

use mhf_resource::container::{Entry, MhaArchive, SimpleArchive, StageArchive, open_layers};

use crate::inspect::Kind;

use super::repack;

fn game_data() -> PathBuf {
    PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").expect("set MHF_RESOURCE_GAME_ROOT"))
        .join("dat")
}

fn decoded(source: &[u8]) -> Vec<u8> {
    open_layers(source, 512 * 1024 * 1024, 16)
        .unwrap()
        .payload()
        .to_vec()
}

fn range(entry: Entry) -> Range<usize> {
    entry.offset as usize..entry.offset as usize + entry.size as usize
}

fn extended(source: &[u8], count: usize) -> Vec<u8> {
    let mut bytes = source.to_vec();
    bytes.resize(bytes.len() + count, 0xa5);
    bytes
}

fn aligned(value: usize, boundary: usize) -> usize {
    value.div_ceil(boundary) * boundary
}

fn assert_member_bytes(
    before: &[u8],
    old: impl ExactSizeIterator<Item = Entry>,
    after: &[u8],
    new: impl ExactSizeIterator<Item = Entry>,
    selected: Entry,
    replacement: &[u8],
) {
    assert_eq!(old.len(), new.len());
    let mut aliases = BTreeMap::new();
    for (old, new) in old.zip(new) {
        assert_eq!(old.index, new.index);
        if old.size != 0 {
            let location = (new.offset, new.size);
            if let Some(previous) = aliases.insert((old.offset, old.size), location) {
                assert_eq!(previous, location, "alias of member {}", old.index);
            }
        }
        if range(old) == range(selected) {
            assert_eq!(new.size as usize, replacement.len());
            assert_eq!(new.payload(after).unwrap(), replacement);
        } else {
            assert_eq!(old.size, new.size, "member {}", old.index);
            assert_eq!(old.payload(before).unwrap(), new.payload(after).unwrap());
        }
    }
}

fn assert_mha_replacement(original: &MhaArchive<'_>, selected: usize, replacement: &[u8]) {
    let source = original.source;
    let target = &original.entries[selected];
    let output = repack::replace(Kind::Mha, source, range(target.entry), replacement).unwrap();
    let updated = MhaArchive::parse(&output, 65_536).unwrap();
    let allocation = aligned(replacement.len() + 24, 512);
    let delta = allocation as isize - target.padded_size as isize;
    let allocation_end = target.entry.offset as usize + target.padded_size as usize;
    let shifted = |offset: u32| {
        if offset as usize >= allocation_end {
            offset
                .checked_add_signed(i32::try_from(delta).unwrap())
                .unwrap()
        } else {
            offset
        }
    };

    assert_eq!(
        output.len(),
        source.len().checked_add_signed(delta).unwrap()
    );
    assert_eq!(updated.header.magic, original.header.magic);
    assert_eq!(updated.header.count, original.header.count);
    assert_eq!(updated.header.names_size, original.header.names_size);
    assert_eq!(updated.header.first_file_id, original.header.first_file_id);
    assert_eq!(updated.header.file_id_count, original.header.file_id_count);
    assert_eq!(
        updated.header.entries_offset,
        shifted(original.header.entries_offset)
    );
    assert_eq!(
        updated.header.names_offset,
        shifted(original.header.names_offset)
    );
    let names = |header: mhf_resource::container::MhaHeader| {
        header.names_offset as usize..header.names_offset as usize + header.names_size as usize
    };
    assert_eq!(
        source[names(original.header)],
        output[names(updated.header)]
    );

    assert_member_bytes(
        source,
        original.entries.iter().map(|item| item.entry),
        &output,
        updated.entries.iter().map(|item| item.entry),
        target.entry,
        replacement,
    );
    for (old, new) in original.entries.iter().zip(&updated.entries) {
        assert_eq!(old.name_offset, new.name_offset);
        assert_eq!(old.name, new.name);
        assert_eq!(old.file_id, new.file_id);
        assert_eq!(new.entry.offset, shifted(old.entry.offset));
        assert_eq!((new.entry.offset - 24) % 512, 0);
        if range(old.entry) == range(target.entry) {
            assert_eq!(new.padded_size as usize, allocation);
        } else {
            // Empty slots still own their original allocation and metadata.
            assert_eq!(new.padded_size, old.padded_size);
        }
        assert_eq!(
            new.padded_size as usize,
            aligned(new.entry.size as usize + 24, 512)
        );
        let padding = new.entry.offset as usize + new.entry.size as usize
            ..new.entry.offset as usize + new.padded_size as usize;
        assert!(output[padding].iter().all(|byte| *byte == 0));
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original MHA archives only"]
fn original_mha_corpus_preserves_allocations_names_ids_empty_slots_and_members() {
    let mut directories = vec![game_data()];
    let mut archives = Vec::new();
    while let Some(directory) = directories.pop() {
        for item in std::fs::read_dir(directory).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "abn") {
                archives.push(path);
            }
        }
    }
    archives.sort();
    let mut checked = 0;
    let mut empty_slots = 0;
    let mut wi521_checked = false;
    for path in archives {
        let source = std::fs::read(&path).unwrap();
        if !source.starts_with(b"mha\x01") {
            continue;
        }
        let archive = MhaArchive::parse(&source, 65_536).unwrap();
        empty_slots += archive
            .entries
            .iter()
            .filter(|item| item.entry.size == 0)
            .count();
        let Some(first) = archive.entries.iter().find(|item| item.entry.size != 0) else {
            continue;
        };
        let replacement = extended(first.entry.payload(&source).unwrap(), 513);
        assert_mha_replacement(&archive, first.entry.index, &replacement);
        checked += 1;

        if path.file_name().is_some_and(|name| name == "wi500.abn") {
            let member = archive
                .entries
                .iter()
                .find(|item| item.name == b"wi521.bin")
                .unwrap();
            let mut replacement = member.entry.payload(&source).unwrap().to_vec();
            replacement.resize(23_692, 0xa5);
            assert_mha_replacement(&archive, member.entry.index, &replacement);
            wi521_checked = true;
        }
    }
    assert!(checked > 0 && empty_slots > 0 && wi521_checked);
    eprintln!("MHA native replacements: {checked} nonempty archives, {empty_slots} empty slots");
}

fn assert_aligned_directory(
    source: &[u8],
    archive: &SimpleArchive<'_>,
    boundary: usize,
    align_tail: bool,
) {
    let mut end = archive.table_offset + archive.entries.len() * 8;
    for entry in archive.entries.iter().filter(|entry| entry.size != 0) {
        let start = entry.offset as usize;
        assert_eq!(start, aligned(end, boundary));
        assert!(source[end..start].iter().all(|byte| *byte == 0));
        end = start + entry.size as usize;
    }
    assert_eq!(
        source.len(),
        if align_tail {
            aligned(end, boundary)
        } else {
            end
        }
    );
    assert!(source[end..].iter().all(|byte| *byte == 0));
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original MOMO sound only"]
fn original_momo_replacement_rebuilds_member_and_file_tail_padding() {
    let mut source = decoded(&std::fs::read(game_data().join("sound/grdn_fes.snd")).unwrap());
    for selected in [0, 2] {
        let old = SimpleArchive::parse(&source, 65_536).unwrap();
        assert_aligned_directory(&source, &old, 64, true);
        let target = old.entries[selected];
        let replacement = extended(target.payload(&source).unwrap(), 65);
        let output = repack::replace(Kind::Momo, &source, range(target), &replacement).unwrap();
        let new = SimpleArchive::parse(&output, 65_536).unwrap();
        assert_eq!(old.kind, new.kind);
        assert_member_bytes(
            &source,
            old.entries.iter().copied(),
            &output,
            new.entries.iter().copied(),
            target,
            &replacement,
        );
        assert_aligned_directory(&output, &new, 64, true);
        source = output;
    }
}

fn original_stage() -> Vec<u8> {
    let source = decoded(&std::fs::read(game_data().join("stage/st002.pac")).unwrap());
    let archive = SimpleArchive::parse(&source, 65_536).unwrap();
    decoded(archive.payload(31).unwrap())
}

fn stage_gaps(source: &[u8], archive: &StageArchive<'_>) -> Vec<usize> {
    let mut end = 28 + archive.additional_count as usize * 12;
    let mut extra = Vec::new();
    for member in &archive.entries {
        if member.entry.size == 0 {
            continue;
        }
        let start = member.entry.offset as usize;
        assert_eq!(start % 16, 0);
        assert!(source[end..start].iter().all(|byte| *byte == 0));
        extra.push(start - aligned(end, 16));
        end = start + member.entry.size as usize;
    }
    assert!(source[end..].iter().all(|byte| *byte == 0));
    // The Stage container has an independent tail, not a file-size alignment.
    extra.push(source.len() - end);
    extra
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original Stage archive only"]
fn original_stage_replacement_preserves_empty_slots_ids_and_extra_gap_blocks() {
    let mut source = original_stage();
    StageArchive::probe(&source, 65_536).unwrap();
    for selected in [0, 4] {
        let old = StageArchive::parse(&source, 65_536).unwrap();
        let old_gaps = stage_gaps(&source, &old);
        let target = old.entries[selected].entry;
        let replacement = extended(target.payload(&source).unwrap(), 17);
        let output = repack::replace(Kind::Stage, &source, range(target), &replacement).unwrap();
        // Member 0 deliberately contains opaque extra bytes in this test, so
        // inspect the container structure without probing the placement table.
        let new = StageArchive::parse(&output, 65_536).unwrap();
        assert_eq!(old.additional_count, new.additional_count);
        assert_member_bytes(
            &source,
            old.entries.iter().map(|member| member.entry),
            &output,
            new.entries.iter().map(|member| member.entry),
            target,
            &replacement,
        );
        for (old, new) in old.entries.iter().zip(&new.entries) {
            assert_eq!(old.resource_id, new.resource_id);
            if old.entry.size == 0 {
                assert_eq!(old.entry, new.entry);
            }
        }
        assert_eq!(old_gaps, stage_gaps(&output, &new));
        source = output;
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original Stage object packages only"]
fn original_sibling_objects_keep_their_distinct_tight_and_aligned_layouts() {
    let stage = original_stage();
    let archive = StageArchive::probe(&stage, 65_536).unwrap();
    for (selected, boundary) in [(3, 1), (6, 16)] {
        let mut source = archive.entries[selected]
            .entry
            .payload(&stage)
            .unwrap()
            .to_vec();
        let last = SimpleArchive::parse(&source, 65_536).unwrap().entries.len() - 1;
        for selected in [1, last] {
            let old = SimpleArchive::parse(&source, 65_536).unwrap();
            assert_aligned_directory(&source, &old, boundary, false);
            let target = old.entries[selected];
            let replacement = extended(target.payload(&source).unwrap(), 17);
            let output = repack::replace(
                Kind::StageObjectPackage,
                &source,
                range(target),
                &replacement,
            )
            .unwrap();
            let new = SimpleArchive::parse(&output, 65_536).unwrap();
            assert_member_bytes(
                &source,
                old.entries.iter().copied(),
                &output,
                new.entries.iter().copied(),
                target,
                &replacement,
            );
            assert_aligned_directory(&output, &new, boundary, false);
            source = output;
        }
    }
}
