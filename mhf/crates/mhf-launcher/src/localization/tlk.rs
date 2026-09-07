use super::{
    CodeHook, HOOK_STATE, HookState, MemoryRange, TranslationKey, replacement_for_key,
    resource::read_image_string,
};
use std::{
    collections::HashSet,
    ffi::c_void,
    mem::size_of,
    ptr,
    sync::{
        PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

const TLK_LOADER_RVA: usize = 0x0071_6350;
const STAGE_FILE_LOADER_RVA: usize = 0x008E_27D0;
const STAGE_HD_FILE_LOADER_RVA: usize = 0x008E_2850;
const TLK_BUFFER_RVA: usize = 0x0ED5_2928;
const TLK_BUFFER_CAPACITY_RVA: usize = 0x0ED5_292C;
const MAX_TLK_BUFFER_CAPACITY: usize = 16 * 1024 * 1024;
pub(super) const UNKNOWN_STAGE: u16 = u16::MAX;
const MAX_STAGE_PATH_LENGTH: usize = 32;
const STAGE_TLK_RETURN_RVA: usize = 0x0089_F4CD;
const LOCALIZED_TLK_RETURN_RVA: usize = 0x0089_F5C1;

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

pub(super) unsafe fn validate_sources(base: usize) -> Result<(), String> {
    for (rva, expected) in [
        (
            0x0089_F4BF,
            &[
                0x8B, 0x8E, 0xF4, 0, 0, 0, 0x50, 0x03, 0xCE, 0xE8, 0x83, 0x6E, 0xE7, 0xFF, 0x83,
                0xC4, 0x04,
            ][..],
        ),
        (
            0x0089_F5AD,
            &[
                0x8B, 0x54, 0xC8, 0x08, 0x85, 0xD2, 0x74, 0x0F, 0x8B, 0x4C, 0xC8, 0x04, 0x52, 0x03,
                0xC8, 0xE8, 0x8F, 0x6D, 0xE7, 0xFF, 0x83, 0xC4, 0x04,
            ][..],
        ),
    ] {
        if unsafe { std::slice::from_raw_parts((base + rva) as *const u8, expected.len()) }
            != expected
        {
            return Err(format!("unsupported TLK source boundary at RVA {rva:#x}"));
        }
    }
    Ok(())
}

unsafe extern "C" fn patch_tlk_dispatch(
    source: *const u8,
    source_size: usize,
    caller: usize,
    saved_esi: u32,
) {
    let invocation = HOOK_STATE.enter();
    if let Some(state) = invocation.state() {
        let (stage, code_page) = match caller.checked_sub(state.module_base) {
            Some(STAGE_TLK_RETURN_RVA) => (state.current_stage.load(Ordering::Acquire), 932),
            Some(LOCALIZED_TLK_RETURN_RVA) => match state.language_code_page() {
                Ok(code_page) => (saved_esi as u16, code_page),
                Err(error) => {
                    eprintln!("failed to load localized TLK text: {error}");
                    return;
                }
            },
            _ => {
                eprintln!("unsupported TLK source caller at {caller:#x}");
                return;
            }
        };
        unsafe { patch_tlk_records(state, source, source_size, stage, code_page) };
    }
}

unsafe extern "C" fn capture_stage_dispatch(path: *const u8, loaded_size: usize) {
    if loaded_size == 0 {
        return;
    }
    let invocation = HOOK_STATE.enter();
    if let Some(state) = invocation.state()
        && let Some(stage) = unsafe { parse_stage_path(path) }
    {
        state.current_stage.store(stage, Ordering::Release);
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

unsafe fn patch_tlk_records(
    state: &HookState,
    source: *const u8,
    source_size: usize,
    stage: u16,
    code_page: u32,
) {
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

    unsafe {
        patch_tlk_image(
            state,
            MemoryRange {
                start: buffer_start,
                end: buffer_end,
            },
            stage,
            code_page,
        )
    };
}

unsafe fn patch_tlk_image(state: &HookState, image: MemoryRange, stage: u16, code_page: u32) {
    let Some(sections) = (unsafe { tlk_sections(image) }) else {
        return;
    };
    let mut text = state.text.lock().unwrap_or_else(PoisonError::into_inner);
    for section in sections {
        let range = section.image;
        if range.end - range.start < size_of::<u32>() {
            continue;
        }
        // A TLK section starts with its relative string-pointer table. The
        // first string begins immediately after that table, so its offset is
        // the table's byte length. Read it before replacing any pointers.
        let table_size = unsafe { ptr::read_unaligned(range.start as *const u32) } as usize;
        let record_count = table_size / size_of::<u32>();
        if table_size == 0
            || !table_size.is_multiple_of(size_of::<u32>())
            || table_size >= range.end - range.start
            || record_count > usize::from(u16::MAX) + 1
        {
            continue;
        }
        let strings = MemoryRange {
            start: range.start + table_size,
            end: range.end,
        };
        for record in 0..record_count {
            let cell = range.start + record * size_of::<u32>();
            let relative = unsafe { ptr::read_unaligned(cell as *const u32) };
            let current = resolve_relative_pointer(range.start, relative);
            // Existing UTF-8 replacements live outside the section. This also
            // prevents a repeated callback from decoding them as legacy text.
            if !strings.contains(current) {
                continue;
            }
            let Some(source) = (unsafe { read_image_string(strings, current) }) else {
                eprintln!(
                    "TLK section {:04X} record {record:04X} has no NUL terminator inside its source section",
                    section.id
                );
                continue;
            };
            let replacement = if stage != UNKNOWN_STAGE && section.first {
                replacement_for_key(
                    state,
                    &mut text,
                    TranslationKey::Stage {
                        stage,
                        section: section.id,
                        record: record as u16,
                    },
                    source,
                    code_page,
                )
            } else {
                text.original(source, code_page)
            };
            match replacement {
                Ok(replacement) => unsafe {
                    ptr::write_unaligned(
                        cell as *mut u32,
                        relative_pointer(range.start, replacement as usize),
                    )
                },
                Err(error) => eprintln!(
                    "failed to convert TLK section {:04X} record {record:04X}: {error}",
                    section.id
                ),
            }
        }
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

struct TlkSection {
    id: u16,
    first: bool,
    image: MemoryRange,
}

unsafe fn tlk_sections(image: MemoryRange) -> Option<Vec<TlkSection>> {
    let mut cursor = image.start;
    let mut sections = Vec::new();
    let mut ids = HashSet::new();
    loop {
        let next = cursor.checked_add(8)?;
        if next > image.end {
            return None;
        }
        let section = unsafe { ptr::read_unaligned(cursor as *const i32) };
        let offset = unsafe { ptr::read_unaligned((cursor + 4) as *const u32) } as usize;
        cursor = next;
        if section == -1 {
            break;
        }
        let id = u16::try_from(section).ok()?;
        let start = image.start.checked_add(offset)?;
        if !image.contains(start) {
            return None;
        }
        sections.push(TlkSection {
            id,
            // The game's directory lookup returns its first matching ID.
            first: ids.insert(id),
            image: MemoryRange {
                start,
                end: image.end,
            },
        });
    }
    sections.sort_unstable_by_key(|section| section.image.start);
    for index in 0..sections.len() {
        let end = sections
            .get(index + 1)
            .map_or(image.end, |next| next.image.start);
        let section = &mut sections[index];
        if section.image.start < cursor || section.image.start >= end {
            return None;
        }
        section.image.end = end;
    }
    Some(sections)
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
        "push esi",
        "push dword ptr [ebp + 8]",
        "call dword ptr [{original}]",
        "add esp, 4",
        "push eax",
        "push dword ptr [ebp - 8]",
        "push dword ptr [ebp + 4]",
        "push dword ptr [ebp + 8]",
        "push dword ptr [ebp - 4]",
        "call {dispatch}",
        "add esp, 16",
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
    use super::{
        MemoryRange, UNKNOWN_STAGE, parse_stage_path, patch_tlk_image, relative_pointer,
        resolve_relative_pointer, tlk_image_size, tlk_sections,
    };
    use crate::{
        MissingTranslation,
        localization::{CompiledDictionary, CompiledLocale, test_state},
    };
    use std::ffi::CStr;

    static TRANSLATIONS: &[u8] = &[
        0, 0, 0, 0, 125, 0, 0, 0, 1, 0, 23, 0, 20, 0, 0, 0, 7, 0, 0, 0, 0xE4, 0xB8, 0xAD, 0xE6,
        0x96, 0x87, 0,
    ];
    static LOCALES: [CompiledLocale; 1] = [CompiledLocale::new("test", 0, 1)];
    static DICTIONARY: CompiledDictionary = CompiledDictionary::new(TRANSLATIONS, &LOCALES);

    struct TlkImage {
        bytes: Vec<u8>,
        sections: Vec<usize>,
    }

    impl TlkImage {
        fn new(sections: &[(u16, &[&[u8]])]) -> Self {
            let mut image = Self {
                bytes: vec![0; (sections.len() + 1) * 8],
                sections: Vec::new(),
            };
            image.write_u32(sections.len() * 8, u32::MAX);
            image.write_u32(sections.len() * 8 + 4, u32::MAX);
            for (index, (id, records)) in sections.iter().enumerate() {
                let start = image.bytes.len();
                image.sections.push(start);
                image.write_u32(index * 8, u32::from(*id));
                image.write_u32(index * 8 + 4, start as u32);
                image.bytes.resize(start + records.len() * 4, 0);
                for (record, source) in records.iter().enumerate() {
                    image.write_u32(start + record * 4, (image.bytes.len() - start) as u32);
                    image.bytes.extend_from_slice(source);
                    image.bytes.push(0);
                }
            }
            image
        }

        fn range(&self) -> MemoryRange {
            let start = self.bytes.as_ptr() as usize;
            MemoryRange {
                start,
                end: start + self.bytes.len(),
            }
        }

        fn write_u32(&mut self, offset: usize, value: u32) {
            self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn pointer(&self, section: usize, record: usize) -> usize {
            let offset = self.sections[section];
            let cell = offset + record * 4;
            let relative = u32::from_le_bytes(self.bytes[cell..cell + 4].try_into().unwrap());
            resolve_relative_pointer(self.range().start + offset, relative)
        }

        fn text(&self, section: usize, record: usize) -> &CStr {
            unsafe { CStr::from_ptr(self.pointer(section, record) as *const _) }
        }
    }

    #[test]
    fn converts_every_record_with_sparse_overrides_and_preserves_utf8_on_repeat() {
        let state = test_state(DICTIONARY.locale("test"), MissingTranslation::Original);
        let image = TlkImage::new(&[(23, &[b"ASCII", b"\x93\xFA\x96\x7B", b"\x93\xFA"])]);

        unsafe { patch_tlk_image(&state, image.range(), 125, 932) };

        assert_eq!(image.text(0, 0), c"ASCII");
        assert_eq!(image.text(0, 1).to_str().unwrap(), "中文");
        assert_eq!(image.text(0, 2).to_str().unwrap(), "日");
        let pointers = [image.pointer(0, 1), image.pointer(0, 2)];
        unsafe { patch_tlk_image(&state, image.range(), 125, 932) };
        assert_eq!([image.pointer(0, 1), image.pointer(0, 2)], pointers);
        assert_eq!(image.text(0, 1).to_str().unwrap(), "中文");
        assert_eq!(image.text(0, 2).to_str().unwrap(), "日");
    }

    #[test]
    fn converts_originals_when_stage_is_unknown_without_applying_missing_policy() {
        let state = test_state(DICTIONARY.locale("test"), MissingTranslation::Key);
        let image = TlkImage::new(&[(23, &[b"\x93\xFA\x96\x7B", b"\x93\xFA"])]);

        unsafe { patch_tlk_image(&state, image.range(), UNKNOWN_STAGE, 932) };

        assert_eq!(image.text(0, 0).to_str().unwrap(), "日本");
        assert_eq!(image.text(0, 1).to_str().unwrap(), "日");
    }

    #[test]
    fn language_overlays_convert_all_records_using_their_source_code_page() {
        let state = test_state(None, MissingTranslation::Original);
        for (code_page, source, expected) in [
            (949, b"\xC7\xD1\xB1\xDB".as_slice(), "한글"),
            (950, b"\xC1\x63\xC5\xE9".as_slice(), "繁體"),
        ] {
            let image = TlkImage::new(&[(23, &[b"ASCII", source])]);
            unsafe { patch_tlk_image(&state, image.range(), 125, code_page) };
            assert_eq!(image.text(0, 0), c"ASCII");
            assert_eq!(image.text(0, 1).to_str().unwrap(), expected);
        }
    }

    #[test]
    fn duplicate_ids_translate_only_the_first_directory_entry() {
        let state = test_state(DICTIONARY.locale("test"), MissingTranslation::Original);
        let records: &[&[u8]] = &[b"ASCII", b"\x93\xFA\x96\x7B"];
        let mut image = TlkImage::new(&[(23, records), (23, records)]);
        // Directory order, rather than the sections' physical byte order,
        // determines which occurrence the game selects.
        image.write_u32(4, image.sections[1] as u32);
        image.write_u32(12, image.sections[0] as u32);

        unsafe { patch_tlk_image(&state, image.range(), 125, 932) };

        assert_eq!(image.text(0, 1).to_str().unwrap(), "日本");
        assert_eq!(image.text(1, 1).to_str().unwrap(), "中文");
    }

    #[test]
    fn a_string_cannot_use_the_next_sections_pointer_table_as_its_terminator() {
        let state = test_state(None, MissingTranslation::Original);
        let mut image = TlkImage::new(&[(23, &[b"\x93\xFA"]), (24, &[b"\x93\xFA"])]);
        image.bytes[image.sections[1] - 1] = b'!';
        let source = image.pointer(0, 0);

        unsafe { patch_tlk_image(&state, image.range(), UNKNOWN_STAGE, 932) };

        assert_eq!(image.pointer(0, 0), source);
        assert_eq!(image.text(1, 0).to_str().unwrap(), "日");
    }

    #[test]
    fn malformed_directories_and_pointer_tables_are_not_patched() {
        let state = test_state(None, MissingTranslation::Original);
        for invalid_offset in [0, u32::MAX] {
            let mut image = TlkImage::new(&[(23, &[b"\x93\xFA"])]);
            image.write_u32(4, invalid_offset);
            assert!(unsafe { tlk_sections(image.range()) }.is_none());
        }
        let mut image = TlkImage::new(&[(23, &[b"\x93\xFA"])]);
        image.write_u32(8, 24);
        assert!(unsafe { tlk_sections(image.range()) }.is_none());

        for invalid_table_size in [0, 3, 8, u32::MAX] {
            let mut image = TlkImage::new(&[(23, &[b"\x93\xFA"])]);
            image.write_u32(image.sections[0], invalid_table_size);
            let original = image.bytes.clone();
            unsafe { patch_tlk_image(&state, image.range(), UNKNOWN_STAGE, 932) };
            assert_eq!(image.bytes, original);
        }
        let mut image = TlkImage::new(&[(23, &[b"ASCII", b"\x93\xFA"])]);
        image.write_u32(image.sections[0] + 4, 4);
        let original = image.bytes.clone();
        unsafe { patch_tlk_image(&state, image.range(), UNKNOWN_STAGE, 932) };
        assert_eq!(image.bytes, original);
    }

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
