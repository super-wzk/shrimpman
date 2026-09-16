use super::*;
use crate::field::{Binding, ScalarType};
use mhf_resource::{
    container::{MhaArchive, SimpleArchive, open_layers},
    crypto::{Ecd, Exf},
    dat,
    fmod::Fmod,
    jkr::Jkr,
};

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn fmod_block(kind: u32, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [
        kind,
        count,
        (payload.len() + mhf_resource::fmod::HEADER_SIZE) as u32,
    ] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

fn jkr(encoding: u16, size: usize, payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"JKR\x1a\x08\x01".to_vec();
    bytes.extend_from_slice(&encoding.to_le_bytes());
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&(size as u32).to_le_bytes());
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
    bytes.extend_from_slice(payload);
    bytes
}

fn ecd(payload: &[u8]) -> Vec<u8> {
    Ecd::parse(b"ecd\x1a\x03\0\xaa\xbb\0\0\0\0\0\0\0\0trailer")
        .unwrap()
        .encode(payload, None)
        .unwrap()
}

fn exf(payload: &[u8]) -> Vec<u8> {
    Exf::parse(b"exf\x1a\x04\0\xaa\xbb\x11\x22\x33\x44\x55\x66\x77\x88")
        .unwrap()
        .encode(payload, None)
        .unwrap()
}

fn archive(members: &[&[u8]], momo: bool) -> Vec<u8> {
    let prefix = if momo { 4 } else { 0 };
    let mut bytes = vec![0; prefix + 4 + members.len() * 8];
    if momo {
        bytes[..4].copy_from_slice(b"MOMO");
    }
    word(&mut bytes, prefix, members.len() as u32);
    for (index, member) in members.iter().enumerate() {
        let offset = bytes.len() as u32;
        word(&mut bytes, prefix + 4 + index * 8, offset);
        word(&mut bytes, prefix + 8 + index * 8, member.len() as u32);
        bytes.extend_from_slice(member);
    }
    bytes
}

fn mha(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 64];
    bytes[..4].copy_from_slice(b"mha\x01");
    for (at, value) in [
        (4, 24),
        (8, 1),
        (12, 44),
        (16, 11),
        (20, 739 | (1 << 16)),
        (28, 64),
        (32, payload.len() as u32),
        (36, payload.len() as u32 + 3),
        (40, 739),
    ] {
        word(&mut bytes, at, value);
    }
    bytes[44..55].copy_from_slice(b"member.bin\0");
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&[0xde, 0xad, 0xef]);
    bytes
}

#[test]
fn ecd_encoding_matches_independent_vectors_and_preserves_metadata() {
    for (key, hex) in [
        "f577b418203c37b628",
        "deb25b2b8fdaea8924",
        "deb25b2b8fdaea8924",
        "deb25b2b8fdaea8924",
        "d66a904d4dd823f293",
        "a2c4fa3dd82ff546ef",
    ]
    .into_iter()
    .enumerate()
    {
        let template = ecd(b"");
        let mut file = Ecd::parse(&template).unwrap();
        file.header.key_index = key as u16;
        let encoded = file.encode(b"123456789", Some(b"test.bin")).unwrap();
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect();
        assert_eq!(&encoded[16..25], expected);
        let decoded = Ecd::parse(&encoded).unwrap().decode(9).unwrap();
        assert_eq!(&**decoded, b"123456789");
        if key < 4 {
            assert_eq!(decoded.encoding.header.filename_checksum, 0xbbaa);
        } else {
            decoded.encoding.validate_filename(b"test.bin").unwrap();
        }
        assert_eq!(decoded.encoding.trailing_bytes().unwrap(), b"trailer");
    }
}

#[test]
fn exf_encoding_covers_every_byte_and_key_without_changing_header() {
    let payload: Vec<u8> = (0..=255).cycle().take(1025).collect();
    for key in 0..5 {
        let template = exf(b"");
        let mut file = Exf::parse(&template).unwrap();
        file.header.key_index = key;
        let encoded = file.encode(&payload, None).unwrap();
        let decoded = Exf::parse(&encoded).unwrap().decode(payload.len()).unwrap();
        assert_eq!(&**decoded, payload);
        assert_eq!(decoded.encoding.header, file.header);
    }
}

