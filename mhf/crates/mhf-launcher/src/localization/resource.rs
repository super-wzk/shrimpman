use super::{
    HookState, MemoryRange, QuestTableLayout, RESOURCE_LAYOUTS, RecordCount, RecordTableLayout,
    ResourceBodyLayout, TextArena, TranslationKey, replacement_for_key,
};
use std::{ffi::CStr, mem::size_of, ptr, sync::PoisonError};

const PAC_TEXT_CACHE_RVA: usize = 0x0154_2BC0;

pub(super) unsafe fn validate_sources(base: usize) -> Result<(), String> {
    for (rva, expected) in [
        (0x00AF_BF22, &[0xE8, 0x99, 0x6C, 0xA4, 0][..]),
        (PAC_TEXT_CACHE_RVA, &[0x56, 0x8B, 0x35][..]),
        (
            PAC_TEXT_CACHE_RVA + 7,
            &[0x8B, 0x86, 0xE8, 0x09, 0, 0, 0x8B, 0x08, 0x8B, 0x50, 0x04][..],
        ),
        (
            0x0154_2C51,
            &[0x8B, 0x8E, 0x2C, 0x06, 0, 0, 0x8B, 0x51, 0x4C][..],
        ),
    ] {
        if unsafe { std::slice::from_raw_parts((base + rva) as *const u8, expected.len()) }
            != expected
        {
            return Err(format!(
                "unsupported PAC text-cache boundary at RVA {rva:#x}"
            ));
        }
    }
    if unsafe { ptr::read_unaligned((base + PAC_TEXT_CACHE_RVA + 3) as *const u32) } as usize
        != base + 0x0E77_DCCC
    {
        return Err("PAC text-cache helper references an unsupported resource global".to_owned());
    }
    Ok(())
}

pub(super) unsafe fn patch_image(state: &HookState, resource_id: &'static str, image: MemoryRange) {
    let Some(layout) = RESOURCE_LAYOUTS
        .iter()
        .find(|layout| layout.id == resource_id)
    else {
        return;
    };
    if let Some((magic, version)) = layout.identity
        && (unsafe { read_image_u32(image, 0) } != Some(relocated_resource_magic(magic))
            || unsafe { read_image_u32(image, 4) } != Some(version))
    {
        return;
    }
    let code_page = match layout
        .code_page
        .map_or_else(|| state.language_code_page(), Ok)
    {
        Ok(code_page) => code_page,
        Err(error) => {
            eprintln!("failed to load resource {}: {error}", layout.id);
            return;
        }
    };

    let mut text = state.text.lock().unwrap_or_else(PoisonError::into_inner);

    match layout.body {
        ResourceBodyLayout::Records(tables) => {
            for table in tables {
                unsafe {
                    patch_record_table(state, &mut text, layout.id, image, *table, code_page)
                };
            }
        }
        ResourceBodyLayout::Quest(quest) => unsafe {
            patch_quest_table(state, &mut text, layout.id, image, quest, code_page)
        },
    }
    drop(text);
    if resource_id == "mhfpac" {
        // The loader cached six labels before the final relocation boundary.
        // Re-run its original cache builder after conversion, including its
        // state-dependent choice between table_518:4 and table_285:19.
        let rebuild = unsafe {
            std::mem::transmute::<usize, unsafe extern "C" fn()>(
                state.module_base + PAC_TEXT_CACHE_RVA,
            )
        };
        unsafe { rebuild() };
    }
}

unsafe fn patch_record_table(
    state: &HookState,
    text: &mut TextArena,
    resource_id: &'static str,
    image: MemoryRange,
    layout: RecordTableLayout,
    code_page: u32,
) {
    if let Some((index, count)) = layout.directory
        && unsafe { record_count(image, count) }.is_none_or(|count| index >= count)
    {
        return;
    }
    let Some(table_start) = (unsafe { read_pointer_target(image, layout.root) }) else {
        return;
    };
    let Some(records) = (unsafe { record_count(image, layout.records) }) else {
        return;
    };
    if records == 0 {
        return;
    }
    let Some(first_record) = usize::try_from(layout.first_record).ok() else {
        return;
    };
    let Some(text_cells_end) = usize::try_from(records - 1)
        .ok()
        .and_then(|last| first_record.checked_add(last))
        .and_then(|record| record.checked_mul(usize::from(layout.stride)))
        .and_then(|offset| offset.checked_add(usize::from(layout.text_offset)))
        .and_then(|offset| offset.checked_add(usize::from(layout.parts) * size_of::<u32>()))
        .and_then(|size| table_start.checked_add(size))
    else {
        return;
    };
    if text_cells_end > image.end {
        return;
    }

    for record in 0..records {
        let record_start =
            table_start + (first_record + record as usize) * usize::from(layout.stride);
        for part in 0..layout.parts {
            let cell = record_start
                + usize::from(layout.text_offset)
                + usize::from(part) * size_of::<u32>();
            unsafe {
                patch_cell(
                    state,
                    text,
                    image,
                    TranslationKey::Resource {
                        resource_id,
                        group_id: layout.id,
                        translation_group: layout.translation_group,
                        record_id: record,
                        part,
                    },
                    cell,
                    code_page,
                )
            };
        }
    }
}

