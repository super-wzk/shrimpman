use crate::{MissingTranslation, TranslationConfig};
use bumpalo::Bump;
use mhf_hooks::{HookGuard, HookSlot};
use std::{
    collections::HashMap,
    ffi::{CStr, c_void},
    fmt::{self, Write as _},
    mem::size_of,
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar},
        System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExA},
    },
    core::PCSTR,
};

use dictionary::{CompiledDictionary, CompiledLocale, RuntimeLocale};

mod dictionary;
#[cfg(test)]
mod extra_resource_tests;
mod native;
mod resource;
mod tlk;

static EMPTY_RECORD: [u8; 1] = [0];

const MAX_RESOURCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TranslationKey {
    Resource {
        resource_id: &'static str,
        group_id: &'static str,
        translation_group: u32,
        record_id: u32,
        part: u16,
    },
    Stage {
        stage: u16,
        section: u16,
        record: u16,
    },
}

impl fmt::Display for TranslationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource {
                resource_id,
                group_id,
                record_id,
                part,
                ..
            } => {
                write!(formatter, "{resource_id}:{group_id}:{record_id}")?;
                if *part != 0 {
                    write!(formatter, ":{part:02}")?;
                }
                Ok(())
            }
            Self::Stage {
                stage,
                section,
                record,
            } => {
                write!(formatter, "stage:{stage:03}:{section:04X}:{record:04X}")
            }
        }
    }
}

#[derive(Default)]
struct TextArena {
    storage: Bump,
    scratch: String,
    originals: HashMap<u32, HashMap<Vec<u8>, usize>>,
}

impl TextArena {
    fn store_key(&mut self, key: TranslationKey) -> *const u8 {
        self.scratch.clear();
        self.scratch.push('[');
        write!(&mut self.scratch, "{key}").expect("writing to a String cannot fail");
        self.scratch.push(']');
        self.scratch.push('\0');
        self.storage
            .alloc_slice_copy(self.scratch.as_bytes())
            .as_ptr()
    }

    fn original(&mut self, source: &CStr, code_page: u32) -> Result<*const u8, String> {
        let bytes = source.to_bytes();
        if bytes.is_ascii() {
            return Ok(source.as_ptr().cast());
        }
        let originals = self.originals.entry(code_page).or_default();
        if let Some(pointer) = originals.get(bytes) {
            return Ok(*pointer as *const u8);
        }
        // Only known source-resource encodings enter this boundary. Runtime
        // UTF-8 strings and compiled translations are never decoded here.
        let mut text = decode_source(bytes, code_page)?;
        text.push('\0');
        let pointer = self.storage.alloc_slice_copy(text.as_bytes()).as_ptr();
        originals.insert(bytes.to_vec(), pointer as usize);
        Ok(pointer)
    }
}

fn decode_source(source: &[u8], code_page: u32) -> Result<String, String> {
    let length = unsafe { MultiByteToWideChar(code_page, MB_ERR_INVALID_CHARS, source, None) };
    if length <= 0 {
        return Err(format!(
            "invalid resource text for Windows code page {code_page}: {}",
            windows::core::Error::from_thread()
        ));
    }
    let mut utf16 = vec![0; length as usize];
    let written =
        unsafe { MultiByteToWideChar(code_page, MB_ERR_INVALID_CHARS, source, Some(&mut utf16)) };
    if written != length {
        return Err(format!(
            "failed to decode resource text for Windows code page {code_page}: {}",
            windows::core::Error::from_thread()
        ));
    }
    String::from_utf16(&utf16).map_err(|error| format!("invalid decoded resource text: {error}"))
}

#[derive(Clone, Copy)]
struct ResourceLayout {
    id: &'static str,
    identity: Option<(u32, u32)>,
    code_page: Option<u32>,
    body: ResourceBodyLayout,
}

