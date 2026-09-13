use super::*;
use crate::{
    edit,
    field::{Field, FieldType, ScalarType},
    inspect::{self, Document},
};
use mhf_resource::{container::SimpleArchive, crypto::Ecd, jkr::Jkr};

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn sample() -> Vec<u8> {
    let mut bytes = vec![0xa5; 640];
    bytes[..84].fill(0);
    for (index, subtype) in [0u16, 1].into_iter().enumerate() {
        let at = index * 28;
        bytes[at..at + 2].copy_from_slice(&subtype.to_le_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&(index as u16 * 6).to_le_bytes());
        bytes[at + 4..at + 6].copy_from_slice(&2u16.to_le_bytes());
        bytes[at + 6..at + 8].copy_from_slice(&1u16.to_le_bytes());
        word(&mut bytes, at + 8, 128);
        word(&mut bytes, at + 12, 256);
        word(&mut bytes, at + 16, 96);
        word(&mut bytes, at + 20, 320);
        word(&mut bytes, at + 24, 2);
    }
    bytes[58..60].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[128..208].fill(0);
    bytes[132..134].copy_from_slice(&58u16.to_le_bytes());
    bytes[172..174].copy_from_slice(&46u16.to_le_bytes());
    bytes[256..288].fill(0);
    bytes[320..384].fill(0);
    for slot in 0..8 {
        word(&mut bytes, 96 + slot * 4, if slot == 1 { 496 } else { 416 });
    }
    bytes[416..456].fill(0);
    bytes[456..458].fill(0xff);
    bytes[496..498].fill(0xff);
    bytes
}

fn field_at(document: &Document, node: usize, range: std::ops::Range<usize>) -> &Field {
    document.nodes[node]
        .fields
        .iter()
        .find(|field| field.binding.range == range)
        .unwrap()
}

fn child(document: &Document, node: usize, kind: Kind) -> usize {
    *document.nodes[node]
        .children
        .iter()
        .find(|&&index| document.nodes[index].kind == kind)
        .unwrap()
}

fn archive(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 12];
    word(&mut bytes, 0, 1);
    word(&mut bytes, 4, 12);
    word(&mut bytes, 8, payload.len() as u32);
    bytes.extend_from_slice(payload);
    bytes
}

fn encoded(payload: &[u8]) -> Vec<u8> {
    let mut jkr = b"JKR\x1a\x08\x01\0\0".to_vec();
    jkr.extend_from_slice(&16u32.to_le_bytes());
    jkr.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    jkr.extend_from_slice(payload);
    Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
        .unwrap()
        .encode(&jkr, Some(b"collection.bin"))
        .unwrap()
}

fn expand_record(document: Document, entry_index: usize, kind: Kind) -> (Document, usize) {
    let root = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Sdt)
        .unwrap();
    let entry = child(&document, root, Kind::SdtEntry(entry_index));
    let document = inspect::expand(&document, entry).unwrap();
    let table = child(&document, entry, kind);
    let document = inspect::expand(&document, table).unwrap();
    let record = document.nodes[table].children[0];
    let document = inspect::expand(&document, record).unwrap();
    (document, record)
}

#[test]
fn deferred_sdt_records_bind_nonzero_container_offsets_and_preserve_every_byte() {
    let source = sample();
    let document = inspect::inspect("outer.bin", archive(&source).into());
    let root = child(&document, document.root, Kind::Sdt);
    assert_eq!(document.nodes[root].range.start, 12);
    assert!(document.nodes[root].error.is_none());
    let entry = child(&document, root, Kind::SdtEntry(0));
    assert!(document.nodes[entry].deferred);
    assert!(document.nodes[entry].children.is_empty());
    assert_eq!(
        field_at(&document, entry, 20..24)
            .read(&document.buffers)
            .unwrap(),
        "128"
    );
    let document = inspect::expand(&document, entry).unwrap();
    let tables = [
        Kind::SdtAttackTable,
        Kind::SdtAuxiliaryTable,
        Kind::SdtExtraTable,
    ]
    .map(|kind| child(&document, entry, kind));
    for &table in &tables {
        assert!(document.nodes[table].deferred);
        assert!(document.nodes[table].children.is_empty());
    }
    let auxiliary = &document.nodes[tables[1]];
    assert!(
        auxiliary
            .fields
            .iter()
            .any(|field| field.name == "原生索引上限")
    );
    assert!(
        auxiliary
            .fields
            .iter()
            .any(|field| field.value.contains("可能共享"))
    );
    let mut document = document;
    for table in tables {
        document = inspect::expand(&document, table).unwrap();
        let records = document.nodes[table].children.clone();
        assert_eq!(records.len(), 2);
        for record in records {
            assert!(document.nodes[record].deferred);
            document = inspect::expand(&document, record).unwrap();
            let current = &document.nodes[record];
            let mut covered = vec![false; current.range.len()];
            for field in &current.fields {
                if field.binding.range.is_empty() {
                    continue;
                }
                assert!(field.writable);
                assert_eq!(field.binding.buffer, current.buffer);
                covered[field.binding.range.start - current.range.start
                    ..field.binding.range.end - current.range.start]
                    .fill(true);
            }
            assert!(covered.iter().all(|&covered| covered), "{:?}", current.kind);
        }
    }
    let attack = document.nodes[tables[0]].children[0];
    let value = field_at(&document, attack, 144..146);
    assert_eq!(value.binding.format, FieldType::Scalar(ScalarType::U16));
    assert_eq!(value.read(&document.buffers).unwrap(), "58");
    let raw = child(&document, root, Kind::SdtUnclaimed);
    assert!(document.nodes[raw].deferred);
    let document = inspect::expand(&document, raw).unwrap();
    assert!(!document.nodes[raw].children.is_empty());
    assert!(
        document.nodes[raw]
            .children
            .iter()
            .all(|&node| !document.nodes[node].range.is_empty())
    );
    assert_eq!(document.bytes(root).unwrap(), source);
}

