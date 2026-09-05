use super::{CodeHook, HookState, MemoryRange, hook_state};
use std::{
    ffi::c_void,
    mem::size_of,
    ptr,
    sync::atomic::{AtomicU16, AtomicUsize, Ordering},
};

const TLK_LOADER_RVA: usize = 0x0071_6350;
const STAGE_FILE_LOADER_RVA: usize = 0x008E_27D0;
const STAGE_HD_FILE_LOADER_RVA: usize = 0x008E_2850;
const TLK_BUFFER_RVA: usize = 0x0ED5_2928;
const TLK_BUFFER_CAPACITY_RVA: usize = 0x0ED5_292C;
const MAX_TLK_BUFFER_CAPACITY: usize = 16 * 1024 * 1024;
const UNKNOWN_STAGE: u16 = u16::MAX;
const MAX_STAGE_PATH_LENGTH: usize = 32;

const TLK_LOADER_SIGNATURE: &[(usize, u8)] = &[
    (0, 0x55),
    (1, 0x8B),
    (2, 0xEC),
    (3, 0x83),
    (4, 0xE4),
    (5, 0xF8),
    (6, 0x83),
    (7, 0xEC),
    (8, 0x20),
    (9, 0x80),
    (10, 0x39),
    (11, 0x4A),
    (12, 0x75),
    (13, 0x43),
    (14, 0x80),
    (15, 0x79),
    (16, 0x01),
    (17, 0x4B),
    (18, 0x75),
    (19, 0x3D),
    (20, 0x80),
    (21, 0x79),
    (22, 0x02),
    (23, 0x52),
    (24, 0x75),
    (25, 0x37),
    (26, 0x80),
    (27, 0x79),
    (28, 0x03),
    (29, 0x1A),
    (30, 0x75),
    (31, 0x31),
];

const STAGE_FILE_LOADER_SIGNATURE: &[(usize, u8)] = &[
    (0, 0x55),
    (1, 0x8B),
    (2, 0xEC),
    (3, 0x53),
    (4, 0x56),
    (5, 0x57),
    (6, 0x8B),
    (7, 0xF8),
    (8, 0x8D),
    (9, 0x50),
    (10, 0x01),
    (11, 0xEB),
    (12, 0x03),
    (13, 0x8D),
    (14, 0x49),
    (15, 0x00),
    (16, 0x8A),
    (17, 0x08),
    (18, 0x40),
    (19, 0x84),
    (20, 0xC9),
    (21, 0x75),
    (22, 0xF9),
    (23, 0x2B),
    (24, 0xC2),
    (25, 0x8B),
    (26, 0xCF),
    (27, 0x8B),
    (28, 0xD8),
];

static TLK_LOADER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static STAGE_FILE_LOADER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static STAGE_HD_FILE_LOADER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static CURRENT_STAGE: AtomicU16 = AtomicU16::new(UNKNOWN_STAGE);

macro_rules! define_stage_file_loader_detour {
    ($name:ident, $original:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push ebp",
                "mov ebp, esp",
                "push eax",
                "push dword ptr [ebp + 8]",
                "mov eax, dword ptr [ebp - 4]",
                "call dword ptr [{original}]",
                "add esp, 4",
                "push eax",
                "push eax",
                "push dword ptr [ebp - 4]",
                "call {dispatch}",
                "add esp, 8",
                "pop eax",
                "mov esp, ebp",
                "pop ebp",
                "ret",
                original = sym $original,
                dispatch = sym capture_stage_dispatch,
            );
        }
    };
}

define_stage_file_loader_detour!(stage_file_loader_detour, STAGE_FILE_LOADER_ORIGINAL);
define_stage_file_loader_detour!(stage_hd_file_loader_detour, STAGE_HD_FILE_LOADER_ORIGINAL);

pub(super) fn code_hooks() -> [CodeHook; 3] {
    [
        CodeHook {
            name: "mhfo TLK loader",
            rva: TLK_LOADER_RVA,
            signature: TLK_LOADER_SIGNATURE,
            detour: tlk_loader_detour as *const () as *mut c_void,
            original: &TLK_LOADER_ORIGINAL,
        },
        CodeHook {
            name: "mhfo stage file loader",
            rva: STAGE_FILE_LOADER_RVA,
            signature: STAGE_FILE_LOADER_SIGNATURE,
            detour: stage_file_loader_detour as *const () as *mut c_void,
            original: &STAGE_FILE_LOADER_ORIGINAL,
        },
        CodeHook {
            name: "mhfo HD stage file loader",
            rva: STAGE_HD_FILE_LOADER_RVA,
            signature: STAGE_FILE_LOADER_SIGNATURE,
            detour: stage_hd_file_loader_detour as *const () as *mut c_void,
            original: &STAGE_HD_FILE_LOADER_ORIGINAL,
        },
    ]
}

