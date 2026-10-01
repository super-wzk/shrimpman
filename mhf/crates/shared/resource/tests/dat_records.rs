use mhf_resource::binary::{Reader, ScalarType};
use mhf_resource::dat::{self, Dat, RecordCount, RecordFormat, TableLayout};

fn set_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn image(size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[..4].copy_from_slice(dat::MAGIC);
    set_u32(&mut bytes, 4, dat::VERSION);
    set_u32(&mut bytes, 12, dat::HEADER_SIZE as u32);
    bytes
}

#[test]
fn armor_records_keep_signed_values_unknown_bytes_and_short_terminator() {
    let mut bytes = image(3200 + 72 * 2 + 2);
    set_u32(&mut bytes, 0x50, 3200);
    bytes[3200..3202].copy_from_slice(&17u16.to_le_bytes());
    bytes[3200 + 0x14] = (-7i8) as u8;
    bytes[3200 + 0x07] = 0xa5;
    bytes[3200 + 144..].copy_from_slice(&u16::MAX.to_le_bytes());
    let file = Dat::parse(&bytes).unwrap();
    let table = file.table(&dat::DATA_TABLES[0]).unwrap();
    assert_eq!(table.count, 2);
    assert_eq!(table.range, 3200..3344);
    assert_eq!(table.terminator, Some(3344..3346));
    let (offset, record) = table.record(0).unwrap();
    assert_eq!(offset, 3200);
    assert_eq!(record[7], 0xa5);
    let RecordFormat::Fields(fields) = table.layout.format else {
        panic!()
    };
    let fire = fields.iter().find(|field| field.offset == 0x14).unwrap();
    assert_eq!(fire.scalar, ScalarType::I8);
    assert_eq!(
        Reader::new(record)
            .read_at::<i8>(fire.offset.into())
            .unwrap()
            .value,
        -7
    );
    assert!(table.record(2).is_err());
    assert_eq!(file.as_bytes(), bytes);
    assert!(std::ptr::eq(record.as_ptr(), bytes[3200..].as_ptr()));
    bytes.truncate(3344);
    assert!(
        Dat::parse(&bytes)
            .unwrap()
            .table(&dat::DATA_TABLES[0])
            .is_err()
    );
}

#[test]
fn counted_tables_reject_out_of_bounds_counts_and_pointers() {
    let mut bytes = image(3400);
    set_u32(&mut bytes, 0x10, 3100);
    set_u32(&mut bytes, 0xfc, 3200);
    bytes[3108..3110].copy_from_slice(&2u16.to_le_bytes());
    let items = &dat::DATA_TABLES[7];
    let file = Dat::parse(&bytes).unwrap();
    assert_eq!(file.table(items).unwrap().range, 3200..3272);
    bytes[3108..3110].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(Dat::parse(&bytes).unwrap().table(items).is_err());
    set_u32(&mut bytes, 0xfc, u32::MAX);
    assert_eq!(
        Dat::parse(&bytes).unwrap().table(items).unwrap_err().offset,
        0xfc
    );
    set_u32(&mut bytes, 0xfc, 16);
    assert!(Dat::parse(&bytes).unwrap().table(items).is_err());
    set_u32(&mut bytes, 0xfc, 0);
    assert_eq!(Dat::parse(&bytes).unwrap().table(items).unwrap().count, 0);
}

