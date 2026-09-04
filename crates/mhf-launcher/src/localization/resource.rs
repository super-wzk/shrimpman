use super::{
    HookState, MemoryRange, MissingKeyArena, QuestTableLayout, RESOURCE_LAYOUTS, RecordTableLayout,
    ResourceBodyLayout, TranslationKey, replacement_for_key,
};
use std::{mem::size_of, ptr};

pub(super) unsafe fn patch_image(state: &HookState, resource_id: &'static str, image: MemoryRange) {
    let Some(layout) = RESOURCE_LAYOUTS
        .iter()
        .find(|layout| layout.id == resource_id)
    else {
        return;
    };
    if unsafe { read_image_u32(image, 0) } != Some(relocated_resource_magic(layout.magic))
        || unsafe { read_image_u32(image, 4) } != Some(layout.format_version)
    {
        return;
    }

    let mut missing_keys = if state.missing == crate::MissingTranslation::Key {
        state.missing_keys.lock().ok()
    } else {
        None
    };

    match layout.body {
        ResourceBodyLayout::Records(tables) => {
            for table in tables {
                unsafe {
                    patch_record_table(state, missing_keys.as_deref_mut(), layout.id, image, *table)
                };
            }
        }
        ResourceBodyLayout::Quest(quest) => unsafe {
            patch_quest_table(state, missing_keys.as_deref_mut(), layout.id, image, quest)
        },
    }
}

unsafe fn patch_record_table(
    state: &HookState,
    mut missing_keys: Option<&mut MissingKeyArena>,
    resource_id: &'static str,
    image: MemoryRange,
    layout: RecordTableLayout,
) {
    let Some(table_start) = (unsafe { read_pointer_target(image, layout.root) }) else {
        return;
    };
    let Some(text_cells_end) = usize::try_from(layout.records - 1)
        .ok()
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

    for record in 0..layout.records {
        let record_start = table_start + record as usize * usize::from(layout.stride);
        let key = TranslationKey {
            resource_id,
            group_id: layout.id,
            translation_group: layout.translation_group,
            record_id: record,
            part: 0,
        };
        for part in 0..layout.parts {
            let cell = record_start
                + usize::from(layout.text_offset)
                + usize::from(part) * size_of::<u32>();
            unsafe {
                patch_cell(
                    state,
                    missing_keys.as_deref_mut(),
                    image,
                    TranslationKey { part, ..key },
                    cell,
                )
            };
        }
    }
}

unsafe fn patch_quest_table(
    state: &HookState,
    mut missing_keys: Option<&mut MissingKeyArena>,
    resource_id: &'static str,
    image: MemoryRange,
    layout: QuestTableLayout,
) {
    let Some(category_table) = (unsafe { read_pointer_target(image, layout.root) }) else {
        return;
    };
    let Some(count_data) = (unsafe { read_pointer_target(image, layout.count_root) }) else {
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
                            missing_keys.as_deref_mut(),
                            image,
                            TranslationKey {
                                resource_id,
                                group_id: layout.id,
                                translation_group: layout.translation_group,
                                record_id: u32::from(quest_id),
                                part,
                            },
                            cell,
                        )
                    };
                }
            }
        }
    }
}

unsafe fn patch_cell(
    state: &HookState,
    missing_keys: Option<&mut MissingKeyArena>,
    image: MemoryRange,
    key: TranslationKey,
    cell: usize,
) {
    let Some(source_pointer) = (unsafe { read_u32_at(image, cell) }) else {
        return;
    };
    if source_pointer == 0 {
        return;
    }
    let Some(replacement) = replacement_for_key(state, missing_keys, key) else {
        return;
    };
    unsafe { ptr::write_unaligned(cell as *mut u32, replacement as usize as u32) };
}

unsafe fn read_pointer_target(image: MemoryRange, root: u32) -> Option<usize> {
    let pointer = unsafe { read_image_u32(image, root as usize) }?;
    resolve_image_pointer(image, pointer)
}

unsafe fn read_image_u32(image: MemoryRange, offset: usize) -> Option<u32> {
    let address = image.start.checked_add(offset)?;
    unsafe { read_u32_at(image, address) }
}

unsafe fn read_u16_at(image: MemoryRange, address: usize) -> Option<u16> {
    (address.checked_add(size_of::<u16>())? <= image.end)
        .then(|| unsafe { ptr::read_unaligned(address as *const u16) })
}

unsafe fn read_u32_at(image: MemoryRange, address: usize) -> Option<u32> {
    (address.checked_add(size_of::<u32>())? <= image.end)
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
    use super::{MemoryRange, relocated_resource_magic, resolve_image_pointer};

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