#[derive(Clone, Copy)]
enum ResourceBodyLayout {
    Records(&'static [RecordTableLayout]),
    Quest(QuestTableLayout),
}

#[derive(Clone, Copy)]
struct RecordTableLayout {
    id: &'static str,
    translation_group: u32,
    root: &'static [u32],
    first_record: u32,
    records: RecordCount,
    text_offset: u16,
    parts: u16,
    stride: u16,
    directory: Option<(u32, RecordCount)>,
}

#[derive(Clone, Copy)]
enum RecordCount {
    Fixed(u32),
    U16(&'static [u32]),
    U32(&'static [u32]),
    Sentinel {
        root: &'static [u32],
        stride: u16,
        offset: u16,
        width: u8,
        value: u32,
    },
}

#[derive(Clone, Copy)]
struct QuestTableLayout {
    id: &'static str,
    translation_group: u32,
    root: u32,
    count_root: u32,
    category_stride: u16,
    category_count_field: u16,
    category_records_field: u16,
    record_text_field: u16,
    record_id_field: u16,
    parts: u16,
}

struct CodeHook {
    name: &'static str,
    rva: usize,
    signature: &'static [(usize, u8)],
    detour: *mut c_void,
    original: &'static AtomicUsize,
}

struct MainResourceBinding {
    id: &'static str,
    buffer_rva: usize,
    size_rva: usize,
}

macro_rules! define_main_resource_detour {
    ($name:ident, $resource_index:literal, $original:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "pushfd",
                "pushad",
                "mov eax, esp",
                "sub esp, 528",
                "and esp, -16",
                "fxsave [esp]",
                "mov [esp + 512], eax",
                "push {resource_index}",
                "call {dispatch}",
                "add esp, 4",
                "fxrstor [esp]",
                "mov esp, [esp + 512]",
                "popad",
                "popfd",
                "jmp dword ptr [{original}]",
                resource_index = const $resource_index,
                dispatch = sym patch_resource_dispatch,
                original = sym $original,
            );
        }
    };
}

include!(concat!(env!("OUT_DIR"), "/translations.rs"));

#[derive(Clone, Copy)]
struct MemoryRange {
    start: usize,
    end: usize,
}

impl MemoryRange {
    fn contains(self, address: usize) -> bool {
        (self.start..self.end).contains(&address)
    }
}

pub(crate) struct HookState {
    // Restore image pointers before releasing the image or text allocations.
    _native: Option<native::NativeTextGuard>,
    // Release the DLL before the buffers it may still reference in DllMain.
    _module: ModuleReference,
    module_base: usize,
    locale: Option<RuntimeLocale>,
    missing: MissingTranslation,
    text: Mutex<TextArena>,
    current_stage: AtomicU16,
}

impl HookState {
    fn language_code_page(&self) -> Result<u32, String> {
        let parameters =
            unsafe { ptr::read_volatile((self.module_base + 0x0E86_6C5C) as *const u32) } as usize;
        if parameters == 0 {
            return Err("game language parameters are unavailable while loading text".to_owned());
        }
        let language = unsafe { ptr::read_volatile((parameters + 0x1DB0) as *const u32) };
        source_code_page(language)
    }
}

fn source_code_page(language: u32) -> Result<u32, String> {
    // The original CreateFontA branch uses SHIFTJIS for Japanese and English,
    // HANGUL for Korean, and BIG5 for Traditional Chinese.
    match language {
        0 | 1 => Ok(932),
        6 => Ok(949),
        7 => Ok(950),
        _ => Err(format!("unsupported source-resource language {language}")),
    }
}

static HOOK_STATE: HookSlot<HookState> = HookSlot::new();

// A DLL reference is process-wide and may be released from the cleanup thread.
struct ModuleReference(usize);

impl ModuleReference {
    unsafe fn acquire(module: HMODULE) -> Result<Self, String> {
        let mut retained = HMODULE::default();
        unsafe {
            GetModuleHandleExA(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                PCSTR(module.0.cast()),
                &mut retained,
            )
        }
        .map_err(|error| format!("failed to retain the localization module: {error}"))?;
        Ok(Self(retained.0 as usize))
    }
}

impl Drop for ModuleReference {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(HMODULE(self.0 as *mut c_void)) };
    }
}

