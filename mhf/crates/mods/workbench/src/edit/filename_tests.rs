use mhf_resource::{
    container::{MhaArchive, SimpleArchive},
    crypto::{Ecd, Exf},
};

use crate::inspect::{self, Kind};

fn encoded(payload: &[u8], filename: &[u8]) -> Vec<u8> {
    Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
        .unwrap()
        .encode(payload, Some(filename))
        .unwrap()
}

fn exf(payload: &[u8], filename: &[u8]) -> Vec<u8> {
    Exf::parse(b"exf\x1a\x04\0\0\0\xaa\xbb\xcc\xdd\x12\x34\x56\x78")
        .unwrap()
        .encode(payload, Some(filename))
        .unwrap()
}

fn word(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn named(name: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 24];
    bytes[..4].copy_from_slice(b"mha\x01");
    bytes.extend_from_slice(payload);
    let names = bytes.len();
    bytes.extend_from_slice(name);
    bytes.push(0);
    let table = bytes.len();
    bytes.resize(table + 20, 0);
    for (at, value) in [
        (4, table),
        (8, 1),
        (12, names),
        (16, name.len() + 1),
        (20, 1 << 16),
        (table + 4, 24),
        (table + 8, payload.len()),
        (table + 12, payload.len()),
    ] {
        word(&mut bytes, at, value);
    }
    bytes
}

#[test]
fn exf_whole_replacement_uses_the_destination_name() {
    let source = named(b"target.mus", &exf(b"old audio", b"target.mus"));
    let document = inspect::inspect("audio.abn", source.into());
    let wrapper = document.nodes[document.root].children[0];
    let donor = exf(b"new audio", b"donor.mus");
    let updated = super::replace(&document, wrapper, &donor).unwrap();
    let archive = MhaArchive::parse(&updated.buffers[0], 1).unwrap();
    let bytes = archive.entries[0]
        .entry
        .payload(&updated.buffers[0])
        .unwrap();
    let file = Exf::parse(bytes).unwrap();
    file.validate_filename(b"target.mus").unwrap();
    assert_eq!(&bytes[8..], &donor[8..]);
    assert_eq!(&**file.decode(100).unwrap(), b"new audio");
}

#[test]
fn nested_exf_and_ecd_header_repairs_share_the_existing_rebuild_pipeline() {
    let mut inner = encoded(b"payload", b"nested.bin");
    inner[6..8].fill(0);
    let mut middle = exf(&inner, b"nested.bin");
    middle[6..8].fill(0);
    let mut outer = encoded(&middle, b"nested.bin");
    outer[6..8].fill(0);
    let document = inspect::inspect("nested.bin", outer.into());
    let updated = super::apply_many(&document, &[]).unwrap();
    let outer = Ecd::parse(&updated.buffers[0]).unwrap();
    outer.validate_filename(b"nested.bin").unwrap();
    let middle = outer.decode(1000).unwrap();
    let middle = Exf::parse(&middle).unwrap();
    middle.validate_filename(b"nested.bin").unwrap();
    let inner = middle.decode(1000).unwrap();
    let inner = Ecd::parse(&inner).unwrap();
    inner.validate_filename(b"nested.bin").unwrap();
    assert_eq!(&**inner.decode(100).unwrap(), b"payload");
}

#[test]
fn decoded_edits_use_the_raw_named_member_instead_of_its_display_label() {
    let source = named(b"wi521.bin", &encoded(b"old payload", b"wi521.bin"));
    let mut document = inspect::inspect("wi500.abn", source.into());
    let wrapper = document.nodes[document.root].children[0];
    assert_eq!(document.nodes[wrapper].kind, Kind::Ecd);
    document.nodes[wrapper].name = "0021 · misleading-display.bin".into();
    let leaf = document.nodes[wrapper].children[0];
    let node = &document.nodes[leaf];
    let updated = super::apply(&document, node.buffer, 0..3, b"new").unwrap();
    let archive = MhaArchive::parse(&updated.buffers[0], 1).unwrap();
    let file = Ecd::parse(
        archive.entries[0]
            .entry
            .payload(&updated.buffers[0])
            .unwrap(),
    )
    .unwrap();
    file.validate_filename(b"wi521.bin").unwrap();
    assert_eq!(&**file.decode(100).unwrap(), b"new payload");
}