unsafe fn patch_quest_table(
    state: &HookState,
    text: &mut TextArena,
    resource_id: &'static str,
    image: MemoryRange,
    layout: QuestTableLayout,
    code_page: u32,
) {
    let Some(category_table) = (unsafe { read_pointer_target(image, &[layout.root]) }) else {
        return;
    };
    let Some(count_data) = (unsafe { read_pointer_target(image, &[layout.count_root]) }) else {
        return;
    };
    let Some(category_count) = (unsafe { read_u16_at(image, count_data) }) else {
        return;
    };
    let Some(categories_end) = usize::from(category_count)
        .checked_mul(usize::from(layout.category_stride))
        .and_then(|size| category_table.checked_add(size))
    else {
        return;
    };
    if categories_end > image.end {
        return;
    }

    for category_index in 0..category_count {
        let category =
            category_table + usize::from(category_index) * usize::from(layout.category_stride);
        let Some(record_count) =
            (unsafe { read_u16_at(image, category + usize::from(layout.category_count_field)) })
        else {
            return;
        };
        let Some(records_pointer) =
            (unsafe { read_u32_at(image, category + usize::from(layout.category_records_field)) })
        else {
            return;
        };
        let Some(records) = resolve_image_pointer(image, records_pointer) else {
            continue;
        };
        let Some(records_end) = usize::from(record_count)
            .checked_mul(size_of::<u32>())
            .and_then(|size| records.checked_add(size))
        else {
            return;
        };
        if records_end > image.end {
            return;
        }

        for record_index in 0..record_count {
            let record_cell = records + usize::from(record_index) * size_of::<u32>();
            let Some(record) = unsafe { read_u32_at(image, record_cell) }
                .and_then(|pointer| resolve_image_pointer(image, pointer))
            else {
                continue;
            };
            let Some(quest_id) = record
                .checked_add(usize::from(layout.record_id_field))
                .and_then(|field| unsafe { read_u16_at(image, field) })
                .filter(|quest_id| *quest_id != 0)
            else {
                continue;
            };
            let text_table = record
                .checked_add(usize::from(layout.record_text_field))
                .and_then(|cell| unsafe { read_u32_at(image, cell) })
                .and_then(|pointer| resolve_image_pointer(image, pointer));
            if let Some(text_table) = text_table {
                for part in 0..layout.parts {
                    let cell = text_table + usize::from(part) * size_of::<u32>();
                    unsafe {
                        patch_cell(
                            state,
                            text,
                            image,
                            TranslationKey::Resource {
                                resource_id,
                                group_id: layout.id,
                                translation_group: layout.translation_group,
                                record_id: u32::from(quest_id),
                                part,
                            },
                            cell,
                            code_page,
                        )
                    };
                }
            }
        }
    }
}

unsafe fn patch_cell(
    state: &HookState,
    text: &mut TextArena,
    image: MemoryRange,
    key: TranslationKey,
    cell: usize,
    code_page: u32,
) {
    let Some(source_pointer) = (unsafe { read_u32_at(image, cell) }) else {
        return;
    };
    // Patched pointers target static UTF-8 records or our arena outside this
    // image. Repeated callbacks must not decode those bytes as legacy text.
    // Some raw-resource loaders unconditionally relocate zero offsets to base.
    // The image header is never a string; preserve these original empty slots.
    if source_pointer as usize == image.start || !image.contains(source_pointer as usize) {
        return;
    }
    let Some(source) = (unsafe { read_image_string(image, source_pointer as usize) }) else {
        eprintln!("resource {key} has no NUL terminator inside its source image");
        return;
    };
    match replacement_for_key(state, text, key, source, code_page) {
        Ok(replacement) => {
            let replacement =
                match keyconfig_header(key, unsafe { CStr::from_ptr(replacement.cast()) }) {
                    Some(header) => text.storage.alloc_slice_copy(&header).as_ptr(),
                    None => replacement,
                };
            unsafe { ptr::write_unaligned(cell as *mut u32, replacement as usize as u32) };
        }
        Err(error) => eprintln!("failed to convert resource {key}: {error}"),
    }
}

