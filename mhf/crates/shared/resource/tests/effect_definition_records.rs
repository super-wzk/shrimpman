use mhf_resource::{
    container::{SimpleArchive, open_layers},
    effect_archive::{
        CurveKind, CurveReference, Definition56, EffectArchive, EffectBank, EffectResource,
    },
};

fn short(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn definition() -> [u8; Definition56::SIZE] {
    let mut bytes = std::array::from_fn(|index| (index as u8).wrapping_mul(37));
    bytes[..4].copy_from_slice(&0x8123_4567_u32.to_le_bytes());
    for (offset, value) in [
        (4, 12),
        (6, 0xabcd),
        (8, 78),
        (10, u16::MAX),
        (12, 30),
        (14, 0x101),
        (16, 0),
        (18, 255),
        (32, 1),
        (34, 7),
        (36, 0xffff),
        (38, 2),
    ] {
        short(&mut bytes, offset, value);
    }
    bytes
}

fn bank_bytes(
    vector_ids: &[u8],
    color_ids: &[u8],
    integer_ids: &[u16],
    definition: &[u8; 56],
) -> Vec<u8> {
    let mut bytes = vec![0; 28];
    for (offset, value) in [
        (0, 4),
        (4, vector_ids.len()),
        (6, color_ids.len()),
        (8, integer_ids.len()),
        (10, 1),
    ] {
        short(&mut bytes, offset, value as u16);
    }
    bytes[20..28].fill(0xa7);
    for (index, &id) in vector_ids.iter().enumerate() {
        let mut record = [0xa5; 24];
        for (i, value) in [0x7fc0_1234_u32, 0x8000_0000, 1.0_f32.to_bits()]
            .into_iter()
            .enumerate()
        {
            record[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        record[12..16].copy_from_slice(&(-(index as i32)).to_le_bytes());
        record[18] = id;
        bytes.extend_from_slice(&record);
    }
    for (index, &id) in color_ids.iter().enumerate() {
        let mut record = [0xc3; 16];
        record[..4].copy_from_slice(&(index as u32).to_le_bytes());
        record[6] = id;
        record[8..12].copy_from_slice(&[1, 2, 3, 4]);
        bytes.extend_from_slice(&record);
    }
    for (index, &id) in integer_ids.iter().enumerate() {
        let mut record = [0xd5; 16];
        record[..4].copy_from_slice(&(index as u32).to_le_bytes());
        short(&mut record, 6, id);
        record[8..12].copy_from_slice(&(-(index as i32)).to_le_bytes());
        bytes.extend_from_slice(&record);
    }
    bytes.extend_from_slice(definition);
    bytes
}

#[test]
fn definition_fields_and_roundtrip_preserve_every_unconfirmed_byte() {
    let bytes = definition();
    let mut parsed = Definition56::from_record(&bytes);
    assert_eq!(parsed.flags, 0x8123_4567);
    assert_eq!(parsed.definition_id, 12);
    assert_eq!(parsed.unknown_06, 0xabcd);
    assert_eq!(parsed.unknown_08, 78);
    assert_eq!(parsed.duration_steps, 30);
    assert_eq!(parsed.position_curve_id, 0x101);
    assert_eq!(parsed.integer_curve_24, -1);
    assert_eq!(parsed.unknown_14, bytes[20..32]);
    assert_eq!(parsed.unknown_28, bytes[40..56]);
    assert_eq!(parsed.to_bytes(), bytes);

    parsed.duration_steps = 400;
    parsed.integer_curve_24 = -2;
    let mut expected = bytes;
    short(&mut expected, 12, 400);
    short(&mut expected, 36, (-2_i16) as u16);
    assert_eq!(parsed.to_bytes(), expected);
    assert_eq!(Definition56::from_record(&expected), parsed);
}

#[test]
fn curve_references_keep_full_unsigned_words_and_the_signed_integer_selector() {
    let source = bank_bytes(&[0, 1, 1, 255], &[0, 1], &[0, 65535], &definition());
    let bank = EffectBank::parse(&source).unwrap();
    let references = bank.definitions_56[0].curve_references();
    let expected = [
        (0x0a, CurveKind::Integer, 65535, vec![1]),
        (0x0e, CurveKind::Vector, 257, vec![]),
        (0x10, CurveKind::Vector, 0, vec![0]),
        (0x12, CurveKind::Vector, 255, vec![3]),
        (0x20, CurveKind::Color, 1, vec![1]),
        (0x22, CurveKind::Vector, 7, vec![]),
        (0x24, CurveKind::Integer, -1, vec![]),
        (0x26, CurveKind::Vector, 2, vec![]),
    ];
    for (reference, (offset, kind, id, matching)) in references.into_iter().zip(expected) {
        assert_eq!(reference, CurveReference { offset, kind, id });
        let lookup = bank.curve_lookup(reference);
        assert_eq!(lookup.matching_indices, matching);
        assert!(lookup.is_contiguous());
        assert_eq!(
            lookup.native_range(),
            matching.first().map(|&first| first..first + matching.len())
        );
    }
    assert!(
        bank.curve_lookup(CurveReference {
            offset: 0x20,
            kind: CurveKind::Color,
            id: 257
        })
        .matching_indices
        .is_empty()
    );
    assert_eq!(bank.vector_keys[0].unknown_13, 0xa5);
    assert_eq!(bank.color_keys[0].unknown_07, 0xc3);
    assert_eq!(bank.as_bytes(), source);
}

#[test]
fn noncontiguous_matching_keys_and_native_first_plus_count_span_remain_distinct() {
    let source = bank_bytes(
        &[7, 8, 7, 9, 7],
        &[7, 8, 7, 9, 7],
        &[7, 8, 7, 9, 7],
        &definition(),
    );
    let bank = EffectBank::parse(&source).unwrap();
    for kind in [CurveKind::Vector, CurveKind::Color, CurveKind::Integer] {
        let lookup = bank.curve_lookup(CurveReference {
            offset: 0,
            kind,
            id: 7,
        });
        assert_eq!(lookup.matching_indices, [0, 2, 4]);
        assert_eq!(lookup.native_range(), Some(0..3));
        assert!(!lookup.is_contiguous());
    }
    assert_eq!(
        bank.vector_keys
            .iter()
            .map(|key| key.curve_id)
            .collect::<Vec<_>>(),
        [7, 8, 7, 9, 7]
    );
    assert_eq!(bank.as_bytes(), source);
}

#[test]
fn absent_key_tables_preserve_definition_selectors_without_fabricating_a_curve() {
    let source = bank_bytes(&[], &[], &[], &definition());
    let bank = EffectBank::parse(&source).unwrap();
    for reference in bank.definitions_56[0].curve_references() {
        let lookup = bank.curve_lookup(reference);
        assert!(lookup.matching_indices.is_empty());
        assert_eq!(lookup.native_range(), None);
        assert!(lookup.is_contiguous());
    }
    assert_eq!(bank.definitions_56[0].to_bytes(), definition());
}

#[test]
#[ignore = "requires MHF_CLIENT_DATA_DIR; reads original effect definitions and key tables"]
fn original_definition_references_include_contiguous_and_interleaved_keys() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_CLIENT_DATA_DIR").unwrap());
    for (path, bank_id, definition_index, offset, id, matching, native) in [
        ("emmodel/em001.pac", 149, 0, 0x0e, 2, vec![3, 4, 5], 3..6),
        ("emmodel/em001.pac", 149, 0, 0x12, 1, vec![0, 1, 2], 0..3),
        ("emmodel/em001.pac", 149, 0, 0x20, 2, vec![4, 5, 6], 4..7),
        (
            "emmodel/em105.pac",
            1022,
            39,
            0x22,
            37,
            vec![91, 92, 93, 94, 97],
            91..96,
        ),
    ] {
        let source = std::fs::read(root.join(path)).unwrap();
        let outer_bytes = open_layers(&source, usize::MAX, 16).unwrap();
        let outer = SimpleArchive::parse(outer_bytes.payload(), 100).unwrap();
        let effect_bytes = open_layers(outer.payload(5).unwrap(), usize::MAX, 16).unwrap();
        let archive = EffectArchive::parse(effect_bytes.payload(), 4096).unwrap();
        let member = archive
            .members
            .iter()
            .find(|member| member.reference.kind == 1 && member.reference.resource_id == bank_id)
            .unwrap();
        let EffectResource::Bank(bank) = member.resource().unwrap() else {
            unreachable!()
        };
        let record = &bank.definitions_56[definition_index];
        let reference = record
            .curve_references()
            .into_iter()
            .find(|reference| reference.offset == offset)
            .unwrap();
        assert_eq!(reference.id, id);
        let lookup = bank.curve_lookup(reference);
        assert_eq!(lookup.matching_indices, matching);
        assert_eq!(lookup.native_range(), Some(native));
        assert_eq!(lookup.is_contiguous(), path.ends_with("em001.pac"));
        let start = bank.table_offsets[4] + definition_index * Definition56::SIZE;
        assert_eq!(
            record.to_bytes(),
            member.as_bytes()[start..start + Definition56::SIZE]
        );
    }
}