#[test]
fn edits_rebuild_nested_envelopes_and_relocate_momo_and_mha_members() {
    let compressed = jkr(3, 27, &[0x70, b'A', 0, 0, 0]);
    let directory = archive(&[&compressed, b"unchanged sibling"], true);
    let named = mha(&directory);
    let source = ecd(&exf(&jkr(0, named.len(), &named)));
    let document = inspect::inspect("nested.bin", source.clone().into());
    let leaf = (0..document.nodes.len())
        .find(|&node| document.bytes(node) == Some(&[b'A'; 27]))
        .unwrap();
    let key = node_key(&document, leaf).unwrap();
    let node = &document.nodes[leaf];
    let updated = apply(&document, node.buffer, 0..1, b"B").unwrap();
    let selected = locate(&updated, &key).unwrap();
    let mut expected = [b'A'; 27];
    expected[0] = b'B';
    assert_eq!(updated.bytes(selected).unwrap(), expected);
    assert_eq!(document.bytes(leaf).unwrap(), [b'A'; 27]);
    assert!(updated.nodes.iter().all(|node| node.error.is_none()));
    let decoded = open_layers(&updated.buffers[0], usize::MAX, 10).unwrap();
    let named = MhaArchive::parse(&decoded, 10).unwrap();
    assert_eq!(named.header.first_file_id, 739);
    assert_eq!(named.entries[0].file_id, 739);
    assert_eq!(named.entries[0].name, b"member.bin");
    assert_eq!(
        named.entries[0].padded_size,
        named.entries[0].entry.size + 3
    );
    assert_eq!(&decoded[decoded.len() - 3..], &[0xde, 0xad, 0xef]);
    let directory =
        SimpleArchive::parse(named.entries[0].entry.payload(&decoded).unwrap(), 10).unwrap();
    assert_eq!(directory.payload(1).unwrap(), b"unchanged sibling");
    let leaf = Jkr::parse(directory.payload(0).unwrap()).unwrap();
    assert_eq!(leaf.header.encoding, 0);
    assert_eq!(leaf.header_extension().unwrap(), &[0xaa, 0xbb, 0xcc, 0xdd]);
    assert_eq!(&**leaf.decode(27).unwrap(), expected);
}

#[test]
fn stage_object_members_relocate_when_a_decoded_edit_grows_the_jkr_envelope() {
    let mut model = vec![0; 44];
    word(&mut model, 0, 1);
    word(&mut model, 8, 12);
    // Twelve literal header bytes followed by a 32-byte zero back-reference.
    let compressed = jkr(
        3,
        model.len(),
        &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0x0e, 12, 0, 0, 0, 0, 0, 6],
    );
    assert_eq!(
        &**Jkr::parse(&compressed).unwrap().decode(44).unwrap(),
        model
    );
    let descriptor = [1, 0, 2, 0, 1, 255];
    let source = archive(&[&descriptor, &compressed, b"untouched"], false);
    let original_sibling = SimpleArchive::parse(&source, 3).unwrap().entries[2].offset;
    let document = inspect::inspect("objects.bin", source.into());
    assert_eq!(document.nodes[document.root].kind, Kind::StageObjectPackage);
    let leaf = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Fmod)
        .unwrap();
    let key = node_key(&document, leaf).unwrap();
    let node = &document.nodes[leaf];
    let updated = apply(&document, node.buffer, 43..44, &[7]).unwrap();
    assert_eq!(updated.nodes[updated.root].kind, Kind::StageObjectPackage);
    assert!(updated.nodes.iter().all(|node| node.error.is_none()));
    assert_eq!(document.bytes(leaf).unwrap(), model);
    model[43] = 7;
    assert_eq!(
        updated.bytes(locate(&updated, &key).unwrap()).unwrap(),
        model
    );
    let directory = SimpleArchive::parse(&updated.buffers[0], 3).unwrap();
    assert_eq!(directory.payload(0).unwrap(), descriptor);
    assert_eq!(directory.payload(2).unwrap(), b"untouched");
    let stored = Jkr::parse(directory.payload(1).unwrap()).unwrap();
    assert_eq!(stored.header.encoding, 0);
    assert_eq!(stored.header_extension().unwrap(), [0xaa, 0xbb, 0xcc, 0xdd]);
    let growth = stored.source.len() - compressed.len();
    assert!(growth > 0);
    assert_eq!(
        directory.entries[2].offset as usize,
        original_sibling as usize + growth
    );
    assert_eq!(&**stored.decode(44).unwrap(), model);
}