fn keyconfig_header(key: TranslationKey, header: &CStr) -> Option<Vec<u8>> {
    // 108D75E0 writes PAC root793 record0 as the keyconfig XML header. Its
    // Japanese comment and category/action labels have just become UTF-8; the
    // ASCII encoding declaration must name that same encoding. Other strings,
    // including controller identifiers and XML-like game text, are unaffected.
    if !matches!(
        key,
        TranslationKey::Resource {
            resource_id: "mhfpac",
            group_id: "table_793",
            record_id: 0,
            part: 0,
            ..
        }
    ) {
        return None;
    }
    let body = header
        .to_bytes_with_nul()
        .strip_prefix(b"<?xml version=\"1.0\" encoding=\"Shift_JIS\" ?>\r\n")?;
    let mut output = b"<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n".to_vec();
    output.extend_from_slice(body);
    Some(output)
}

pub(super) unsafe fn read_image_string<'a>(image: MemoryRange, address: usize) -> Option<&'a CStr> {
    if !image.contains(address) {
        return None;
    }
    let remaining =
        unsafe { std::slice::from_raw_parts(address as *const u8, image.end - address) };
    CStr::from_bytes_until_nul(remaining).ok()
}

unsafe fn field_address(image: MemoryRange, path: &[u32]) -> Option<usize> {
    let (field, parents) = path.split_last()?;
    let mut base = image.start;
    for offset in parents {
        let pointer = unsafe { read_u32_at(image, base.checked_add(*offset as usize)?) }?;
        base = resolve_image_pointer(image, pointer)?;
    }
    let address = base.checked_add(*field as usize)?;
    image.contains(address).then_some(address)
}

unsafe fn read_pointer_target(image: MemoryRange, path: &[u32]) -> Option<usize> {
    let field = unsafe { field_address(image, path) }?;
    let pointer = unsafe { read_u32_at(image, field) }?;
    resolve_image_pointer(image, pointer)
}

unsafe fn record_count(image: MemoryRange, count: RecordCount) -> Option<u32> {
    match count {
        RecordCount::Fixed(count) => Some(count),
        RecordCount::U16(path) => {
            let field = unsafe { field_address(image, path) }?;
            unsafe { read_u16_at(image, field) }.map(u32::from)
        }
        RecordCount::U32(path) => {
            let field = unsafe { field_address(image, path) }?;
            unsafe { read_u32_at(image, field) }
        }
        RecordCount::Sentinel {
            root,
            stride,
            offset,
            width,
            value,
        } => {
            let table = unsafe { read_pointer_target(image, root) }?;
            let mut field = table.checked_add(offset as usize)?;
            let mut count = 0;
            loop {
                let current = match width {
                    2 => unsafe { read_u16_at(image, field) }.map(u32::from)?,
                    4 => unsafe { read_u32_at(image, field) }?,
                    _ => return None,
                };
                if current == value {
                    return Some(count);
                }
                field = field.checked_add(stride as usize)?;
                count += 1;
            }
        }
    }
}

unsafe fn read_image_u32(image: MemoryRange, offset: usize) -> Option<u32> {
    let address = image.start.checked_add(offset)?;
    unsafe { read_u32_at(image, address) }
}

unsafe fn read_u16_at(image: MemoryRange, address: usize) -> Option<u16> {
    (image.contains(address) && address.checked_add(size_of::<u16>())? <= image.end)
        .then(|| unsafe { ptr::read_unaligned(address as *const u16) })
}

unsafe fn read_u32_at(image: MemoryRange, address: usize) -> Option<u32> {
    (image.contains(address) && address.checked_add(size_of::<u32>())? <= image.end)
        .then(|| unsafe { ptr::read_unaligned(address as *const u32) })
}

fn resolve_image_pointer(image: MemoryRange, pointer: u32) -> Option<usize> {
    if pointer == 0 {
        return None;
    }
    let address = pointer as usize;
    image.contains(address).then_some(address)
}