/// The module must be valid, and its native callers must stop before cleanup:
/// the naked shims use their trampolines outside the Rust callback invocation.
pub(crate) unsafe fn install(
    module: HMODULE,
    translation: Option<&TranslationConfig>,
) -> Result<HookGuard<HookState>, String> {
    let mut hooks = HOOK_STATE.prepare()?;
    let dictionary = &TRANSLATION_DICTIONARY;
    let locale = translation
        .map(|translation| {
            dictionary.locale(&translation.locale).ok_or_else(|| {
                let available = dictionary.locale_ids().collect::<Vec<_>>().join(", ");
                format!(
                    "translation locale {:?} is not embedded; available locales: {available}",
                    translation.locale
                )
            })
        })
        .transpose()?;

    let retained_module = unsafe { ModuleReference::acquire(module) }?;
    let module_base = module.0 as usize;
    let image_size = unsafe { module_image_size(module_base) }
        .ok_or_else(|| "mhfo module has an invalid PE image layout".to_owned())?;
    let main_resource_hooks = main_resource_code_hooks();
    let dynamic_resource_hooks = tlk::code_hooks();
    let required_end = main_resource_hooks
        .iter()
        .chain(&dynamic_resource_hooks)
        .map(|hook| hook.rva + signature_size(hook.signature))
        .chain(MAIN_RESOURCE_BINDINGS.iter().flat_map(|resource| {
            [
                resource.buffer_rva + size_of::<u32>(),
                resource.size_rva + size_of::<u32>(),
            ]
        }))
        .chain([tlk::required_image_end()])
        .max()
        .expect("localization hooks use at least one RVA");
    if image_size < required_end {
        return Err(format!(
            "mhfo module image is too small for localization hooks: 0x{:X}",
            image_size
        ));
    }

    for hook in main_resource_hooks.iter().chain(&dynamic_resource_hooks) {
        let target = module_base + hook.rva;
        if !unsafe { matches_signature(target, hook.signature) } {
            return Err(format!(
                "unsupported {} at RVA 0x{:08X}",
                hook.name, hook.rva
            ));
        }
    }
    unsafe { tlk::validate_sources(module_base) }?;
    unsafe { resource::validate_sources(module_base) }?;

    for hook in main_resource_hooks
        .into_iter()
        .chain(dynamic_resource_hooks)
    {
        let target = module_base + hook.rva;
        let trampoline = unsafe { hooks.create(hook.name, target as *mut c_void, hook.detour) }?;
        hook.original.store(trampoline as usize, Ordering::Release);
    }

    let missing = translation.map_or(MissingTranslation::Original, |config| config.missing);
    let native = unsafe { native::install(module_base, image_size, locale.as_ref(), missing) }?;
    let state = HookState {
        _native: Some(native),
        _module: retained_module,
        module_base,
        locale,
        missing,
        text: Mutex::new(TextArena::default()),
        current_stage: AtomicU16::new(tlk::UNKNOWN_STAGE),
    };
    unsafe { hooks.install(state) }
}

fn replacement_for_key(
    state: &HookState,
    text: &mut TextArena,
    key: TranslationKey,
    source: &CStr,
    code_page: u32,
) -> Result<*const u8, String> {
    replacement_for_locale(
        state.locale.as_ref(),
        state.missing,
        text,
        key,
        source,
        code_page,
    )
}

fn replacement_for_locale(
    locale: Option<&RuntimeLocale>,
    missing: MissingTranslation,
    text: &mut TextArena,
    key: TranslationKey,
    source: &CStr,
    code_page: u32,
) -> Result<*const u8, String> {
    if let Some(translation) = locale.and_then(|locale| locale.translation(key)) {
        return Ok(translation.as_ptr());
    }
    match missing {
        MissingTranslation::Original => text.original(source, code_page),
        MissingTranslation::Empty => Ok(EMPTY_RECORD.as_ptr()),
        MissingTranslation::Key => Ok(text.store_key(key)),
    }
}

unsafe extern "C" fn patch_resource_dispatch(resource_index: u32) {
    let Some(resource) = MAIN_RESOURCE_BINDINGS.get(resource_index as usize) else {
        return;
    };
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return;
    };
    let start = unsafe {
        ptr::read_volatile((state.module_base + resource.buffer_rva) as *const u32) as usize
    };
    let size = unsafe {
        ptr::read_volatile((state.module_base + resource.size_rva) as *const u32) as usize
    };
    if start == 0 || size == 0 || size > MAX_RESOURCE_BUFFER_SIZE {
        return;
    }
    let Some(end) = start.checked_add(size) else {
        return;
    };
    let image = MemoryRange { start, end };
    unsafe { resource::patch_image(state, resource.id, image) };
}

unsafe fn module_image_size(module_base: usize) -> Option<usize> {
    if module_base == 0 || unsafe { ptr::read_unaligned(module_base as *const u16) } != 0x5A4D {
        return None;
    }
    let pe_offset = unsafe { ptr::read_unaligned((module_base + 0x3C) as *const u32) } as usize;
    if pe_offset > 0x10_0000 {
        return None;
    }
    let pe = module_base.checked_add(pe_offset)?;
    if unsafe { ptr::read_unaligned(pe as *const u32) } != 0x0000_4550 {
        return None;
    }
    let section_count = unsafe { ptr::read_unaligned((pe + 6) as *const u16) } as usize;
    if section_count == 0 || section_count > 96 {
        return None;
    }
    let optional_header_size = unsafe { ptr::read_unaligned((pe + 20) as *const u16) } as usize;
    let optional_header = pe.checked_add(24)?;
    if optional_header_size < 96
        || unsafe { ptr::read_unaligned(optional_header as *const u16) } != 0x010B
    {
        return None;
    }
    let image_size = unsafe { ptr::read_unaligned((optional_header + 56) as *const u32) } as usize;
    if image_size == 0 {
        return None;
    }
    module_base.checked_add(image_size)?;
    Some(image_size)
}