#[test]
fn effect_archive_member_replacement_preserves_descriptors_and_relocates_siblings() {
    let descriptor = [1, 0, 2, 0, 2, 0, 7, 0, 255, 0, 9, 0];
    let events = [0; 8];
    let source = archive(&[&descriptor, &events, b"untouched"], false);
    let original_sibling = SimpleArchive::parse(&source, 3).unwrap().entries[2].offset;
    let document = inspect::inspect("effects.bin", source.into());
    assert_eq!(document.nodes[document.root].kind, Kind::EffectArchive);
    let node = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::EffectMotionEvents)
        .unwrap();
    let mut replacement = events.to_vec();
    replacement[4..6].copy_from_slice(&1u16.to_le_bytes());
    replacement.extend_from_slice(&[0xa5; 32]);
    let updated = replace(&document, node, &replacement).unwrap();
    assert_eq!(updated.nodes[updated.root].kind, Kind::EffectArchive);
    assert!(updated.nodes.iter().all(|node| node.error.is_none()));
    let directory = SimpleArchive::parse(&updated.buffers[0], 3).unwrap();
    assert_eq!(directory.payload(0).unwrap(), descriptor);
    assert_eq!(directory.payload(1).unwrap(), replacement);
    assert_eq!(directory.payload(2).unwrap(), b"untouched");
    assert_eq!(directory.entries[2].offset, original_sibling + 32);
    assert_eq!(document.bytes(node).unwrap(), events);
}

fn effect_image() -> Vec<u8> {
    let mut bytes = vec![0; 5600];
    bytes[..4].copy_from_slice(dat::MAGIC);
    word(&mut bytes, 4, dat::VERSION);
    word(&mut bytes, 12, dat::HEADER_SIZE as u32);
    word(&mut bytes, 0x10, 3100);
    word(&mut bytes, 0x280, 3600);
    word(&mut bytes, 0x284, 4000);
    bytes[3100 + 0x72..3102 + 0x72].copy_from_slice(&2u16.to_le_bytes());
    bytes[3100 + 0x74..3102 + 0x74].copy_from_slice(&3u16.to_le_bytes());
    bytes[3618..3636].copy_from_slice(
        &mhf_resource::effect::AttachmentGroup {
            part_code: 4,
            definition_ids: [2, 0, 0, 0, 0, 0, 0, 0],
        }
        .to_bytes(),
    );
    bytes[4256 + 13] = 17;
    bytes
}

#[test]
fn dat_referenced_definition_edits_repack_the_complete_file_and_restore_expansion() {
    let original = effect_image();
    let directory = archive(&[&original, b"keep"], false);
    let document = inspect::inspect(
        "mhfdat.bin",
        ecd(&jkr(0, directory.len(), &directory)).into(),
    );
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(dat::DATA_TABLES.len()))
        .unwrap();
    let document = inspect::expand(&document, table).unwrap();
    let binding = document.nodes[table].children[1];
    let document = inspect::expand(&document, binding).unwrap();
    let definition = document.nodes[binding].children[0];
    let document = inspect::expand(&document, definition).unwrap();
    let key = node_key(&document, definition).unwrap();
    let node = &document.nodes[definition];
    let field = node
        .fields
        .iter()
        .find(|field| field.name == "骨骼节点索引")
        .unwrap();
    assert!(
        !document.nodes[binding]
            .range
            .contains(&field.binding.range.start)
    );
    let updated = apply(
        &document,
        node.buffer,
        field.binding.range.start..field.binding.range.start + 1,
        &[23],
    )
    .unwrap();
    let selected = locate(&updated, &key).unwrap();
    assert!(!updated.nodes[selected].deferred);
    assert_eq!(
        updated.nodes[selected]
            .fields
            .iter()
            .find(|field| field.name == "骨骼节点索引")
            .unwrap()
            .value,
        "23"
    );
    let decoded = open_layers(&updated.buffers[0], usize::MAX, 4).unwrap();
    let directory = SimpleArchive::parse(&decoded, 10).unwrap();
    let mut expected = original;
    expected[4269] = 23;
    assert_eq!(directory.payload(0).unwrap(), expected);
    assert_eq!(directory.payload(1).unwrap(), b"keep");
}

