use crate::{
    edit::{apply_many, locate, node_key, prepare_pack, replace},
    field::{Field, FieldType, ScalarType},
    inspect::{self, Document, Kind},
};
use mhf_resource::{container::SimpleArchive, effect_archive::EffectBank, jkr::Jkr};

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn short(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn bank() -> Vec<u8> {
    let mut bytes = vec![0; 28 + 3 * 24 + 16 + 16 + 2 * 56];
    for (at, value) in [(0, 4), (4, 3), (6, 1), (8, 1), (10, 2)] {
        short(&mut bytes, at, value);
    }
    bytes[20..28].fill(0xa7);
    for (index, id) in [7, 8, 7].into_iter().enumerate() {
        let at = 28 + index * 24;
        bytes[at..at + 24].fill(0xa5);
        word(&mut bytes, at, 0x7fc0_1234);
        word(&mut bytes, at + 4, (-0_f32).to_bits());
        word(&mut bytes, at + 8, 1_f32.to_bits());
        word(&mut bytes, at + 12, (-(index as i32)) as u32);
        bytes[at + 18] = id;
    }
    bytes[100..116].fill(0xc3);
    bytes[106] = 9;
    bytes[108..112].copy_from_slice(&[1, 2, 3, 4]);
    bytes[116..132].fill(0xd5);
    short(&mut bytes, 122, u16::MAX);
    for index in 0..2 {
        let at = 132 + index * 56;
        bytes[at..at + 56].fill(0xb7);
        word(&mut bytes, at, 0x8000_0001);
        for (offset, value) in [
            (4, index as u16),
            (10, u16::MAX),
            (12, 30),
            (14, 7),
            (16, 0x107),
            (18, 8),
            (32, 9),
            (34, 7),
            (36, u16::MAX),
            (38, 8),
        ] {
            short(&mut bytes, at + offset, value);
        }
    }
    bytes
}

fn wrapped(bank: &[u8]) -> Vec<u8> {
    let mut archive = vec![0; 28];
    for (offset, value) in [(0, 2), (4, 20), (8, 8), (12, 28), (16, bank.len() as u32)] {
        word(&mut archive, offset, value);
    }
    for (offset, value) in [(20, 1), (22, 1), (24, 1), (26, 149)] {
        short(&mut archive, offset, value);
    }
    archive.extend_from_slice(bank);
    let mut bytes = b"JKR\x1a\x08\x01\0\0".to_vec();
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&(archive.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    bytes.extend_from_slice(&archive);
    bytes
}

fn field<'a>(document: &'a Document, node: usize, name: &str) -> &'a Field {
    document.nodes[node]
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap()
}

#[test]
fn definition_references_locate_native_spans_without_reparenting_physical_keys() {
    let document = inspect::inspect("effects.bin", wrapped(&bank()).into());
    let bank_node = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::EffectBank)
        .unwrap();
    assert!(document.nodes[bank_node].deferred);
    assert!(document.nodes[bank_node].children.is_empty());
    let document = inspect::expand(&document, bank_node).unwrap();
    let base = document.nodes[bank_node].range.start;
    assert_eq!(base, 28);
    let tables = &document.nodes[bank_node].children;
    assert_eq!(tables.len(), 4);
    let vectors = tables[0];
    let definitions = tables[3];
    assert_eq!(document.nodes[vectors].range, base + 28..base + 100);
    assert_eq!(document.nodes[vectors].children.len(), 3);
    assert_eq!(document.nodes[definitions].children.len(), 2);
    for &definition in &document.nodes[definitions].children {
        assert!(document.nodes[definition].children.is_empty());
        let reference = field(&document, definition, "+0E 三分量曲线引用");
        assert_eq!(reference.binding.range, base + 28..base + 28 + 48);
        assert_eq!(reference.binding.buffer, document.nodes[vectors].buffer);
        assert!(reference.value.contains("[0, 2]"));
        assert!(reference.value.contains("原生 0..2"));
        assert!(reference.value.contains("匹配位置与原生跨度不同"));
        assert_eq!(reference.binding.format, FieldType::ReadOnly);
        assert!(reference.write(&document.buffers, "7").is_err());
        for name in ["+10 三分量曲线引用", "+24 整数曲线引用"] {
            let reference = field(&document, definition, name);
            assert!(reference.value.contains("无匹配记录"));
            assert!(reference.binding.range.is_empty());
        }
    }
    for &key in &document.nodes[vectors].children {
        assert_eq!(
            document
                .nodes
                .iter()
                .filter(|node| node.children.contains(&key))
                .count(),
            1
        );
        assert_eq!(
            locate(&document, &node_key(&document, key).unwrap()),
            Some(key)
        );
    }
    assert!(document.nodes.iter().all(|node| node.error.is_none()));
}

