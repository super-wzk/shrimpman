use super::*;
use crate::inspect::{expand, inspect};

fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn item_image() -> Vec<u8> {
    let mut bytes = vec![0; 3400];
    bytes[..4].copy_from_slice(dat::MAGIC);
    set_u32(&mut bytes, 4, dat::VERSION);
    set_u32(&mut bytes, 12, dat::HEADER_SIZE as u32);
    set_u32(&mut bytes, 0x10, 3100);
    bytes[3108..3110].copy_from_slice(&2u16.to_le_bytes());
    set_u32(&mut bytes, 0xfc, 3200);
    set_u32(&mut bytes, 0x100, 3300);
    set_u32(&mut bytes, 3300, 3340);
    set_u32(&mut bytes, 3304, 3350);
    bytes[3340..3346].copy_from_slice(b"item0\0");
    bytes[3350..3356].copy_from_slice(b"item1\0");
    set_u32(&mut bytes, 3248, 123456);
    bytes[3243] = 0xcc;
    // One of the scalar count slots interspersed among header pointers.
    set_u32(&mut bytes, 616 * 4, 13);
    bytes
}

#[test]
fn dat_inside_an_archive_expands_binary_fields_with_absolute_buffer_offsets() {
    let dat = item_image();
    let mut archive = vec![0; 32];
    set_u32(&mut archive, 0, 1);
    set_u32(&mut archive, 4, 32);
    set_u32(&mut archive, 8, dat.len() as u32);
    archive.extend_from_slice(&dat);
    let document = inspect("data.bin", archive.clone().into());
    // The small fixture deliberately omits DAT's large absolute text tables.
    assert!(
        document
            .nodes
            .iter()
            .filter(|node| !matches!(node.kind, Kind::DatTable(_)))
            .all(|node| node.error.is_none())
    );
    let item_table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(7))
        .unwrap();
    assert!(document.nodes[item_table].deferred);
    assert!(document.nodes[item_table].children.is_empty());
    let document = expand(&document, item_table).unwrap();
    assert_eq!(document.nodes[item_table].children.len(), 2);
    let record = document.nodes[item_table].children[1];
    assert!(document.nodes[record].name.contains("item1"));
    assert_eq!(document.bytes(record).unwrap(), &dat[3236..3272]);
    let document = expand(&document, record).unwrap();
    let node = &document.nodes[record];
    assert!(!node.deferred);
    let price = node
        .fields
        .iter()
        .find(|field| field.name == "买入价格")
        .unwrap();
    assert!(price.value.starts_with("123456"));
    assert_eq!(
        (price.binding.range.start, price.binding.range.len()),
        (32 + 3248, 4)
    );
    assert_eq!(price.binding.format, FieldType::Scalar(ScalarType::U32));
    assert_eq!(
        price
            .binding
            .format
            .encode(
                &archive[price.binding.range.start
                    ..price.binding.range.start + price.binding.range.len()],
                "42"
            )
            .unwrap(),
        42_u32.to_le_bytes()
    );
    let unknown = node
        .fields
        .iter()
        .find(|field| field.binding.range.start == 32 + 3243)
        .unwrap();
    assert_eq!(
        &archive[unknown.binding.range.start
            ..unknown.binding.range.start + unknown.binding.range.len()],
        [0xcc]
    );
    let text = node
        .fields
        .iter()
        .find(|field| field.name == "名称")
        .unwrap();
    assert_eq!(
        (text.binding.range.start, text.binding.range.len()),
        (32 + 3350, 6)
    );
    assert_eq!(
        text.binding.format,
        FieldType::Text {
            encoding: TextEncoding::ShiftJis,
            terminated: true
        }
    );
    assert!(
        text.binding
            .format
            .encode(
                &archive
                    [text.binding.range.start..text.binding.range.start + text.binding.range.len()],
                "item too long"
            )
            .is_err()
    );
    let mut coverage = [false; 36];
    for field in &node.fields {
        if field.binding.range.start >= node.range.start
            && field.binding.range.start + field.binding.range.len() <= node.range.end
        {
            coverage[field.binding.range.start - node.range.start
                ..field.binding.range.start - node.range.start + field.binding.range.len()]
                .fill(true);
        }
    }
    assert!(coverage.into_iter().all(|covered| covered));
    assert_eq!(
        expand(&document, record).unwrap().nodes[record]
            .fields
            .len(),
        node.fields.len()
    );
}