#[test]
fn shortened_dat_text_retains_only_its_observed_capacity_for_later_edits() {
    let mut bytes = effect_image();
    bytes[3108..3110].copy_from_slice(&2u16.to_le_bytes());
    word(&mut bytes, 0xfc, 3200);
    word(&mut bytes, 0x100, 3300);
    word(&mut bytes, 3300, 3340);
    word(&mut bytes, 3304, 3350);
    bytes[3340..3346].copy_from_slice(b"item0\0");
    bytes[3350..3357].copy_from_slice(b"longer\0");
    let document = inspect::inspect("mhfdat.bin", ecd(&jkr(0, bytes.len(), &bytes)).into());
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(7))
        .unwrap();
    let document = inspect::expand(&document, table).unwrap();
    let record = document.nodes[table].children[1];
    let document = inspect::expand(&document, record).unwrap();
    let key = node_key(&document, record).unwrap();
    let mut updated = document;
    for text in ["a", "longer"] {
        let node = &updated.nodes[locate(&updated, &key).unwrap()];
        let field = node
            .fields
            .iter()
            .find(|field| field.name == "名称")
            .unwrap();
        assert_eq!(field.binding.range.len(), 7);
        let patch = field.write(&updated.buffers, text).unwrap().unwrap();
        updated = apply_many(&updated, &[patch]).unwrap();
        let node = &updated.nodes[locate(&updated, &key).unwrap()];
        let field = node
            .fields
            .iter()
            .find(|field| field.name == "名称")
            .unwrap();
        assert_eq!(field.binding.range.len(), 7);
        assert_eq!(field.value, text);
    }
    // Unknown zero padding after the originally observed terminator remains
    // outside capacity, even though the backing DAT has room for more bytes.
    let node = &updated.nodes[locate(&updated, &key).unwrap()];
    let field = node
        .fields
        .iter()
        .find(|field| field.name == "名称")
        .unwrap();
    assert!(field.write(&updated.buffers, "too long").is_err());
}

#[test]
fn relocation_updates_exact_aliases_and_rejects_partial_overlaps() {
    let mut bytes = archive(&[b"abcd", b"tail"], false);
    word(&mut bytes, 12, 20);
    let updated = repack::replace(Kind::Txb, &bytes, 20..24, b"longer").unwrap();
    let parsed = SimpleArchive::parse(&updated, 2).unwrap();
    assert_eq!(parsed.payload(0).unwrap(), b"longer");
    assert_eq!(parsed.payload(1).unwrap(), b"longer");
    word(&mut bytes, 12, 22);
    assert!(repack::replace(Kind::Archive, &bytes, 20..24, b"longer").is_err());
    let dat = inspect::inspect("mhfdat.bin", effect_image().into());
    assert!(apply(&dat, 0, 3600..3601, &[1, 2]).is_err());
    assert!(repack::replace(Kind::Dat, &bytes, 20..24, b"longer").is_err());
}

#[test]
fn named_metadata_after_payload_and_stage_ids_survive_relocation() {
    let mut bytes = vec![0; 54];
    bytes[..4].copy_from_slice(b"mha\x01");
    for (at, value) in [
        (4, 27),
        (8, 1),
        (12, 47),
        (16, 7),
        (27, 0),
        (31, 24),
        (35, 3),
        (39, 3),
        (43, 999),
    ] {
        word(&mut bytes, at, value);
    }
    bytes[24..27].copy_from_slice(b"abc");
    bytes[47..54].copy_from_slice(b"x.bin\0\0");
    let updated = repack::replace(Kind::Mha, &bytes, 24..27, b"abcdef").unwrap();
    let parsed = MhaArchive::parse(&updated, 10).unwrap();
    assert_eq!(parsed.header.entries_offset, 30);
    assert_eq!(parsed.header.names_offset, 50);
    assert_eq!(parsed.entries[0].file_id, 999);
    assert_eq!(parsed.entries[0].name, b"x.bin");
    assert_eq!(
        parsed.entries[0].entry.payload(&updated).unwrap(),
        b"abcdef"
    );

    let mut bytes = vec![0; 44];
    for (at, value) in [(0, 40), (4, 2), (24, 1), (28, 77), (32, 42), (36, 2)] {
        word(&mut bytes, at, value);
    }
    bytes[40..44].copy_from_slice(b"abcd");
    let updated = repack::replace(Kind::Stage, &bytes, 40..42, b"abcdef").unwrap();
    let parsed = mhf_resource::container::StageArchive::parse(&updated, 4).unwrap();
    assert_eq!(parsed.entries[3].resource_id, Some(77));
    assert_eq!(parsed.entries[3].entry.offset, 46);
    assert_eq!(parsed.entries[3].entry.payload(&updated).unwrap(), b"cd");
}