unsafe fn matches_signature(address: usize, signature: &[(usize, u8)]) -> bool {
    signature
        .iter()
        .all(|(offset, expected)| unsafe { *((address + offset) as *const u8) == *expected })
}

fn signature_size(signature: &[(usize, u8)]) -> usize {
    signature
        .iter()
        .map(|(offset, _)| offset + 1)
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
fn test_state(locale: Option<RuntimeLocale>, missing: MissingTranslation) -> HookState {
    let module =
        unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleA(PCSTR::null()) }.unwrap();
    HookState {
        _native: None,
        _module: unsafe { ModuleReference::acquire(module) }.unwrap(),
        module_base: module.0 as usize,
        locale,
        missing,
        text: Mutex::new(TextArena::default()),
        current_stage: AtomicU16::new(tlk::UNKNOWN_STAGE),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::{TextArena, TranslationKey, decode_source, source_code_page};

    fn resource_key(record_id: u32) -> TranslationKey {
        TranslationKey::Resource {
            resource_id: "mhfdat",
            group_id: "table",
            translation_group: 7,
            record_id,
            part: 0,
        }
    }

    #[test]
    fn resource_arena_keeps_originals_and_missing_keys_stable() {
        let mut arena = TextArena::default();
        let original = c"\x83\x65\x83\x58\x83\x67";
        let decoded = arena.original(original, 932).unwrap();
        let first = arena.store_key(resource_key(0));
        let expected = b"[mhfdat:table:0]\0";
        let second = arena.store_key(resource_key(1));
        let second_expected = b"[mhfdat:table:1]\0";

        for record_id in 2..32_768 {
            arena.store_key(resource_key(record_id));
        }

        assert!(arena.storage.iter_allocated_chunks().count() > 1);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(first, expected.len()) },
            expected
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(second, second_expected.len()) },
            second_expected
        );
        assert_eq!(arena.original(original, 932).unwrap(), decoded);
        assert_eq!(
            unsafe { CStr::from_ptr(decoded.cast()) }.to_str().unwrap(),
            "テスト"
        );
    }

    #[test]
    fn source_conversion_preserves_ascii_controls_and_rejects_invalid_cp932() {
        assert_eq!(
            decode_source(b"~C00\x83\x65\x83\x58\x83\x67 %s\n\x1B", 932).unwrap(),
            "~C00テスト %s\n\x1B"
        );
        assert!(decode_source(&[0x81], 932).is_err());

        let mut arena = TextArena::default();
        let ascii = c"~C00ASCII %d";
        assert_eq!(arena.original(ascii, 932).unwrap(), ascii.as_ptr().cast());
    }

    #[test]
    fn localized_resources_use_the_original_language_character_sets() {
        assert_eq!(source_code_page(0).unwrap(), 932);
        assert_eq!(source_code_page(1).unwrap(), 932);
        assert_eq!(source_code_page(6).unwrap(), 949);
        assert_eq!(source_code_page(7).unwrap(), 950);
        assert!(source_code_page(2).is_err());
        assert_eq!(
            decode_source(b"~C00\xC7\xD1\xB1\xDB %s", 949).unwrap(),
            "~C00한글 %s"
        );
        assert_eq!(
            decode_source(b"~C00\xC1\x63\xC5\xE9 %s", 950).unwrap(),
            "~C00繁體 %s"
        );

        let mut arena = TextArena::default();
        let korean = arena.original(c"\xB0\xA1", 949).unwrap();
        let chinese = arena.original(c"\xB0\xA1", 950).unwrap();
        assert_ne!(korean, chinese);
        assert_eq!(
            unsafe { CStr::from_ptr(korean.cast()) }.to_str().unwrap(),
            "가"
        );
        assert_eq!(
            unsafe { CStr::from_ptr(chinese.cast()) }.to_str().unwrap(),
            "陛"
        );
    }

    #[test]
    fn translation_keys_include_the_stable_group_identity() {
        let resource = TranslationKey::Resource {
            resource_id: "mhfdat",
            group_id: "melee_weapon_descriptions",
            translation_group: 16,
            record_id: 42,
            part: 1,
        };
        let quest = TranslationKey::Resource {
            resource_id: "mhfinf",
            group_id: "quest",
            translation_group: 17,
            record_id: 25001,
            part: 7,
        };

        assert_eq!(
            resource.to_string(),
            "mhfdat:melee_weapon_descriptions:42:01"
        );
        assert_eq!(quest.to_string(), "mhfinf:quest:25001:07");
        assert_eq!(
            TranslationKey::Stage {
                stage: 1,
                section: 0x17,
                record: 0x2A
            }
            .to_string(),
            "stage:001:0017:002A"
        );
    }
}
