use mhf_resource::{
    binary::Reader,
    inf::{self, Inf, QuestLayout},
};

const LAYOUT: QuestLayout = QuestLayout {
    root_field: 20,
    count_root_field: 16,
    category_stride: 8,
    category_count_field: 2,
    category_records_field: 4,
    record_text_field: 40,
    record_id_field: 46,
    parts: 8,
};

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn short(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn sample() -> Vec<u8> {
    let mut bytes = vec![0xa5; 640];
    bytes[..inf::HEADER_SIZE].fill(0);
    bytes[..4].copy_from_slice(inf::MAGIC);
    word(&mut bytes, 4, inf::VERSION);
    word(&mut bytes, 8, 0xdead_beef);
    word(&mut bytes, 12, inf::HEADER_SIZE as u32);
    word(&mut bytes, 16, 137);
    word(&mut bytes, 20, 151);
    short(&mut bytes, 137, 3);
    for (index, limit, count, slots) in [(0, 100, 4, 200), (1, 200, 4, 216), (2, 0, 0, u32::MAX)] {
        let at = 151 + index * 8;
        short(&mut bytes, at, limit);
        short(&mut bytes, at + 2, count);
        word(&mut bytes, at + 4, slots);
    }
    for (index, offset) in [0, 300, 300, 400, 0, 400, 0, 300].into_iter().enumerate() {
        word(&mut bytes, 200 + index * 4, offset);
    }
    for (offset, id, texts) in [(300, 1, 360), (400, 101, 450)] {
        word(&mut bytes, offset + 40, texts);
        short(&mut bytes, offset + 46, id);
    }
    for table in [360, 450] {
        for (part, offset) in [500, 510, 0, 500, 520, 530, 540, 550]
            .into_iter()
            .enumerate()
        {
            word(&mut bytes, table + part * 4, offset);
        }
    }
    bytes[500..505].copy_from_slice(&[0x82, 0xa0, 0x82, 0xa2, 0]);
    bytes[510..514].copy_from_slice(b"abc\0");
    bytes[520] = 0;
    bytes[530..533].copy_from_slice(&[0xff, 0x81, 0]);
    bytes[540..542].copy_from_slice(b"x\0");
    bytes[550..552].copy_from_slice(b"y\0");
    bytes
}

#[test]
fn categories_keep_physical_slots_aliases_unknown_bytes_and_image_relative_references() {
    let bytes = sample();
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert_eq!(file.unknown_08, 0xdead_beef);
    assert_eq!(file.category_count().unwrap(), 3);
    assert_eq!(file.category_range().unwrap(), 151..175);
    let category = file.category(0).unwrap();
    assert_eq!(category.quest_id_limit, 100);
    assert_eq!(category.slot_count, 4);
    assert_eq!(category.slots_offset, 200);
    assert_eq!(category.as_bytes(), &bytes[151..159]);
    assert_eq!(file.slots_range(&category).unwrap(), 200..216);
    assert_eq!(file.slot(&category, 0).unwrap().offset, 0);
    let first = file.slot(&category, 1).unwrap();
    let alias = file.slot(&category, 2).unwrap();
    assert_eq!((first.field, alias.field), (204, 208));
    assert_eq!(first.offset, alias.offset);
    let quest = file.quest(first.offset).unwrap();
    let same = file.quest(alias.offset).unwrap();
    assert_eq!(quest.prefix(), &bytes[300..348]);
    assert!(std::ptr::eq(
        quest.prefix().as_ptr(),
        same.prefix().as_ptr()
    ));
    assert_eq!(quest.quest_id, 1);
    assert_eq!(quest.text_table_offset, 360);
    assert_eq!(file.text_table_range(&quest).unwrap(), Some(360..392));
    let text = file.text(&quest, 0).unwrap().unwrap();
    let alias = file.text(&quest, 3).unwrap().unwrap();
    assert_eq!((text.pointer_field, text.offset), (360, 500));
    assert_eq!(text.bytes, &bytes[500..504]);
    assert!(std::ptr::eq(text.bytes.as_ptr(), alias.bytes.as_ptr()));
    assert!(file.text(&quest, 2).unwrap().is_none());
    assert!(file.text(&quest, 4).unwrap().unwrap().bytes.is_empty());
    assert_eq!(file.text(&quest, 5).unwrap().unwrap().bytes, &[0xff, 0x81]);
    assert_eq!(file.as_bytes(), bytes);
    assert_eq!(&file.as_bytes()[600..], &[0xa5; 40]);
}

#[test]
fn lookup_uses_the_first_limit_modulo_slot_and_zero_limit_fallback_without_sorting() {
    let mut bytes = sample();
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert!(file.lookup(0).unwrap().is_none());
    assert_eq!(file.lookup(1).unwrap().unwrap().offset, 300);
    assert_eq!(file.lookup(2).unwrap().unwrap().offset, 300);
    assert_eq!(file.lookup(101).unwrap().unwrap().offset, 400);
    assert_eq!(file.lookup(201).unwrap_err().offset, 169);
    assert!(file.lookup(40000).unwrap().is_none());
    assert!(file.lookup(u16::MAX).unwrap().is_none());
    assert_eq!(file.lookup(104).unwrap_err().offset, 161);
    // Unsorted or overlapping limits are not normalized: the first match wins.
    short(&mut bytes, 151, 200);
    short(&mut bytes, 159, 100);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert_eq!(file.lookup(101).unwrap().unwrap().offset, 300);
    // A zero limit stops the scan but still reads this category's slot table.
    short(&mut bytes, 151, 0);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert_eq!(file.category_count().unwrap(), 3);
    assert_eq!(file.category(1).unwrap().slot_count, 4);
    assert_eq!(file.lookup(101).unwrap().unwrap().offset, 300);
    assert_eq!(file.lookup(104).unwrap_err().offset, 153);

    let mut bytes = sample();
    short(&mut bytes, 169, 4);
    word(&mut bytes, 171, 232);
    for (index, offset) in [0, 400, 0, 300].into_iter().enumerate() {
        word(&mut bytes, 232 + index * 4, offset);
    }
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert_eq!(file.lookup(201).unwrap().unwrap().offset, 400);
    assert!(file.lookup(202).unwrap().is_none());
    // A complete-looking fallback beyond the declared count is not followed.
    short(&mut bytes, 137, 2);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert_eq!(file.lookup(201).unwrap_err().offset, 20);
}

#[test]
fn empty_tables_preserve_stale_offsets_and_null_text_tables_stay_absent() {
    let mut bytes = sample();
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    let empty = file.category(2).unwrap();
    assert_eq!(empty.slots_offset, u32::MAX);
    assert_eq!(file.slots_range(&empty).unwrap(), 0..0);
    assert!(file.slot(&empty, 0).is_err());
    word(&mut bytes, 340, 0);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    let quest = file.quest(300).unwrap();
    assert!(file.text_table_range(&quest).unwrap().is_none());
    assert!(file.text(&quest, 0).unwrap().is_none());
    assert!(file.text(&quest, 8).is_err());
    short(&mut bytes, 137, 0);
    word(&mut bytes, 20, u32::MAX);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert_eq!(file.category_range().unwrap(), 0..0);
    assert_eq!(file.lookup(1).unwrap_err().offset, 20);
    word(&mut bytes, 16, 0);
    assert_eq!(
        Inf::parse(&bytes, LAYOUT)
            .unwrap()
            .category_count()
            .unwrap(),
        0
    );
}

#[test]
fn invalid_references_fail_locally_without_hiding_other_slots_and_strings() {
    let mut bytes = sample();
    word(&mut bytes, 204, u32::MAX);
    word(&mut bytes, 364, 16);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    let category = file.category(0).unwrap();
    assert_eq!(file.slot(&category, 1).unwrap().offset, u32::MAX);
    assert!(file.lookup(1).is_err());
    let quest = file.lookup(2).unwrap().unwrap();
    assert_eq!(quest.quest_id, 1);
    assert_eq!(file.text(&quest, 1).unwrap_err().offset, 364);
    assert_eq!(
        file.text(&quest, 0).unwrap().unwrap().bytes,
        &bytes[500..504]
    );
    word(&mut bytes, 360, 600);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    let quest = file.quest(300).unwrap();
    assert_eq!(file.text(&quest, 0).unwrap_err().offset, 600);
    assert_eq!(file.text(&quest, 3).unwrap().unwrap().offset, 500);
    word(&mut bytes, 340, 620);
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    let quest = file.quest(300).unwrap();
    assert_eq!(file.text_table_range(&quest).unwrap_err().offset, 340);
    assert_eq!(file.lookup(101).unwrap().unwrap().offset, 400);
}

#[test]
fn headers_counts_and_complete_reference_tables_are_bounded_before_iteration() {
    let bytes = sample();
    for end in 0..inf::HEADER_SIZE {
        assert!(Inf::parse(&bytes[..end], LAYOUT).is_err());
    }
    for (field, value, expected) in [(0, 0, 0), (4, 7, 4), (12, 32, 12)] {
        let mut invalid = bytes.clone();
        word(&mut invalid, field, value);
        assert_eq!(Inf::parse(&invalid, LAYOUT).unwrap_err().offset, expected);
    }
    for (field, value) in [(16, u32::MAX), (20, 20), (20, 632)] {
        let mut invalid = bytes.clone();
        word(&mut invalid, field, value);
        let file = Inf::parse(&invalid, LAYOUT).unwrap();
        assert_eq!(file.category_range().unwrap_err().offset, field);
    }
    let mut invalid = bytes.clone();
    short(&mut invalid, 137, u16::MAX);
    assert_eq!(
        Inf::parse(&invalid, LAYOUT)
            .unwrap()
            .category_range()
            .unwrap_err()
            .offset,
        20
    );
    for (field, value) in [(155, 0), (155, 634)] {
        let mut invalid = bytes.clone();
        word(&mut invalid, field, value);
        let file = Inf::parse(&invalid, LAYOUT).unwrap();
        let category = file.category(0).unwrap();
        assert_eq!(file.slot(&category, 0).unwrap_err().offset, field);
    }
    let file = Inf::parse(&bytes, LAYOUT).unwrap();
    assert!(file.category(usize::MAX).is_err());
    assert!(file.slot(&file.category(0).unwrap(), usize::MAX).is_err());
    for offset in [0, 135, 593, u32::MAX] {
        assert!(file.quest(offset).is_err());
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original mhfinf.bin only"]
fn original_inf_categories_records_and_texts_retain_their_source_ranges() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfinf.bin")).unwrap();
    let opened = mhf_resource::container::open_layers(&source, 128 * 1024 * 1024, 8).unwrap();
    let bytes = opened.payload();
    let file = Inf::parse(bytes, LAYOUT).unwrap();
    let mut records = 0;
    let mut texts = 0;
    for index in 0..file.category_count().unwrap() {
        let category = file.category(index).unwrap();
        for slot in 0..usize::from(category.slot_count) {
            let pointer = file.slot(&category, slot).unwrap();
            assert_eq!(
                Reader::new(bytes)
                    .read_at::<u32>(pointer.field)
                    .unwrap()
                    .value,
                pointer.offset
            );
            if pointer.offset == 0 {
                continue;
            }
            let quest = file.quest(pointer.offset).unwrap();
            assert_eq!(
                quest.prefix(),
                &bytes[quest.offset..quest.offset + LAYOUT.quest_prefix_size()]
            );
            records += 1;
            for part in 0..usize::from(LAYOUT.parts) {
                if let Some(text) = file.text(&quest, part).unwrap() {
                    assert_eq!(
                        Reader::new(bytes)
                            .read_at::<u32>(text.pointer_field)
                            .unwrap()
                            .value as usize,
                        text.offset
                    );
                    assert_eq!(
                        text.bytes,
                        &bytes[text.offset..text.offset + text.bytes.len()]
                    );
                    assert_eq!(bytes[text.offset + text.bytes.len()], 0);
                    texts += 1;
                }
            }
        }
    }
    assert!(file.category_count().unwrap() > 0 && records > 0 && texts > 0);
    assert_eq!(file.as_bytes(), bytes);
}