#[test]
fn raw_wrapper_trailers_and_reference_identity_are_preserved() {
    let mut wrapped = jkr(1, 3, b"abc");
    wrapped.extend_from_slice(b"tail");
    let mut document = inspect::inspect("plain.bin", wrapped.into());
    let leaf = document.nodes[0].children[0];
    let key = node_key(&document, leaf).unwrap();
    let mut reference = document.nodes[leaf].clone();
    reference.kind = Kind::StageResourceReference;
    reference.children = vec![leaf];
    let reference_index = document.nodes.len();
    document.nodes.push(reference);
    document.nodes[0].children.push(reference_index);
    assert_eq!(node_key(&document, leaf), Some(key));
    // Restore the actual wrapper shape before rebuilding its payload.
    document.nodes[0].children.pop();
    document.nodes.pop();
    let updated = apply(&document, 1, 1..2, b"X").unwrap();
    let encoded = Jkr::parse(&updated.buffers[0]).unwrap();
    assert_eq!(encoded.header.encoding, 1);
    assert_eq!(&updated.buffers[0][23..], b"tail");
    assert_eq!(&**encoded.decode(3).unwrap(), b"aXc");
}

#[test]
fn uv_transform_switch_edit_reaches_the_parsed_rendering_record() {
    use mhf_resource::fmod::{FILE, MAIN, OBJECT, RENDERING, RENDERING_VERSION, UV_TRANSFORM_WORD};

    let mut parameters = [0; mhf_resource::fmod::RENDERING_WORDS];
    parameters[0] = RENDERING_VERSION;
    let payload: Vec<_> = parameters
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let source = fmod_block(
        FILE,
        1,
        &fmod_block(
            MAIN,
            1,
            &fmod_block(OBJECT, 1, &fmod_block(RENDERING, 1, &payload)),
        ),
    );
    let document = inspect::inspect("model.bin", source.into());
    let switch = document
        .nodes
        .iter()
        .flat_map(|node| &node.fields)
        .find(|field| field.name == "UV 变换")
        .unwrap();
    // `word_1C` is the switch row itself, and it stays a plain u32.
    assert_eq!(switch.value, "0");
    assert_eq!(switch.binding.format, FieldType::Scalar(ScalarType::U32));
    assert!(switch.writable);
    assert!(
        document
            .nodes
            .iter()
            .flat_map(|node| &node.fields)
            .all(|field| field.name != "word_1C")
    );
    let patch = switch.write(&document.buffers, "1").unwrap().unwrap();
    assert_eq!(patch.before, [0; 4]);
    assert_eq!(patch.after, [1, 0, 0, 0]);
    let updated = apply_many(&document, &[patch]).unwrap();
    let root = updated.payload(updated.root).unwrap();
    let model = Fmod::parse(updated.bytes(root).unwrap()).unwrap();
    let rendering = model.rendering_block(0).unwrap().unwrap();
    assert_eq!(rendering.words[UV_TRANSFORM_WORD], 1);
}