#[test]
fn sdt_alias_edits_rebuild_both_encoding_layers_and_keep_expanded_views() {
    let source = sample();
    let document = inspect::inspect("collection.bin", encoded(&archive(&source)).into());
    let (document, first) = expand_record(document, 0, Kind::SdtAttackTable);
    let (document, alias) = expand_record(document, 1, Kind::SdtAttackTable);
    let first_key = edit::node_key(&document, first).unwrap();
    let alias_key = edit::node_key(&document, alias).unwrap();
    let field = field_at(&document, first, 144..146);
    assert_eq!(
        field.binding.range,
        field_at(&document, alias, 144..146).binding.range
    );
    let patch = field.write(&document.buffers, "99").unwrap().unwrap();
    let updated = edit::apply_many(&document, &[patch]).unwrap();
    let mut expected = source.clone();
    expected[132..134].copy_from_slice(&99u16.to_le_bytes());
    for key in [&first_key, &alias_key] {
        let record = edit::locate(&updated, key).unwrap();
        assert!(!updated.nodes[record].deferred);
        assert_eq!(
            field_at(&updated, record, 144..146)
                .read(&updated.buffers)
                .unwrap(),
            "99"
        );
        assert!(edit::replace(&updated, record, &[0; 41]).is_err());
    }
    let encrypted = Ecd::parse(&updated.buffers[0]).unwrap();
    encrypted.validate_filename(b"collection.bin").unwrap();
    let compressed = encrypted.decode(2048).unwrap();
    let directory = Jkr::parse(&compressed).unwrap().decode(2048).unwrap();
    assert_eq!(
        SimpleArchive::parse(&directory, 1)
            .unwrap()
            .payload(0)
            .unwrap(),
        expected
    );
}

#[test]
fn hitbox_slots_preserve_aliases_empty_sentinels_and_local_pointer_errors() {
    let mut source = sample();
    word(&mut source, 104, 0);
    let document = inspect::inspect("mhfsdt.bin", source.into());
    let entry = child(&document, document.root, Kind::SdtEntry(0));
    let document = inspect::expand(&document, entry).unwrap();
    let groups = child(&document, entry, Kind::SdtCollisionGroups);
    assert!(document.nodes[groups].deferred);
    let document = inspect::expand(&document, groups).unwrap();
    let group = child(&document, groups, Kind::SdtCollisionGroup(0));
    assert_eq!(document.nodes[group].range, 96..128);
    let mut document = inspect::expand(&document, group).unwrap();
    assert_eq!(document.nodes[group].children.len(), 8);
    let empty = child(&document, group, Kind::SdtCollisionList(1));
    assert!(!document.nodes[empty].deferred);
    assert!(document.nodes[empty].range.is_empty());
    assert_eq!(
        field_at(&document, empty, 496..498)
            .read(&document.buffers)
            .unwrap(),
        "FF FF"
    );
    let invalid = child(&document, group, Kind::SdtCollisionList(2));
    assert!(document.nodes[invalid].error.is_some());
    assert!(field_at(&document, invalid, 104..108).writable);
    assert!(document.nodes[group].error.is_none());
    let mut records = Vec::new();
    for slot in [0, 3] {
        let list = child(&document, group, Kind::SdtCollisionList(slot));
        assert_eq!(
            field_at(&document, list, 456..458)
                .read(&document.buffers)
                .unwrap(),
            "FF FF"
        );
        document = inspect::expand(&document, list).unwrap();
        assert_eq!(document.nodes[list].children.len(), 1);
        let record = document.nodes[list].children[0];
        assert_eq!(document.nodes[record].range, 416..456);
        document = inspect::expand(&document, record).unwrap();
        assert!(!document.nodes[record].fields.is_empty());
        records.push(record);
    }
    let keys = records
        .iter()
        .map(|&node| edit::node_key(&document, node).unwrap())
        .collect::<Vec<_>>();
    let buffer = document.nodes[records[0]].buffer;
    let updated = edit::apply(&document, buffer, 440..444, &1.0f32.to_le_bytes()).unwrap();
    for key in &keys {
        let record = edit::locate(&updated, key).unwrap();
        assert_eq!(
            &updated.bytes(record).unwrap()[24..28],
            &1.0f32.to_le_bytes()
        );
        assert!(!updated.nodes[record].deferred);
    }
    let capsule = edit::apply(&updated, buffer, 418..420, &1u16.to_le_bytes()).unwrap();
    for key in &keys {
        let record = edit::locate(&capsule, key).unwrap();
        let endpoint = field_at(&capsule, record, 444..448);
        assert_eq!(endpoint.binding.format, FieldType::Scalar(ScalarType::F32));
        assert_eq!(endpoint.name, "局部终点 X");
    }
}