#[test]
fn random_recipe_tables_share_indices_and_keep_signed_ranges_and_flags() {
    let recipes = dat::DATA_TABLES
        .iter()
        .find(|table| table.id == "random_recipes")
        .unwrap();
    let candidates = dat::DATA_TABLES
        .iter()
        .find(|table| table.id == "random_candidates")
        .unwrap();
    let mut bytes = image(3908);
    set_u32(&mut bytes, 0x10, 3100);
    set_u32(&mut bytes, 0x620, 3680);
    set_u32(&mut bytes, 0x624, 3780);
    // The recipe directory enumerates +0x238, and both tables use its index.
    bytes[3100 + 0x238..3100 + 0x23a].copy_from_slice(&2u16.to_le_bytes());
    // +0x23a is deliberately different; it is not the count used by consumers.
    bytes[3100 + 0x23a..3100 + 0x23c].copy_from_slice(&9u16.to_le_bytes());
    bytes[3680..3684].copy_from_slice(&[0xf1, 13, 27, 0xe2]);
    set_u32(&mut bytes, 3684, 45000);
    bytes[3688..3690].copy_from_slice(&(-3i16).to_le_bytes());
    bytes[3690..3692].copy_from_slice(&(-32761i16).to_le_bytes());
    bytes[3780..3788].copy_from_slice(&[0xff, 0xff, 70, 0xa5, 0xf9, 0xff, 11, 0]);

    let file = Dat::parse(&bytes).unwrap();
    let recipe_table = file.table(recipes).unwrap();
    let candidate_table = file.table(candidates).unwrap();
    assert_eq!(recipe_table.count, 2);
    assert_eq!(candidate_table.count, 2);
    assert_eq!(recipe_table.range, 3680..3760);
    assert_eq!(candidate_table.range, 3780..3908);
    for (layout, expected) in [
        (
            recipes,
            vec![
                ("field_01", 1, ScalarType::U8),
                ("field_0a", 10, ScalarType::I16),
            ],
        ),
        (
            candidates,
            vec![
                ("field_02", 2, ScalarType::U8),
                ("field_03", 3, ScalarType::U8),
                ("field_04", 4, ScalarType::I16),
                ("field_3e", 62, ScalarType::I16),
            ],
        ),
    ] {
        let RecordFormat::Fields(fields) = layout.format else {
            panic!()
        };
        for (key, offset, scalar) in expected {
            let field = fields.iter().find(|field| field.key == key).unwrap();
            assert_eq!((field.offset, field.scalar), (offset, scalar));
        }
    }
    let (_, recipe) = recipe_table.record(0).unwrap();
    assert_eq!(
        Reader::new(recipe).read_at::<i16>(10).unwrap().value,
        -32761
    );
    let (_, candidate) = candidate_table.record(0).unwrap();
    assert_eq!(
        Reader::new(candidate).read_at::<u16>(0).unwrap().value,
        u16::MAX
    );
    assert_eq!(candidate[3], 0xa5);
    assert_eq!(Reader::new(candidate).read_at::<i16>(4).unwrap().value, -7);
    assert_eq!(file.as_bytes(), bytes);

    bytes.truncate(3907);
    assert!(Dat::parse(&bytes).unwrap().table(candidates).is_err());
    set_u32(&mut bytes, 0x620, u32::MAX);
    assert_eq!(
        Dat::parse(&bytes)
            .unwrap()
            .table(recipes)
            .unwrap_err()
            .offset,
        0x620
    );
    set_u32(&mut bytes, 0x620, 3680);
    bytes[3100 + 0x238..3100 + 0x23a].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(Dat::parse(&bytes).unwrap().table(recipes).is_err());
    assert!(Dat::parse(&bytes).unwrap().table(candidates).is_err());
}

#[test]
fn motion_event_group_keys_use_a_scalar_root_count() {
    let layout = dat::DATA_TABLES
        .iter()
        .find(|table| table.id == "motion_event_group_keys")
        .unwrap();
    let mut bytes = image(3216);
    set_u32(&mut bytes, 664 * 4, 3200);
    set_u32(&mut bytes, 665 * 4, 2);
    bytes[3200..3208].copy_from_slice(&[0x93, 0, 0x9b, 0, 0xff, 0xff, 0, 0]);
    let file = Dat::parse(&bytes).unwrap();
    let table = file.table(layout).unwrap();
    assert_eq!(table.count, 2);
    assert_eq!(table.range, 3200..3216);
    let (_, record) = table.record(0).unwrap();
    assert_eq!(
        Reader::new(record).read_at::<u16>(4).unwrap().value,
        u16::MAX
    );
    assert!(table.record(2).is_err());
    assert_eq!(file.as_bytes(), bytes);
    set_u32(&mut bytes, 665 * 4, u32::MAX);
    assert!(Dat::parse(&bytes).unwrap().table(layout).is_err());
    set_u32(&mut bytes, 665 * 4, 0);
    set_u32(&mut bytes, 664 * 4, 0);
    assert_eq!(Dat::parse(&bytes).unwrap().table(layout).unwrap().count, 0);
}