pub(super) const fn required_image_end() -> usize {
    TLK_BUFFER_CAPACITY_RVA + size_of::<u32>()
}

unsafe extern "C" fn patch_tlk_dispatch(source: *const u8, source_size: usize) {
    unsafe { patch_tlk_records(hook_state(), source, source_size) };
}

unsafe extern "C" fn capture_stage_dispatch(path: *const u8, loaded_size: usize) {
    if loaded_size == 0 {
        return;
    }
    if let Some(stage) = unsafe { parse_stage_path(path) } {
        CURRENT_STAGE.store(stage, Ordering::Release);
    }
}

unsafe fn parse_stage_path(path: *const u8) -> Option<u16> {
    if path.is_null() {
        return None;
    }
    let length = (0..=MAX_STAGE_PATH_LENGTH).find(|offset| unsafe { *path.add(*offset) == 0 })?;
    let path = unsafe { std::slice::from_raw_parts(path, length) };
    let suffix = path
        .strip_prefix(b"stage\\st")
        .or_else(|| path.strip_prefix(b"stage/st"))
        .or_else(|| path.strip_prefix(b"stage-hd\\st"))
        .or_else(|| path.strip_prefix(b"stage-hd/st"))?
        .strip_suffix(b".pac")?;
    if suffix.len() != 3 || !suffix.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(
        u16::from(suffix[0] - b'0') * 100
            + u16::from(suffix[1] - b'0') * 10
            + u16::from(suffix[2] - b'0'),
    )
}

unsafe fn patch_tlk_records(state: &HookState, source: *const u8, source_size: usize) {
    let stage = CURRENT_STAGE.load(Ordering::Acquire);
    if stage == UNKNOWN_STAGE {
        return;
    }
    let buffer_start =
        unsafe { ptr::read_volatile((state.module_base + TLK_BUFFER_RVA) as *const u32) as usize };
    let capacity = unsafe {
        ptr::read_volatile((state.module_base + TLK_BUFFER_CAPACITY_RVA) as *const u32) as usize
    };
    let Some(image_size) = (unsafe { tlk_image_size(source, source_size, capacity) }) else {
        return;
    };
    if buffer_start == 0 {
        return;
    }
    let Some(buffer_end) = buffer_start.checked_add(image_size) else {
        return;
    };

    for entry in state.locale.stage_translations() {
        if entry.stage != stage {
            continue;
        }
        let replacement = entry.replacement.as_ptr();
        let Some(section_range) =
            (unsafe { find_tlk_section(buffer_start, buffer_end, entry.section) })
        else {
            continue;
        };
        let record_index_end = usize::from(entry.record) + 1;
        let Some(cell) = section_range
            .start
            .checked_add(usize::from(entry.record) * size_of::<u32>())
        else {
            continue;
        };
        let Some(cell_end) = cell.checked_add(size_of::<u32>()) else {
            continue;
        };
        if cell_end > section_range.end {
            continue;
        }
        let relative = unsafe { ptr::read_unaligned(cell as *const u32) };
        let current = resolve_relative_pointer(section_range.start, relative);
        if current == replacement as usize {
            continue;
        }
        let minimum_record_offset = record_index_end * size_of::<u32>();
        if (relative as usize) < minimum_record_offset || !section_range.contains(current) {
            continue;
        }
        unsafe {
            ptr::write_unaligned(
                cell as *mut u32,
                relative_pointer(section_range.start, replacement as usize),
            )
        };
    }
}

unsafe fn tlk_image_size(source: *const u8, source_size: usize, capacity: usize) -> Option<usize> {
    const JKR_HEADER_SIZE: usize = 16;
    const JKR_MAGIC: u32 = 0x1A52_4B4A;

    if source.is_null() || source_size == 0 || capacity == 0 || capacity > MAX_TLK_BUFFER_CAPACITY {
        return None;
    }
    let image_size = if source_size >= JKR_HEADER_SIZE
        && unsafe { ptr::read_unaligned(source.cast::<u32>()) } == JKR_MAGIC
    {
        unsafe { ptr::read_unaligned(source.add(12).cast::<u32>()) as usize }
    } else {
        source_size
    };
    (image_size != 0 && image_size <= capacity && image_size <= MAX_TLK_BUFFER_CAPACITY)
        .then_some(image_size)
}