#[test]
fn rendering_parameter_initialization_repacks_nested_model_members() {
    use mhf_resource::fmod::{FILE, MAIN, OBJECT, RENDERING};

    let object = fmod_block(OBJECT, 0, &[]);
    let fmod = fmod_block(FILE, 1, &fmod_block(MAIN, 1, &object));
    let directory = archive(&[&jkr(0, fmod.len(), &fmod), b"unchanged"], false);
    let named = mha(&directory);
    let source = ecd(&named);
    let document = inspect::inspect("nested-model.bin", source.into());
    let item = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::MissingBlock)
        .unwrap();
    let updated = super::fmod::initialize_rendering_block(&document, item).unwrap();
    let decoded = open_layers(&updated.buffers[0], usize::MAX, 10).unwrap();
    let named = MhaArchive::parse(&decoded, 10).unwrap();
    let directory =
        SimpleArchive::parse(named.entries[0].entry.payload(&decoded).unwrap(), 2).unwrap();
    let compressed = directory.entries[0]
        .payload(named.entries[0].entry.payload(&decoded).unwrap())
        .unwrap();
    let model = open_layers(compressed, usize::MAX, 4).unwrap();
    let model = Fmod::parse(model.payload()).unwrap();
    let rendering = model.rendering_block(0).unwrap().unwrap();
    assert_eq!(rendering.block.header.kind, RENDERING);
    assert_eq!(rendering.words[0], mhf_resource::fmod::RENDERING_VERSION);
    assert!(rendering.words[1..].iter().all(|&word| word == 0));
    assert_eq!(
        directory.entries[1]
            .payload(named.entries[0].entry.payload(&decoded).unwrap())
            .unwrap(),
        b"unchanged"
    );
}

fn byte_patch(document: &Document, buffer: usize, range: Range<usize>, after: &[u8]) -> Patch {
    Patch {
        before: document.buffers[buffer][range.clone()].to_vec(),
        after: after.to_vec(),
        binding: Binding {
            buffer,
            range,
            format: FieldType::Bytes,
            endian: Default::default(),
        },
    }
}

#[test]
fn one_batch_rebuilds_sibling_buffers_in_physical_order_and_keeps_parent_edits() {
    let first = jkr(3, 27, &[0x70, b'A', 0, 0, 0]);
    let second = jkr(3, 27, &[0x70, b'B', 0, 0, 0]);
    let mut directory = archive(&[&first, &second, b"sibling"], false);
    // Entry order is independent of physical order, which relocation must use.
    let records = directory[4..20].to_vec();
    directory[4..12].copy_from_slice(&records[8..16]);
    directory[12..20].copy_from_slice(&records[..8]);
    let named = mha(&directory);
    let source = ecd(&jkr(0, named.len(), &named));
    let document = inspect::inspect("batch.bin", source.into());
    let leaf_a = (0..document.nodes.len())
        .find(|&node| document.bytes(node) == Some(&[b'A'; 27]))
        .unwrap();
    let leaf_b = (0..document.nodes.len())
        .find(|&node| document.bytes(node) == Some(&[b'B'; 27]))
        .unwrap();
    let sibling = (0..document.nodes.len())
        .find(|&node| document.bytes(node) == Some(b"sibling"))
        .unwrap();
    let sibling = &document.nodes[sibling];
    let patches = [
        byte_patch(&document, document.nodes[leaf_a].buffer, 0..1, b"X"),
        byte_patch(&document, document.nodes[leaf_b].buffer, 0..1, b"Y"),
        byte_patch(
            &document,
            sibling.buffer,
            sibling.range.start..sibling.range.start + 1,
            b"!",
        ),
    ];
    let updated = apply_many(&document, &patches).unwrap();
    let opened = open_layers(&updated.buffers[0], usize::MAX, 10).unwrap();
    let named = MhaArchive::parse(&opened, 10).unwrap();
    let directory =
        SimpleArchive::parse(named.entries[0].entry.payload(&opened).unwrap(), 10).unwrap();
    for (index, changed, rest) in [(0, b'Y', b'B'), (1, b'X', b'A')] {
        let leaf = Jkr::parse(directory.payload(index).unwrap())
            .unwrap()
            .decode(27)
            .unwrap();
        assert_eq!(leaf[0], changed);
        assert_eq!(leaf[1..], [rest; 26]);
    }
    assert_eq!(directory.payload(2).unwrap(), b"!ibling");
    assert!(
        apply_many(&updated, &patches)
            .unwrap_err()
            .contains("旧数据")
    );
}