#[test]
fn bow_shot_stages_start_inside_the_root_object_and_require_complete_rows() {
    let layout = dat::DATA_TABLES
        .iter()
        .find(|table| table.id == "bow_shot_stages")
        .unwrap();
    let root = 3200;
    let first = root + 0x21c;
    let end = first + 8 * 0x28;
    let mut bytes = image(end);
    set_u32(&mut bytes, 408 * 4, root as u32);
    bytes[first + 0x18..first + 0x1c].copy_from_slice(&0.4f32.to_le_bytes());
    bytes[first + 0x1c..first + 0x20].copy_from_slice(&0.7f32.to_le_bytes());
    let last = first + 7 * 0x28;
    bytes[last + 0x18..last + 0x1c].copy_from_slice(&2.0f32.to_le_bytes());
    bytes[last + 0x1c..last + 0x20].copy_from_slice(&1.5f32.to_le_bytes());
    bytes[last + 0x04] = 0xa5;

    let file = Dat::parse(&bytes).unwrap();
    let table = file.table(layout).unwrap();
    assert_eq!(table.range, first..end);
    assert_eq!(table.count, 8);
    let (offset, stage0) = table.record(0).unwrap();
    assert_eq!(offset, first);
    assert_eq!(Reader::new(stage0).read_at::<f32>(0x18).unwrap().value, 0.4);
    assert_eq!(Reader::new(stage0).read_at::<f32>(0x1c).unwrap().value, 0.7);
    let (offset, stage7) = table.record(7).unwrap();
    assert_eq!(offset, last);
    assert_eq!(Reader::new(stage7).read_at::<f32>(0x18).unwrap().value, 2.0);
    assert_eq!(Reader::new(stage7).read_at::<f32>(0x1c).unwrap().value, 1.5);
    assert_eq!(stage7[0x04], 0xa5);
    assert!(table.record(8).is_err());

    bytes.pop();
    assert!(Dat::parse(&bytes).unwrap().table(layout).is_err());
}

static TEXT: TableLayout = TableLayout {
    id: "text",
    label: "text",
    root: &[0x100],
    start_offset: 0,
    first_record: 24,
    records: RecordCount::U16(&[0x10, 8]),
    stride: 4,
    format: RecordFormat::Text {
        offset: 0,
        parts: 1,
    },
    directory: None,
    names: None,
};

#[test]
fn text_uses_image_relative_offsets_and_preserves_null_empty_and_invalid_encoding() {
    let mut bytes = image(3350);
    set_u32(&mut bytes, 0x10, 3100);
    set_u32(&mut bytes, 0x100, 3200);
    bytes[3108..3110].copy_from_slice(&3u16.to_le_bytes());
    set_u32(&mut bytes, 3296, 3340);
    set_u32(&mut bytes, 3300, 3344);
    bytes[3340..3344].copy_from_slice(&[0xff, 0x81, 0x40, 0]);
    let file = Dat::parse(&bytes).unwrap();
    let table = file.table(&TEXT).unwrap();
    assert_eq!(table.range, 3296..3308);
    assert_eq!(file.text(3296).unwrap(), Some((3340, &bytes[3340..3343])));
    assert_eq!(file.text(3300).unwrap(), Some((3344, &b""[..])));
    assert_eq!(file.text(3304).unwrap(), None);
    bytes[3340..].fill(0x81);
    assert!(Dat::parse(&bytes).unwrap().text(3296).is_err());
}

#[test]
fn absent_directory_entries_are_checked_before_dereferencing() {
    static GUARDED: TableLayout = TableLayout {
        id: "guard",
        label: "guard",
        root: &[0x100, 4],
        start_offset: 0,
        first_record: 0,
        records: RecordCount::Fixed(1),
        stride: 4,
        format: RecordFormat::Text {
            offset: 0,
            parts: 1,
        },
        directory: Some((1, RecordCount::U32(&[0x104]))),
        names: None,
    };
    let mut bytes = image(dat::HEADER_SIZE);
    set_u32(&mut bytes, 0x100, u32::MAX);
    set_u32(&mut bytes, 0x104, 1);
    assert_eq!(
        Dat::parse(&bytes).unwrap().table(&GUARDED).unwrap().count,
        0
    );
    set_u32(&mut bytes, 0x104, 2);
    assert!(Dat::parse(&bytes).unwrap().table(&GUARDED).is_err());
}