unsafe fn find_tlk_section(
    buffer_start: usize,
    buffer_end: usize,
    wanted_section: u16,
) -> Option<MemoryRange> {
    let mut cursor = buffer_start;
    let mut selected_offset = None;
    let mut directory_end = None;
    while cursor.checked_add(8)? <= buffer_end {
        let section = unsafe { ptr::read_unaligned(cursor as *const i32) };
        let offset = unsafe { ptr::read_unaligned((cursor + 4) as *const u32) } as usize;
        cursor += 8;
        if section == -1 {
            directory_end = Some(cursor);
            break;
        }
        if section < 0 || offset >= buffer_end - buffer_start {
            return None;
        }
        if section as u32 == u32::from(wanted_section) {
            selected_offset = Some(offset);
        }
    }
    let directory_end = directory_end?;
    let selected_offset = selected_offset?;
    let section_start = buffer_start.checked_add(selected_offset)?;
    if section_start < directory_end {
        return None;
    }

    let mut section_end = buffer_end;
    cursor = buffer_start;
    while cursor.checked_add(8)? <= directory_end {
        let section = unsafe { ptr::read_unaligned(cursor as *const i32) };
        if section == -1 {
            break;
        }
        let offset = unsafe { ptr::read_unaligned((cursor + 4) as *const u32) } as usize;
        if offset > selected_offset {
            section_end = section_end.min(buffer_start.checked_add(offset)?);
        }
        cursor += 8;
    }
    (section_start < section_end).then_some(MemoryRange {
        start: section_start,
        end: section_end,
    })
}

fn relative_pointer(base: usize, target: usize) -> u32 {
    (target as u32).wrapping_sub(base as u32)
}

fn resolve_relative_pointer(base: usize, offset: u32) -> usize {
    (base as u32).wrapping_add(offset) as usize
}

#[unsafe(naked)]
unsafe extern "C" fn tlk_loader_detour() {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push ecx",
        "push dword ptr [ebp + 8]",
        "call dword ptr [{original}]",
        "add esp, 4",
        "push eax",
        "push dword ptr [ebp + 8]",
        "push dword ptr [ebp - 4]",
        "call {dispatch}",
        "add esp, 8",
        "pop eax",
        "mov esp, ebp",
        "pop ebp",
        "ret",
        original = sym TLK_LOADER_ORIGINAL,
        dispatch = sym patch_tlk_dispatch,
    );
}

#[cfg(test)]
mod tests {
    use super::{parse_stage_path, relative_pointer, resolve_relative_pointer, tlk_image_size};

    #[test]
    fn relative_tlk_pointer_can_target_the_static_translation_image() {
        let base = 0xF000_0000usize;
        let target = 0x1000_1234usize;
        let offset = relative_pointer(base, target);

        assert_eq!(resolve_relative_pointer(base, offset), target);
    }

    #[test]
    fn tlk_image_size_uses_the_decompressed_size() {
        let mut jkr = [0u8; 16];
        jkr[..4].copy_from_slice(&0x1A52_4B4Au32.to_le_bytes());
        jkr[12..].copy_from_slice(&0x1234u32.to_le_bytes());

        assert_eq!(
            unsafe { tlk_image_size(jkr.as_ptr(), jkr.len(), 0x2000) },
            Some(0x1234)
        );
        assert_eq!(unsafe { tlk_image_size(b"raw".as_ptr(), 3, 16) }, Some(3));
    }

    #[test]
    fn reads_the_actual_stage_number_from_loaded_resource_paths() {
        assert_eq!(
            unsafe { parse_stage_path(c"stage\\st125.pac".as_ptr().cast()) },
            Some(125)
        );
        assert_eq!(
            unsafe { parse_stage_path(c"stage-hd\\st340.pac".as_ptr().cast()) },
            Some(340)
        );
        assert_eq!(
            unsafe { parse_stage_path(c"stage/st001.pac".as_ptr().cast()) },
            Some(1)
        );
        assert_eq!(
            unsafe { parse_stage_path(c"campaign\\x01.txb".as_ptr().cast()) },
            None
        );
    }
}