#[test]
fn typed_definition_and_curve_edits_rebuild_jkr_and_preserve_adjacent_bytes() {
    let source = bank();
    let document = inspect::inspect("effects.bin", wrapped(&source).into());
    let bank_node = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::EffectBank)
        .unwrap();
    let document = inspect::expand(&document, bank_node).unwrap();
    let base = document.nodes[bank_node].range.start;
    let tables = &document.nodes[bank_node].children;
    let vector = document.nodes[tables[0]].children[0];
    let color = document.nodes[tables[1]].children[0];
    let integer = document.nodes[tables[2]].children[0];
    let definition = document.nodes[tables[3]].children[0];
    let edits = [
        (
            definition,
            "duration_steps",
            "400",
            base + 144..base + 146,
            FieldType::Scalar(ScalarType::U16),
        ),
        (
            definition,
            "position_curve_id",
            "263",
            base + 146..base + 148,
            FieldType::Scalar(ScalarType::U16),
        ),
        (
            definition,
            "integer_curve_24",
            "-2",
            base + 168..base + 170,
            FieldType::Scalar(ScalarType::I16),
        ),
        (
            vector,
            "curve_id",
            "10",
            base + 46..base + 47,
            FieldType::Scalar(ScalarType::U8),
        ),
        (
            vector,
            "value",
            "NaN, -0, 2",
            base + 28..base + 40,
            FieldType::Array(ScalarType::F32),
        ),
        (
            color,
            "rgba",
            "10, 20, 30, 40",
            base + 108..base + 112,
            FieldType::Color { alpha: true },
        ),
        (
            integer,
            "curve_id",
            "60000",
            base + 122..base + 124,
            FieldType::Scalar(ScalarType::U16),
        ),
    ];
    let patches: Vec<_> = edits
        .into_iter()
        .map(|(node, name, input, range, kind)| {
            let field = field(&document, node, name);
            assert_eq!(field.binding.range, range);
            assert_eq!(field.binding.format, kind);
            field.write(&document.buffers, input).unwrap().unwrap()
        })
        .collect();
    assert!(
        field(&document, vector, "curve_id")
            .write(&document.buffers, "256")
            .is_err()
    );
    let key = node_key(&document, definition).unwrap();
    let updated = apply_many(&document, &patches).unwrap();
    let mut expected = source.clone();
    short(&mut expected, 144, 400);
    short(&mut expected, 146, 263);
    short(&mut expected, 168, (-2_i16) as u16);
    expected[46] = 10;
    word(&mut expected, 36, 2_f32.to_bits());
    expected[108..112].copy_from_slice(&[10, 20, 30, 40]);
    short(&mut expected, 122, 60000);
    assert_eq!(updated.bytes(bank_node).unwrap(), expected);
    assert_eq!(document.bytes(bank_node).unwrap(), source);
    let definition = locate(&updated, &key).unwrap();
    assert_eq!(
        field(&updated, definition, "duration_steps")
            .read(&updated.buffers)
            .unwrap(),
        "400"
    );
    assert!(
        field(&updated, definition, "+0E 三分量曲线引用")
            .value
            .contains("无匹配记录")
    );
    assert!(replace(&updated, definition, &[0; 57]).is_err());
    let packed = prepare_pack(&updated, &[]).unwrap();
    let envelope = Jkr::parse(&packed.buffers[0]).unwrap();
    let decoded = envelope.decode(1024).unwrap();
    let archive = SimpleArchive::parse(&decoded, 2).unwrap();
    assert_eq!(archive.payload(0).unwrap(), [1, 0, 1, 0, 1, 0, 149, 0]);
    assert_eq!(archive.payload(1).unwrap(), expected);
    assert_eq!(
        EffectBank::parse(archive.payload(1).unwrap())
            .unwrap()
            .definitions_56[0]
            .duration_steps,
        400
    );
    assert_eq!(&packed.buffers[0][16..20], &[0xde, 0xad, 0xbe, 0xef]);
    assert!(packed.nodes.iter().all(|node| node.error.is_none()));
}
