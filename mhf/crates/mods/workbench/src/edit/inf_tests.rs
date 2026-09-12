use super::*;
use crate::field::{Field, TextEncoding};
use mhf_resource::{
    container::{MhaArchive, SimpleArchive},
    crypto::Ecd,
};

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn inf() -> Vec<u8> {
    let mut bytes = vec![0; 256];
    bytes[..4].copy_from_slice(b"inf\x1a");
    for (offset, value) in [
        (4, 6),
        (8, 0xaabb_ccdd),
        (12, 136),
        (16, 136),
        (20, 140),
        (136, 1),
        (140, 199 | (3 << 16)),
        (144, 148),
        (152, 160),
        (156, 160),
        (200, 208),
        (208, 240),
        (212, 240),
    ] {
        word(&mut bytes, offset, value);
    }
    bytes[206..208].copy_from_slice(&101u16.to_le_bytes());
    bytes[240..249].copy_from_slice(b"original\0");
    bytes[249..].fill(0xa7);
    bytes
}

fn wrapped(inf: &[u8]) -> Vec<u8> {
    // The inner directory gives INF a nonzero base in its decoded buffer.
    let mut directory = vec![0; 12];
    word(&mut directory, 0, 1);
    word(&mut directory, 4, 12);
    word(&mut directory, 8, inf.len() as u32);
    directory.extend_from_slice(inf);
    let encrypted = Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
        .unwrap()
        .encode(&directory, Some(b"mhfinf.bin"))
        .unwrap();
    let mut bytes = vec![0; 64];
    bytes[..4].copy_from_slice(b"mha\x01");
    for (offset, value) in [
        (4, 24),
        (8, 1),
        (12, 44),
        (16, 11),
        (20, 1 << 16),
        (28, 64),
        (32, encrypted.len() as u32),
        (36, encrypted.len() as u32),
    ] {
        word(&mut bytes, offset, value);
    }
    bytes[44..55].copy_from_slice(b"mhfinf.bin\0");
    bytes.extend_from_slice(&encrypted);
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
fn aliased_inf_text_edits_rebuild_named_envelopes_at_the_original_field_range() {
    let source = inf();
    let mut document = inspect::inspect("quests.abn", wrapped(&source).into());
    let root = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Inf)
        .unwrap();
    let base = document.nodes[root].range.start;
    assert_eq!(base, 12);
    let category = document.nodes[root].children[0];
    assert_eq!(document.nodes[category].kind, Kind::InfCategory(0));
    document = inspect::expand(&document, category).unwrap();
    let quests: Vec<_> = document.nodes[category]
        .children
        .iter()
        .copied()
        .filter(|&node| document.nodes[node].kind == Kind::InfQuest)
        .collect();
    assert_eq!(quests.len(), 2);
    for &quest in &quests {
        document = inspect::expand(&document, quest).unwrap();
        assert_eq!(document.nodes[quest].range, base + 160..base + 208);
    }

    let text = field(&document, quests[0], "文本 0");
    assert_eq!(text.binding.buffer, document.nodes[root].buffer);
    assert_eq!(text.binding.range, base + 240..base + 249);
    assert!(matches!(
        text.binding.format,
        FieldType::Text {
            encoding: TextEncoding::ShiftJis,
            terminated: true,
        }
    ));
    assert!(
        text.write(&document.buffers, "beyond the known capacity")
            .is_err()
    );
    let patch = text.write(&document.buffers, "edited").unwrap().unwrap();
    let keys: Vec<_> = quests
        .iter()
        .map(|&node| node_key(&document, node).unwrap())
        .collect();
    let updated = apply_many(&document, &[patch]).unwrap();
    let mut expected = source.clone();
    expected[240..249].copy_from_slice(b"edited\0\0\0");
    assert_eq!(updated.bytes(root).unwrap(), expected);
    assert_eq!(document.bytes(root).unwrap(), source);
    for key in &keys {
        let quest = locate(&updated, key).unwrap();
        for name in ["文本 0", "文本 1"] {
            assert_eq!(
                field(&updated, quest, name).read(&updated.buffers).unwrap(),
                "edited"
            );
        }
        assert!(replace(&updated, quest, &[0; 49]).is_err());
    }

    let packed = prepare_pack(&updated, &[]).unwrap();
    let archive = MhaArchive::parse(&packed.buffers[0], 1).unwrap();
    let encrypted = Ecd::parse(
        archive.entries[0]
            .entry
            .payload(&packed.buffers[0])
            .unwrap(),
    )
    .unwrap();
    encrypted.validate_filename(b"mhfinf.bin").unwrap();
    let decoded = encrypted.decode(1024).unwrap();
    assert_eq!(
        SimpleArchive::parse(&decoded, 1)
            .unwrap()
            .payload(0)
            .unwrap(),
        expected
    );
}