fn relocated_resource_magic(magic: u32) -> u32 {
    magic & 0x00FF_FFFF
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::{
        MemoryRange, QuestTableLayout, RecordCount, RecordTableLayout, patch_quest_table,
        patch_record_table, read_image_string, relocated_resource_magic, resolve_image_pointer,
    };
    use crate::{MissingTranslation, localization::test_state};

    fn pointer(buffer: &mut [u8], offset: usize, target: usize) {
        buffer[offset..offset + 4].copy_from_slice(&(target as u32).to_le_bytes());
    }

    fn read_pointer(buffer: &[u8], offset: usize) -> usize {
        u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn record_counts_follow_header_paths_and_metadata_sentinels() {
        let mut buffer = vec![0u8; 128];
        let base = buffer.as_ptr() as usize;
        let image = MemoryRange {
            start: base,
            end: base + buffer.len(),
        };
        pointer(&mut buffer, 0, base + 32);
        buffer[38..40].copy_from_slice(&2u16.to_le_bytes());
        pointer(&mut buffer, 4, base + 64);
        buffer[64..66].copy_from_slice(&1u16.to_le_bytes());
        buffer[72..74].copy_from_slice(&2u16.to_le_bytes());
        buffer[80..82].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(
            unsafe { super::record_count(image, RecordCount::U16(&[0, 6])) },
            Some(2)
        );
        let count = RecordCount::Sentinel {
            root: &[4],
            stride: 8,
            offset: 0,
            width: 2,
            value: u16::MAX as u32,
        };
        assert_eq!(unsafe { super::record_count(image, count) }, Some(2));
        buffer[64..66].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(unsafe { super::record_count(image, count) }, Some(0));
        buffer[64..].fill(0);
        assert_eq!(unsafe { super::record_count(image, count) }, None);
    }

    #[test]
    fn table_segments_keep_their_own_zero_based_translation_ids() {
        let state = test_state(None, MissingTranslation::Key);
        let mut text = state.text.lock().unwrap();
        let mut buffer = vec![0u8; 128];
        let base = buffer.as_ptr() as usize;
        pointer(&mut buffer, 0, base + 16);
        for cell in [16, 20, 24, 28] {
            pointer(&mut buffer, cell, base + 64);
        }
        buffer[64..71].copy_from_slice(b"\x83\x65\x83\x58\x83\x67\0");
        let image = MemoryRange {
            start: base,
            end: base + buffer.len(),
        };
        let messages = RecordTableLayout {
            id: "item_messages",
            translation_group: 1,
            root: &[0],
            first_record: 0,
            records: RecordCount::Fixed(2),
            text_offset: 0,
            parts: 1,
            stride: 4,
            directory: None,
        };
        let descriptions = RecordTableLayout {
            id: "item_descriptions",
            translation_group: 2,
            first_record: 2,
            ..messages
        };
        unsafe {
            patch_record_table(&state, &mut text, "mhfdat", image, messages, 932);
            patch_record_table(&state, &mut text, "mhfdat", image, descriptions, 932);
        }
        for (cell, expected) in [
            (16, "[mhfdat:item_messages:0]"),
            (20, "[mhfdat:item_messages:1]"),
            (24, "[mhfdat:item_descriptions:0]"),
            (28, "[mhfdat:item_descriptions:1]"),
        ] {
            assert_eq!(
                unsafe { CStr::from_ptr(read_pointer(&buffer, cell) as *const _) }
                    .to_str()
                    .unwrap(),
                expected
            );
        }
    }

    #[test]
    fn nested_tables_obey_outer_counts_and_preserve_relocated_empty_slots() {
        let state = test_state(None, MissingTranslation::Original);
        let mut text = state.text.lock().unwrap();
        let mut buffer = vec![0u8; 256];
        let base = buffer.as_ptr() as usize;
        let image = MemoryRange {
            start: base,
            end: base + buffer.len(),
        };
        pointer(&mut buffer, 0, base + 32);
        pointer(&mut buffer, 4, 1);
        pointer(&mut buffer, 32, base + 64);
        pointer(&mut buffer, 36, 2);
        pointer(&mut buffer, 40, base + 80);
        pointer(&mut buffer, 44, 1);
        pointer(&mut buffer, 64, base + 128);
        pointer(&mut buffer, 68, base);
        pointer(&mut buffer, 80, base + 128);
        buffer[128..135].copy_from_slice(b"\x83\x65\x83\x58\x83\x67\0");
        let layout = RecordTableLayout {
            id: "nested",
            translation_group: 1,
            root: &[0, 0],
            first_record: 0,
            records: RecordCount::U32(&[0, 4]),
            text_offset: 0,
            parts: 1,
            stride: 4,
            directory: Some((0, RecordCount::U32(&[4]))),
        };
        unsafe { patch_record_table(&state, &mut text, "sample", image, layout, 932) };
        let translated = read_pointer(&buffer, 64);
        assert_eq!(
            unsafe { CStr::from_ptr(translated as *const _) }
                .to_str()
                .unwrap(),
            "テスト"
        );
        assert_eq!(read_pointer(&buffer, 68), base);
        let inactive = RecordTableLayout {
            root: &[0, 8],
            first_record: 0,
            records: RecordCount::U32(&[0, 12]),
            directory: Some((1, RecordCount::U32(&[4]))),
            ..layout
        };
        unsafe { patch_record_table(&state, &mut text, "sample", image, inactive, 932) };
        assert_eq!(read_pointer(&buffer, 80), base + 128);
        unsafe { patch_record_table(&state, &mut text, "sample", image, layout, 932) };
        assert_eq!(read_pointer(&buffer, 64), translated);
    }

    #[test]
    fn headerless_resources_use_fixed_encoding_and_the_shared_translation_policy() {
        for (locale, missing, expected) in [
            (None, MissingTranslation::Original, "テスト"),
            (None, MissingTranslation::Empty, ""),
            (None, MissingTranslation::Key, "[mhfmsx:treasure_colors:0]"),
            (Some("ja-JP"), MissingTranslation::Original, "赤"),
        ] {
            let locale = locale.map(|id| super::super::TRANSLATION_DICTIONARY.locale(id).unwrap());
            let state = test_state(locale, missing);
            let mut buffer = vec![0u8; 512];
            let base = buffer.as_ptr() as usize;
            pointer(&mut buffer, 28, base + 128);
            pointer(&mut buffer, 132, base + 400);
            buffer[400..407].copy_from_slice(b"\x83\x65\x83\x58\x83\x67\0");
            unsafe {
                super::patch_image(
                    &state,
                    "mhfmsx",
                    MemoryRange {
                        start: base,
                        end: base + buffer.len(),
                    },
                )
            };
            let pointer = read_pointer(&buffer, 132);
            assert_eq!(
                unsafe { CStr::from_ptr(pointer as *const _) }
                    .to_str()
                    .unwrap(),
                expected
            );
        }
    }

    #[test]
    fn converts_all_declared_record_parts_without_a_translation_locale() {
        let state = test_state(None, MissingTranslation::Original);
        let mut text = state.text.lock().unwrap();
        let mut buffer = vec![0u8; 96];
        let base = buffer.as_ptr() as usize;
        pointer(&mut buffer, 0, base + 16);
        pointer(&mut buffer, 16, base + 48);
        pointer(&mut buffer, 20, base + 64);
        pointer(&mut buffer, 24, base + 48);
        buffer[48..55].copy_from_slice(b"\x83\x65\x83\x58\x83\x67\0");
        buffer[64..71].copy_from_slice(b"~C00%d\0");
        let image = MemoryRange {
            start: base,
            end: base + buffer.len(),
        };
        let layout = RecordTableLayout {
            id: "table",
            translation_group: 1,
            root: &[0],
            first_record: 0,
            records: RecordCount::Fixed(2),
            text_offset: 0,
            parts: 2,
            stride: 8,
            directory: None,
        };

        unsafe { patch_record_table(&state, &mut text, "sample", image, layout, 932) };

        let first = read_pointer(&buffer, 16);
        assert!(!image.contains(first));
        assert_eq!(
            unsafe { CStr::from_ptr(first as *const _) }
                .to_str()
                .unwrap(),
            "テスト"
        );
        assert_eq!(read_pointer(&buffer, 20), base + 64);
        assert_eq!(read_pointer(&buffer, 24), first);
        assert_eq!(read_pointer(&buffer, 28), 0);
        unsafe { patch_record_table(&state, &mut text, "sample", image, layout, 932) };
        assert_eq!(read_pointer(&buffer, 16), first);
        assert_eq!(text.originals.get(&932).unwrap().len(), 1);
    }

    #[test]
    fn keyconfig_header_declares_utf8_after_its_resource_text_is_converted() {
        let state = test_state(None, MissingTranslation::Original);
        let mut text = state.text.lock().unwrap();
        let mut buffer = vec![0u8; 512];
        let base = buffer.as_ptr() as usize;
        pointer(&mut buffer, 0, base + 16);
        pointer(&mut buffer, 16, base + 64);
        pointer(&mut buffer, 20, base + 384);
        let header = b"<?xml version=\"1.0\" encoding=\"Shift_JIS\" ?>\r\n<!-- \x83\x65\x83\x58\x83\x67 -->\r\n<keyconfig>\r\n\0";
        buffer[64..64 + header.len()].copy_from_slice(header);
        let other = b"encoding=\"Shift_JIS\"\0";
        buffer[384..384 + other.len()].copy_from_slice(other);
        let image = MemoryRange {
            start: base,
            end: base + buffer.len(),
        };
        let layout = RecordTableLayout {
            id: "table_793",
            translation_group: 793,
            root: &[0],
            first_record: 0,
            records: RecordCount::Fixed(2),
            text_offset: 0,
            parts: 1,
            stride: 4,
            directory: None,
        };
        unsafe { patch_record_table(&state, &mut text, "mhfpac", image, layout, 932) };
        let replacement = read_pointer(&buffer, 16);
        assert_eq!(
            unsafe { CStr::from_ptr(replacement as *const _) }
                .to_str()
                .unwrap(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<!-- テスト -->\r\n<keyconfig>\r\n"
        );
        assert_eq!(read_pointer(&buffer, 20), base + 384);
        unsafe { patch_record_table(&state, &mut text, "mhfpac", image, layout, 932) };
        assert_eq!(read_pointer(&buffer, 16), replacement);
    }

    #[test]
    fn converts_quest_parts_from_the_runtime_category_counts() {
        let state = test_state(None, MissingTranslation::Original);
        let mut text = state.text.lock().unwrap();
        let mut buffer = vec![0u8; 224];
        let base = buffer.as_ptr() as usize;
        pointer(&mut buffer, 0, base + 16);
        pointer(&mut buffer, 4, base + 24);
        buffer[18..20].copy_from_slice(&1u16.to_le_bytes());
        pointer(&mut buffer, 20, base + 32);
        buffer[24..26].copy_from_slice(&1u16.to_le_bytes());
        pointer(&mut buffer, 32, base + 40);
        pointer(&mut buffer, 80, base + 128);
        buffer[86..88].copy_from_slice(&100u16.to_le_bytes());
        pointer(&mut buffer, 128, base + 192);
        pointer(&mut buffer, 156, base + 208);
        buffer[192..199].copy_from_slice(b"\x83\x65\x83\x58\x83\x67\0");
        buffer[208..214].copy_from_slice(b"quest\0");
        let image = MemoryRange {
            start: base,
            end: base + buffer.len(),
        };
        let layout = QuestTableLayout {
            id: "quest",
            translation_group: 1,
            root: 0,
            count_root: 4,
            category_stride: 8,
            category_count_field: 2,
            category_records_field: 4,
            record_text_field: 40,
            record_id_field: 46,
            parts: 8,
        };

        unsafe { patch_quest_table(&state, &mut text, "sample", image, layout, 932) };

        let first = read_pointer(&buffer, 128);
        assert_eq!(
            unsafe { CStr::from_ptr(first as *const _) }
                .to_str()
                .unwrap(),
            "テスト"
        );
        assert_eq!(read_pointer(&buffer, 132), 0);
        assert_eq!(read_pointer(&buffer, 156), base + 208);
    }

    #[test]
    fn source_strings_must_be_terminated_inside_their_image() {
        let buffer = b"text\0unterminated";
        let image = MemoryRange {
            start: buffer.as_ptr() as usize,
            end: buffer.as_ptr() as usize + buffer.len(),
        };
        assert_eq!(
            unsafe { read_image_string(image, image.start) }
                .unwrap()
                .to_bytes(),
            b"text"
        );
        assert!(unsafe { read_image_string(image, image.start - 1) }.is_none());
        assert!(unsafe { read_image_string(image, image.start + 5) }.is_none());
        assert!(unsafe { read_image_string(image, image.end) }.is_none());
    }

    #[test]
    fn main_resource_pointers_are_absolute_after_relocation() {
        let image = MemoryRange {
            start: 0x1000_0000,
            end: 0x1001_0000,
        };

        assert_eq!(resolve_image_pointer(image, 0x1000_1234), Some(0x1000_1234));
        assert_eq!(resolve_image_pointer(image, 0x0000_1234), None);
        assert_eq!(resolve_image_pointer(image, 0), None);
    }

    #[test]
    fn post_relocation_magic_has_its_loader_marker_cleared() {
        assert_eq!(relocated_resource_magic(0x1A66_686D), 0x0066_686D);
    }
}