#[test]
fn corrupt_names_do_not_hide_item_data_or_other_records() {
    let mut bytes = item_image();
    set_u32(&mut bytes, 3304, u32::MAX);
    let document = inspect("mhfdat.bin", bytes.into());
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(7))
        .unwrap();
    let document = expand(&document, table).unwrap();
    let records = document.nodes[table].children.clone();
    assert!(document.nodes[records[0]].error.is_none());
    assert!(document.nodes[records[1]].error.is_some());
    let document = expand(&document, records[1]).unwrap();
    assert!(
        document.nodes[records[1]]
            .fields
            .iter()
            .any(|field| field.name == "买入价格")
    );
    let mut bytes = item_image();
    set_u32(&mut bytes, 0x100, u32::MAX);
    let document = inspect("mhfdat.bin", bytes.into());
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(7))
        .unwrap();
    let document = expand(&document, table).unwrap();
    assert!(document.nodes[table].error.is_some());
    assert_eq!(document.nodes[table].children.len(), 2);
}

#[test]
fn jkr_wrapped_dat_and_invalid_dat_versions_keep_their_identity() {
    let dat = item_image();
    let mut bytes = b"JKR\x1a\x08\x01\0\0".to_vec();
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&(dat.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&dat);
    let document = inspect("unrelated.pac", bytes.into());
    assert_eq!(document.nodes[document.root].kind, Kind::Jkr);
    assert!(
        document
            .nodes
            .iter()
            .any(|node| node.kind == Kind::Dat && node.buffer == 1)
    );
    let mut wrong = dat;
    set_u32(&mut wrong, 4, 90);
    let document = inspect("mhfdat.bin", wrong.into());
    assert_eq!(document.nodes[document.root].kind, Kind::Dat);
    assert!(
        document.nodes[document.root]
            .error
            .as_deref()
            .unwrap()
            .contains("version")
    );
}