#[test]
fn whole_encoded_replacement_rebinds_to_the_destination_member() {
    let source = named(b"wi521.bin", &encoded(b"original", b"wi521.bin"));
    let document = inspect::inspect("wi500.abn", source.into());
    let wrapper = document.nodes[document.root].children[0];
    for payload in [&b"same len"[..], b"a longer replacement"] {
        let donor = encoded(payload, b"wi522.bin");
        let updated = super::replace(&document, wrapper, &donor).unwrap();
        let archive = MhaArchive::parse(&updated.buffers[0], 1).unwrap();
        let bytes = archive.entries[0]
            .entry
            .payload(&updated.buffers[0])
            .unwrap();
        let file = Ecd::parse(bytes).unwrap();
        file.validate_filename(b"wi521.bin").unwrap();
        assert!(file.validate_filename(b"wi522.bin").is_err());
        assert_eq!(&bytes[8..], &donor[8..]);
        assert_eq!(&**file.decode(100).unwrap(), payload);
    }
}

#[test]
fn packing_without_field_changes_repairs_a_stale_name_checksum_only() {
    let mut source = encoded(b"replacement", b"wi521.bin");
    source[6..8].copy_from_slice(&0xac10u16.to_le_bytes());
    assert!(
        Ecd::parse(&source)
            .unwrap()
            .validate_filename(b"wi521.bin")
            .is_err()
    );
    let document = inspect::inspect("dat/weapon/wi521.bin", source.clone().into());
    let updated = super::apply_many(&document, &[]).unwrap();
    let bytes = &updated.buffers[0];
    assert_eq!(&bytes[..6], &source[..6]);
    assert_eq!(&bytes[8..], &source[8..]);
    Ecd::parse(bytes)
        .unwrap()
        .validate_filename(b"wi521.bin")
        .unwrap();
}

#[test]
fn renamed_member_rechecks_its_unchanged_ecd_payload() {
    let source = named(b"old.bin", &encoded(b"payload", b"old.bin"));
    let names = MhaArchive::parse(&source, 1).unwrap().header.names_offset as usize;
    let document = inspect::inspect("archive.abn", source.into());
    let updated = super::apply(&document, 0, names..names + 7, b"new.bin").unwrap();
    let archive = MhaArchive::parse(&updated.buffers[0], 1).unwrap();
    assert_eq!(archive.entries[0].name, b"new.bin");
    Ecd::parse(
        archive.entries[0]
            .entry
            .payload(&updated.buffers[0])
            .unwrap(),
    )
    .unwrap()
    .validate_filename(b"new.bin")
    .unwrap();
}

#[test]
fn anonymous_members_do_not_inherit_the_parent_containers_filename() {
    let member = encoded(b"old", b"unknown.bin");
    let mut source = vec![0; 12];
    word(&mut source, 0, 1);
    word(&mut source, 4, 12);
    word(&mut source, 8, member.len());
    source.extend_from_slice(&member);
    SimpleArchive::parse(&source, 1).unwrap();
    let document = inspect::inspect("parent.bin", source.into());
    let wrapper = document.nodes[document.root].children[0];
    let leaf = document.nodes[wrapper].children[0];
    let error = super::apply(&document, document.nodes[leaf].buffer, 0..3, b"new").unwrap_err();
    assert!(error.contains("destination filename"), "{error}");
}

#[test]
fn packing_rejects_invalid_id_ranges_but_the_header_remains_editable() {
    let mut source = named(b"entry.bin", b"payload");
    source[22..24].fill(0);
    let document = inspect::inspect("archive.abn", source.into());
    assert!(
        super::prepare_pack(&document, &[])
            .unwrap_err()
            .contains("资源 ID 索引")
    );
    let corrected = super::apply(&document, 0, 22..24, &1u16.to_le_bytes()).unwrap();
    super::prepare_pack(&corrected, &[]).unwrap();
}