#[test]
fn header_validation_and_schema_fields_are_bounded() {
    let bytes = image(dat::HEADER_SIZE);
    // Header +8 is opaque, even when it resembles an invalid root pointer.
    for unknown in [8, dat::HEADER_SIZE as u32, u32::MAX] {
        let mut source = bytes.clone();
        set_u32(&mut source, 8, unknown);
        let file = Dat::parse(&source).unwrap();
        assert_eq!(file.unknown_08, unknown);
        assert_eq!(file.as_bytes(), source);
    }
    for end in [0, 3, 4, 7, 12, dat::HEADER_SIZE - 1] {
        assert!(Dat::parse(&bytes[..end]).is_err());
    }
    let mut wrong = bytes.clone();
    set_u32(&mut wrong, 4, 90);
    assert_eq!(Dat::parse(&wrong).unwrap_err().offset, 4);
    set_u32(&mut wrong, 4, dat::VERSION);
    set_u32(&mut wrong, 12, 16);
    assert_eq!(Dat::parse(&wrong).unwrap_err().offset, 12);
    for layout in dat::DATA_TABLES {
        let RecordFormat::Fields(fields) = layout.format else {
            panic!()
        };
        let mut used = vec![false; usize::from(layout.stride)];
        for field in fields {
            let start = usize::from(field.offset);
            let end = start + field.scalar.size();
            assert!(end <= used.len(), "{}: {}", layout.id, field.name);
            assert!(used[start..end].iter().all(|value| !value));
            used[start..end].fill(true);
        }
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original mhfdat.bin only"]
fn original_dat_core_tables_and_all_record_fields() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let bytes = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
    let opened = mhf_resource::container::open_layers(&bytes, usize::MAX, 16).unwrap();
    let file = Dat::parse(opened.payload()).unwrap();
    let expected = [14594, 13462, 13452, 13708, 13514, 17568, 4223, 16701];
    for (index, layout) in dat::DATA_TABLES.iter().enumerate() {
        let table = file.table(layout).unwrap();
        if let Some(&count) = expected.get(index) {
            assert_eq!(table.count, count, "{}", layout.id);
        }
        let RecordFormat::Fields(fields) = layout.format else {
            panic!()
        };
        for record in 0..table.count {
            let (_, bytes) = table.record(record).unwrap();
            for field in fields {
                let at = usize::from(field.offset);
                assert!(bytes.get(at..at + field.scalar.size()).is_some());
            }
        }
        println!(
            "{}: {} records x {} bytes",
            layout.id, table.count, layout.stride
        );
    }
    let item = file.table(&dat::DATA_TABLES[7]).unwrap();
    let (_, book) = item.record(1).unwrap();
    assert_eq!(u32::from_le_bytes(book[12..16].try_into().unwrap()), 1000);
    assert_eq!(u32::from_le_bytes(book[16..20].try_into().unwrap()), 100);
    let bow = dat::DATA_TABLES
        .iter()
        .find(|table| table.id == "bow_shot_stages")
        .unwrap();
    let stages = file.table(bow).unwrap();
    assert_eq!(stages.count, 8);
    let multipliers = [
        [0.4f32, 0.7, 0.6, 0.7],
        [1.0, 0.95, 1.1, 1.2],
        [1.5, 1.2, 1.1, 1.5],
        [1.85, 1.34, 1.25, 1.7],
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
        [2.0, 1.5, 1.5, 1.5],
    ];
    for (index, expected) in multipliers.into_iter().enumerate() {
        let (_, record) = stages.record(index).unwrap();
        let reader = Reader::new(record);
        for (offset, value) in [0x18, 0x1c, 0x20, 0x24].into_iter().zip(expected) {
            assert_eq!(reader.read_at::<f32>(offset).unwrap().value, value);
        }
    }
    let shot = dat::DATA_TABLES
        .iter()
        .find(|table| table.id == "shot_event_multipliers")
        .unwrap();
    let shot = file.table(shot).unwrap();
    assert_eq!(shot.count, 1);
    let (_, record) = shot.record(0).unwrap();
    assert_eq!(Reader::new(record).read_at::<f32>(0).unwrap().value, 2.0);
    assert_eq!(Reader::new(record).read_at::<f32>(4).unwrap().value, 0.6);
    use mhf_resource::effect::{
        AttachmentDefinition, AttachmentGroup, ModelEffectBinding, ModelEffectDefinition,
    };
    for (index, layout) in dat::EFFECT_TABLES.iter().enumerate() {
        let table = file.table(layout).unwrap();
        assert_eq!(table.count, [1321, 2194, 1354, 930][index]);
        for record in 0..table.count {
            let (_, bytes) = table.record(record).unwrap();
            match index {
                0 => {
                    let parsed = AttachmentGroup::parse(bytes).unwrap();
                    assert_eq!(parsed.to_bytes(), bytes);
                    for &id in parsed.active_definition_ids() {
                        assert!(usize::from(id) < 2194);
                    }
                }
                1 => assert_eq!(
                    AttachmentDefinition::parse(bytes).unwrap().to_bytes(),
                    bytes
                ),
                2 => {
                    let parsed = ModelEffectBinding::parse(bytes).unwrap();
                    assert_eq!(parsed.to_bytes(), bytes);
                    for &id in parsed.active_definition_ids() {
                        assert!(usize::from(id) < 930);
                    }
                }
                3 => assert_eq!(
                    ModelEffectDefinition::parse(bytes).unwrap().to_bytes(),
                    bytes
                ),
                _ => unreachable!(),
            }
        }
        println!(
            "{}: {} records x {} bytes",
            layout.id, table.count, layout.stride
        );
    }
}