#[test]
fn effect_bindings_follow_their_own_definitions_and_keep_unused_ids() {
    let mut bytes = item_image();
    bytes.resize(5600, 0);
    for (field, offset, count_field, count) in [
        (0x280, 3600, 0x72, 2u16),
        (0x284, 4000, 0x74, 3),
        (0x294, 4600, 0x7c, 3),
        (0x298, 5000, 0x7e, 3),
    ] {
        set_u32(&mut bytes, field, offset);
        bytes[3100 + count_field..3102 + count_field].copy_from_slice(&count.to_le_bytes());
    }
    let attachment = mhf_resource::effect::AttachmentGroup {
        part_code: 4,
        definition_ids: [2, 0, 99, 0, 0, 0, 0, 0],
    };
    bytes[3618..3636].copy_from_slice(&attachment.to_bytes());
    bytes[4000 + 256 + 13] = 17;
    let binding = mhf_resource::effect::ModelEffectBinding {
        part_code: 3,
        weapon_class: 7,
        variant: 2,
        model_id: 44,
        definition_ids: [1, 0, 999, 0, 0, 0, 0, 0],
    };
    bytes[4624..4648].copy_from_slice(&binding.to_bytes());
    set_u32(&mut bytes, 5180, 0x8000_0000); // Preserve negative zero.
    bytes[5180 + 13] = 6;
    bytes[5180 + 15] = 21;
    let mut broken = bytes.clone();
    broken[4632..4634].copy_from_slice(&u16::MAX.to_le_bytes());
    let document = inspect("mhfdat.bin", bytes.into());
    for (table_index, definition_at, node_at, node_index) in [
        (dat::DATA_TABLES.len(), 4256, 4269, "17"),
        (dat::DATA_TABLES.len() + 2, 5180, 5195, "21"),
    ] {
        let table = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::DatTable(table_index))
            .unwrap();
        let records = expand(&document, table).unwrap();
        let binding = records.nodes[table].children[1];
        let details = expand(&records, binding).unwrap();
        assert!(details.nodes[binding].error.is_none());
        assert_eq!(details.nodes[binding].children.len(), 1);
        assert!(
            details.nodes[binding]
                .fields
                .iter()
                .any(|field| field.name == "未使用定义槽 2")
        );
        let definition = details.nodes[binding].children[0];
        assert_eq!(details.nodes[definition].range.start, definition_at);
        let details = expand(&details, definition).unwrap();
        for field in &details.nodes[definition].fields {
            assert_ne!(field.binding.format, FieldType::ReadOnly, "{}", field.name);
            let bytes = &details.buffers[details.nodes[definition].buffer]
                [field.binding.range.start..field.binding.range.start + field.binding.range.len()];
            let input = field.binding.format.decode(bytes).unwrap();
            assert_eq!(field.binding.format.encode(bytes, &input).unwrap(), bytes);
        }
        let field = details.nodes[definition]
            .fields
            .iter()
            .find(|field| field.name == "骨骼节点索引")
            .unwrap();
        assert_eq!(
            (field.binding.range.start, field.value.as_str()),
            (node_at, node_index)
        );
        if definition_at == 4256 {
            let node = &details.nodes[definition];
            let mut covered = [false; 128];
            for field in &node.fields {
                if field.binding.range.start >= node.range.start
                    && field.binding.range.start + field.binding.range.len() <= node.range.end
                {
                    let start = field.binding.range.start - node.range.start;
                    assert!(
                        covered[start..start + field.binding.range.len()]
                            .iter()
                            .all(|value| !value)
                    );
                    covered[start..start + field.binding.range.len()].fill(true);
                }
            }
            assert!(covered.into_iter().all(|value| value));
            assert_eq!(
                node.fields
                    .iter()
                    .find(|field| field.binding.range.start == definition_at + 0xf)
                    .unwrap()
                    .binding
                    .range
                    .len(),
                1
            );
            assert_eq!(
                node.fields
                    .iter()
                    .find(|field| field.binding.range.start == definition_at + 0x12)
                    .unwrap()
                    .binding
                    .range
                    .len(),
                2
            );
        }
        if definition_at == 5180 {
            let position = details.nodes[definition]
                .fields
                .iter()
                .find(|field| field.name == "节点位移增量 XYZ")
                .unwrap();
            assert_eq!(position.read(&details.buffers).unwrap(), "-0, 0, 0");
            assert!(
                position
                    .write(&details.buffers, "-0, 0, 0")
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                &position.binding.bytes(&details.buffers).unwrap()[..4],
                &0x8000_0000u32.to_le_bytes()
            );
            let node = &details.nodes[definition];
            let mut covered = [false; 180];
            for field in &node.fields {
                if field.binding.range.start >= node.range.start
                    && field.binding.range.start + field.binding.range.len() <= node.range.end
                {
                    let start = field.binding.range.start - node.range.start;
                    assert!(
                        covered[start..start + field.binding.range.len()]
                            .iter()
                            .all(|value| !value)
                    );
                    covered[start..start + field.binding.range.len()].fill(true);
                }
            }
            assert!(covered.into_iter().all(|value| value));
            assert_eq!(
                node.fields
                    .iter()
                    .find(|field| field.binding.range.start == definition_at + 0x12)
                    .unwrap()
                    .binding
                    .range
                    .len(),
                2
            );
            assert_eq!(
                node.fields
                    .iter()
                    .find(|field| field.binding.range.start == definition_at + 0x26)
                    .unwrap()
                    .binding
                    .range
                    .len(),
                2
            );
            assert_eq!(
                node.fields
                    .iter()
                    .find(|field| field.binding.range.start == definition_at + 0x94)
                    .unwrap()
                    .binding
                    .range
                    .len(),
                2
            );
        }
    }
    let document = inspect("mhfdat.bin", broken.into());
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(dat::DATA_TABLES.len() + 2))
        .unwrap();
    let document = expand(&document, table).unwrap();
    let binding = document.nodes[table].children[1];
    let document = expand(&document, binding).unwrap();
    assert!(
        document.nodes[binding]
            .error
            .as_deref()
            .unwrap()
            .contains("65535")
    );
    assert!(document.nodes[binding].children.is_empty());
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; validates the original DAT catalog and deferred fields"]
fn original_dat_catalog_and_record_expansion() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
    let document = inspect("mhfdat.bin", source.into());
    assert!(
        document.nodes.iter().all(|node| node.error.is_none()),
        "{:?}",
        document
            .nodes
            .iter()
            .filter_map(|node| node.error.as_ref())
            .collect::<Vec<_>>()
    );
    let dat = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Dat)
        .unwrap();
    let file = Dat::parse(document.bytes(dat).unwrap()).unwrap();
    let mut text_records = 0;
    for layout in TEXT_TABLES {
        let table = file.table(layout).unwrap();
        let RecordFormat::Text { offset, parts } = layout.format else {
            panic!()
        };
        for record in 0..table.count {
            let (start, _) = table.record(record).unwrap();
            for part in 0..usize::from(parts) {
                file.text(start + usize::from(offset) + part * 4).unwrap();
            }
        }
        text_records += table.count;
    }
    println!(
        "{} text tables, {text_records} records, {} data tables",
        TEXT_TABLES.len(),
        dat::DATA_TABLES.len()
    );
    for table_index in [0, 5, 6, 7, 8, 10, 22, 25, 26, 27, 28] {
        let node = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::DatTable(table_index))
            .unwrap();
        let expanded = expand(&document, node).unwrap();
        assert!(expanded.nodes.iter().all(|node| node.error.is_none()));
        let records = &expanded.nodes[node].children;
        for &record in &[records[1], *records.last().unwrap()] {
            let details = expand(&expanded, record).unwrap();
            assert!(details.nodes[record].error.is_none());
            assert!(details.nodes[record].fields.len() > 5);
        }
    }
}
