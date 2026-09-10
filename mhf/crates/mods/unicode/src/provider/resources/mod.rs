//! Convert known native resource ingress to UTF-8 and retain patched text.

use crate::decode_source;
use bumpalo::Bump;
use mhf_hooks::{HookGuard, HookSlot, ModuleReference};
use mhf_translation::Key;
use mhf_translation::{Translation, TranslationTable};
use std::{
    collections::HashMap,
    ffi::{CStr, c_void},
    mem::size_of,
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
};
use windows::Win32::Foundation::HMODULE;

#[cfg(test)]
mod extra_resource_tests;
mod native;
mod resource;
#[cfg(test)]
mod test_translation;
mod tlk;

const MAX_RESOURCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

#[derive(Default)]
struct TextArena {
    storage: Bump,
    originals: HashMap<u32, HashMap<Vec<u8>, usize>>,
}

impl TextArena {
    fn store_override(&self, text: &[u8]) -> *const u8 {
        self.storage.alloc_slice_copy(text).as_ptr()
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

include!(concat!(env!("OUT_DIR"), "/resources.rs"));

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
    native: Option<native::NativeTextGuard>,
    // Release explicitly while the host retains the DLL and we retain its buffers.
    module: ModuleReference,
    module_base: usize,
    translation: Option<Translation<'static>>,
    text: Mutex<TextArena>,
    current_stage: AtomicU16,
}

impl HookState {
    /// Restore image pointers and release our DLL references after all native
    /// hooks detach. Keep this state alive through the host's final DLL release.
    pub(crate) unsafe fn prepare_release(&mut self) -> Result<(), String> {
        if let Some(native) = &mut self.native {
            unsafe { native.prepare_release() }?;
        }
        unsafe { self.module.release() }
    }

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

/// The module must be valid, and its native callers must stop before cleanup:
/// the naked shims use their trampolines outside the Rust callback invocation.
/// A translation provider must remain alive through this guard's final release.
pub(crate) unsafe fn install(
    module: HMODULE,
    translation: Option<*const TranslationTable>,
) -> Result<HookGuard<HookState>, String> {
    let mut hooks = HOOK_STATE.prepare()?;
    // HookSlot owns static callback state. The Mod host retains this dependency
    // until after the guard is destroyed, including failed cleanup.
    let translation = translation.map(|table| unsafe { mhf_translation::bind(table) });

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
        .expect("text resource hooks use at least one RVA");
    if image_size < required_end {
        return Err(format!(
            "mhfo module image is too small for text resource hooks: 0x{:X}",
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

    let native = unsafe { native::install(module_base, image_size, translation) }?;
    let state = HookState {
        native: Some(native),
        module: retained_module,
        module_base,
        translation,
        text: Mutex::new(TextArena::default()),
        current_stage: AtomicU16::new(tlk::UNKNOWN_STAGE),
    };
    unsafe { hooks.install(state) }
}

fn replacement_for_key(
    state: &HookState,
    text: &mut TextArena,
    key: Key,
    source: &CStr,
    code_page: u32,
) -> Result<*const u8, String> {
    replacement(state.translation, text, key, source, code_page)
}

fn replacement(
    translation: Option<Translation<'_>>,
    text: &mut TextArena,
    key: Key<'_>,
    source: &CStr,
    code_page: u32,
) -> Result<*const u8, String> {
    if let Some(provider) = translation
        && let Some(mut translation) = provider.resolve(key).map_err(|error| error.to_string())?
    {
        translation.push('\0');
        return Ok(text.store_override(translation.as_bytes()));
    }
    text.original(source, code_page)
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
fn test_state() -> HookState {
    let module = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleA(windows::core::PCSTR::null())
    }
    .unwrap();
    HookState {
        native: None,
        module: unsafe { ModuleReference::acquire(module) }.unwrap(),
        module_base: module.0 as usize,
        translation: None,
        text: Mutex::new(TextArena::default()),
        current_stage: AtomicU16::new(tlk::UNKNOWN_STAGE),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::test_translation::TestTranslation;
    use super::{TextArena, decode_source, source_code_page};
    use mhf_translation::{Key, MissingTranslation};

    #[test]
    fn external_c_provider_results_are_copied_into_the_resource_arena() {
        use mhf_mod_sdk::abi as api;
        use mhf_translation::{Key as PublicKey, TranslationApi, TranslationTable};
        struct External(String);
        impl TranslationApi for External {
            fn resolve(
                &self,
                _key: PublicKey<'_>,
                mut buffer: safer_ffi::slice::Mut<'_, u8>,
                required: &mut usize,
            ) -> api::Status {
                let text = &self.0;
                *required = text.len();
                if buffer.len() < text.len() {
                    return api::BUFFER_TOO_SMALL;
                }
                buffer[..text.len()].copy_from_slice(text.as_bytes());
                api::OK
            }
        }
        let mut arena = TextArena::default();
        let pointer = {
            let table: TranslationTable = Box::new(External(String::from("外部翻译"))).into();
            super::replacement(
                Some(unsafe { mhf_translation::bind(&table) }),
                &mut arena,
                Key::stage(1, 23, 2),
                c"original",
                932,
            )
            .unwrap()
        };
        // The provider's String and table are gone; native pointers only refer
        // to our arena, including the NUL added at this resource boundary.
        assert_eq!(
            unsafe { CStr::from_ptr(pointer.cast()) }.to_str().unwrap(),
            "外部翻译"
        );
    }

    fn resource_key(record_id: u32) -> Key<'static> {
        Key::resource("mhfdat", "table", record_id, 0)
    }

    #[test]
    fn resource_arena_keeps_originals_and_missing_keys_stable() {
        let mut arena = TextArena::default();
        let original = c"\x83\x65\x83\x58\x83\x67";
        let decoded = arena.original(original, 932).unwrap();
        let provider = TestTranslation::with_locale(None, MissingTranslation::Key);
        let translations = unsafe { mhf_translation::bind(provider.api()) };
        let first = arena.store_override(
            format!(
                "{}\0",
                translations.resolve(resource_key(0)).unwrap().unwrap()
            )
            .as_bytes(),
        );
        let expected = b"[mhfdat:table:0]\0";
        let second = arena.store_override(
            format!(
                "{}\0",
                translations.resolve(resource_key(1)).unwrap().unwrap()
            )
            .as_bytes(),
        );
        let second_expected = b"[mhfdat:table:1]\0";

        for record_id in 2..32_768 {
            arena.store_override(
                format!(
                    "{}\0",
                    translations
                        .resolve(resource_key(record_id))
                        .unwrap()
                        .unwrap()
                )
                .as_bytes(),
            );
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
        assert_eq!(decode_source(b"", 932).unwrap(), "");
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
}