#[test]
fn named_sdt_keeps_local_invalid_table_diagnostics_and_anonymous_data_is_not_claimed() {
    let mut source = sample();
    word(&mut source, 12, u32::MAX);
    let document = inspect::inspect("MHfSDT.BIN", source.into());
    assert_eq!(document.nodes[document.root].kind, Kind::Sdt);
    assert!(document.nodes[document.root].error.is_none());
    let entry = child(&document, document.root, Kind::SdtEntry(0));
    let document = inspect::expand(&document, entry).unwrap();
    let auxiliary = child(&document, entry, Kind::SdtAuxiliaryTable);
    assert!(document.nodes[auxiliary].error.is_some());
    assert!(field_at(&document, auxiliary, 12..16).writable);
    assert!(document.nodes[child(&document, entry, Kind::SdtAttackTable)].deferred);
    let mut fake = vec![0; 84];
    fake[58..60].copy_from_slice(&u16::MAX.to_le_bytes());
    for bytes in [vec![0; 64], fake, sample()[..40].to_vec()] {
        let document = inspect::inspect("unknown.bin", bytes.into());
        assert!(!document.nodes.iter().any(|node| node.kind == Kind::Sdt));
    }
    let document = inspect::inspect("mhfsdt.bin", vec![0; 64].into());
    assert_eq!(document.nodes[document.root].kind, Kind::Sdt);
    assert!(document.nodes[document.root].error.is_some());
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; edits only an in-memory copy of original mhfsdt.bin"]
fn original_sdt_edit_round_trips_encoded_file_and_preserves_other_payload_bytes() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfsdt.bin")).unwrap();
    let decoded = mhf_resource::container::open_layers(&source, 128 * 1024 * 1024, 8).unwrap();
    let file = Sdt::probe(decoded.payload()).unwrap();
    let entry = file
        .entries()
        .iter()
        .find(|entry| entry.record_count > 1)
        .unwrap();
    let table = file.table(entry, TableKind::Attack).unwrap().unwrap();
    let offset = table.record(0).unwrap().offset + 4;
    let mut expected = decoded.payload().to_vec();
    let old = u16::from_le_bytes(expected[offset..offset + 2].try_into().unwrap());
    let value = old.wrapping_add(1);
    expected[offset..offset + 2].copy_from_slice(&value.to_le_bytes());

    let document = inspect::inspect("mhfsdt.bin", source.clone().into());
    let (document, record) = expand_record(document, entry.index, Kind::SdtAttackTable);
    let at = document.nodes[record].range.start + 4;
    let patch = field_at(&document, record, at..at + 2)
        .write(&document.buffers, &value.to_string())
        .unwrap()
        .unwrap();
    let updated = edit::apply_many(&document, &[patch]).unwrap();
    let repacked =
        mhf_resource::container::open_layers(&updated.buffers[0], 128 * 1024 * 1024, 8).unwrap();
    assert_eq!(repacked.payload(), expected);
    assert_eq!(repacked.layers.len(), decoded.layers.len());
    assert_eq!(&*document.buffers[0], source);
    assert!(updated.nodes.iter().all(|node| node.error.is_none()));
    println!(
        "SDT original envelope round-trip: one u16 changed at decoded {offset:#x}; other payload bytes unchanged"
    );
}