#[test]
fn batches_reject_overlapping_fields_and_encoded_decoded_layer_conflicts() {
    let document = inspect::inspect("wrapped.bin", ecd(&jkr(0, 4, b"abcd")).into());
    let leaf = document.payload(document.root).unwrap();
    let buffer = document.nodes[leaf].buffer;
    let overlaps = [
        byte_patch(&document, buffer, 0..2, b"XY"),
        byte_patch(&document, buffer, 1..3, b"YZ"),
    ];
    assert!(
        apply_many(&document, &overlaps)
            .unwrap_err()
            .contains("重叠")
    );
    let layers = [
        byte_patch(&document, buffer, 0..1, b"X"),
        byte_patch(&document, 0, 16..17, &[document.buffers[0][16] ^ 1]),
    ];
    assert!(apply_many(&document, &layers).unwrap_err().contains("重叠"));
    let mut stale = byte_patch(&document, buffer, 0..1, b"X");
    stale.before[0] ^= 1;
    assert!(
        apply_many(&document, &[stale])
            .unwrap_err()
            .contains("旧数据")
    );
    assert_eq!(document.bytes(leaf).unwrap(), b"abcd");
}

fn png(text: &[u8]) -> Vec<u8> {
    let mut bytes = mhf_resource::png::MAGIC.to_vec();
    for (kind, data) in [
        (&b"IHDR"[..], &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0][..]),
        (&b"tEXt"[..], text),
        (&b"IDAT"[..], &[0x78, 0x9c, 0x63, 0, 1, 0, 0, 5, 0, 1][..]),
        (&b"IEND"[..], &[][..]),
    ] {
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let at = bytes.len();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(data);
        let crc = mhf_resource::crypto::crc32(&bytes[at..]);
        bytes.extend_from_slice(&crc.to_be_bytes());
    }
    bytes
}

#[test]
fn complete_asset_replacement_rebuilds_directories_but_fragments_cannot_resize() {
    let texture = png(b"name\0old");
    let imported = png(b"name\0new imported asset");
    let directory = archive(&[&texture, b"untouched"], false);
    let document = inspect::inspect(
        "assets.bin",
        ecd(&jkr(0, directory.len(), &directory)).into(),
    );
    let node = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Png)
        .unwrap();
    let key = node_key(&document, node).unwrap();
    let updated = replace(&document, node, &imported).unwrap();
    let selected = locate(&updated, &key).unwrap();
    assert_eq!(updated.bytes(selected).unwrap(), imported);
    assert!(updated.nodes[selected].error.is_none());
    let opened = open_layers(&updated.buffers[0], usize::MAX, 10).unwrap();
    assert_eq!(
        SimpleArchive::parse(&opened, 10)
            .unwrap()
            .payload(1)
            .unwrap(),
        b"untouched"
    );
    let whole = replace(&updated, updated.root, &imported).unwrap();
    assert_eq!(whole.nodes[whole.root].kind, Kind::Png);
    assert_eq!(&*whole.buffers[0], imported);

    let document = inspect::inspect("mhfdat.bin", effect_image().into());
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(dat::DATA_TABLES.len()))
        .unwrap();
    let document = inspect::expand(&document, table).unwrap();
    assert!(
        replace(&document, document.nodes[table].children[1], &[0; 19])
            .unwrap_err()
            .contains("内部记录")
    );
    let group = document.nodes[document.root].children[0];
    assert!(
        replace(&document, group, &[0; 32])
            .unwrap_err()
            .contains("内部记录")
    );
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; edits and reopens the original complete mhfdat"]
fn original_dat_effect_edit_repackages_the_full_image() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
    let document = inspect::inspect("mhfdat.bin", source.into());
    let dat = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Dat)
        .unwrap();
    let node = &document.nodes[dat];
    let image = mhf_resource::dat::Dat::parse(document.bytes(dat).unwrap()).unwrap();
    let at = image.pointer(0x280).unwrap().unwrap() + 18;
    let mut expected = document.bytes(dat).unwrap().to_vec();
    expected[at] ^= 1;
    let updated = apply(
        &document,
        node.buffer,
        node.range.start + at..node.range.start + at + 1,
        &expected[at..at + 1],
    )
    .unwrap();
    let dat = updated
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Dat)
        .unwrap();
    assert_eq!(updated.bytes(dat).unwrap(), expected);
    assert!(updated.nodes[updated.root].error.is_none());
    let decoded = open_layers(&updated.buffers[0], usize::MAX, 10).unwrap();
    assert_eq!(decoded.payload(), expected);
}