#[test]
fn truncated_encoded_siblings_do_not_block_local_edits_but_still_block_packing() {
    for (kind, damaged) in [
        (Kind::Ecd, b"ecd\x1a\x04\0\0\0"),
        (Kind::Exf, b"exf\x1a\x04\0\0\0"),
        (Kind::Jkr, b"JKR\x1a\x08\x01\0\0"),
    ] {
        let mut source = vec![0; 20];
        for (at, value) in [(0, 2), (4, 20), (8, 3), (12, 23), (16, damaged.len())] {
            word(&mut source, at, value);
        }
        source.extend_from_slice(b"old");
        source.extend_from_slice(damaged);
        let document = inspect::inspect("siblings.bin", source.into());
        assert_eq!(document.nodes[document.root].kind, Kind::Archive);
        let children = &document.nodes[document.root].children;
        let healthy = children[0];
        let broken = children[1];
        assert_eq!(document.nodes[broken].kind, kind);
        let original_error = document.nodes[broken].error.as_ref().unwrap();
        assert!(original_error.contains("truncated"), "{original_error}");
        assert!(document.nodes[broken].children.is_empty());

        // Equal-length edits and a replacement that moves the damaged sibling
        // must both retain the opaque envelope and its diagnostic.
        for replacement in [&b"new"[..], &b"a longer sibling"[..]] {
            let updated = super::replace(&document, healthy, replacement).unwrap();
            let archive = SimpleArchive::parse(&updated.buffers[0], 2).unwrap();
            assert_eq!(archive.payload(0).unwrap(), replacement);
            assert_eq!(archive.payload(1).unwrap(), damaged);
            let broken = updated.nodes[updated.root].children[1];
            assert_eq!(updated.nodes[broken].kind, kind);
            assert_eq!(updated.nodes[broken].error.as_ref(), Some(original_error));
            assert_eq!(updated.bytes(broken).unwrap(), damaged);
            assert!(updated.nodes[broken].children.is_empty());
            let error = super::prepare_pack(&updated, &[]).unwrap_err();
            assert!(error.contains(original_error), "{error}");
        }
        assert_eq!(document.bytes(healthy).unwrap(), b"old");
        assert_eq!(document.bytes(broken).unwrap(), damaged);
    }
}

#[test]
fn packing_does_not_mask_an_invalid_ecd_payload_crc() {
    let mut source = encoded(b"payload", b"file.bin");
    source[16] ^= 1;
    let document = inspect::inspect("file.bin", source.into());
    let error = super::prepare_pack(&document, &[]).unwrap_err();
    assert!(error.contains("ECD CRC32 mismatch"), "{error}");
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; validates filename bindings and a whole weapon replacement"]
fn original_named_ecd_bindings_and_whole_weapon_replacement() {
    let root =
        std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap()).join("dat");
    let mut folders = vec![root.clone()];
    let mut checked = 0;
    while let Some(folder) = folders.pop() {
        for item in std::fs::read_dir(folder).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "abn") {
                continue;
            }
            let bytes = std::fs::read(path).unwrap();
            if !bytes.starts_with(b"mha\x01") {
                continue;
            }
            let archive = MhaArchive::parse(&bytes, 65536).unwrap();
            for member in archive.entries {
                let payload = member.entry.payload(&bytes).unwrap();
                if payload.starts_with(b"ecd\x1a") {
                    let file = Ecd::parse(payload).unwrap();
                    if file.header.key_index >= 4 {
                        file.validate_filename(member.name).unwrap();
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 0);
    let source = std::fs::read(root.join("extend/wi500.abn")).unwrap();
    let archive = MhaArchive::parse(&source, 1000).unwrap();
    let target = archive
        .entries
        .iter()
        .find(|entry| entry.name == b"wi521.bin")
        .unwrap()
        .entry
        .index;
    let donor = archive
        .entries
        .iter()
        .find(|entry| entry.name == b"wi522.bin")
        .unwrap()
        .entry
        .payload(&source)
        .unwrap()
        .to_vec();
    let document = inspect::inspect("wi500.abn", source.into());
    let node = document.nodes[document.root].children[target];
    let updated = super::replace(&document, node, &donor).unwrap();
    let archive = MhaArchive::parse(&updated.buffers[0], 1000).unwrap();
    let file = Ecd::parse(
        archive.entries[target]
            .entry
            .payload(&updated.buffers[0])
            .unwrap(),
    )
    .unwrap();
    file.validate_filename(b"wi521.bin").unwrap();
    assert_eq!(
        &**file.decode(1000000).unwrap(),
        &**Ecd::parse(&donor).unwrap().decode(1000000).unwrap()
    );
    eprintln!("Validated {checked} original named ECD resources and whole wi521 replacement");
}
