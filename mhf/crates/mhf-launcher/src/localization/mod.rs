use crate::MissingTranslation;
use bumpalo::Bump;
use mhf_hooks::{HookGuard, HookSlot};
use std::{
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
        System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExA},
    },
    core::PCSTR,
};

use dictionary::{CompiledDictionary, CompiledLocale, RuntimeLocale};

mod dictionary;
mod rendering;
mod resource;
mod tlk;

static EMPTY_RECORD: [u8; 1] = [0];

const MAX_RESOURCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TranslationKey {
    resource_id: &'static str,
    group_id: &'static str,
    translation_group: u32,
    record_id: u32,
    part: u16,
}

impl fmt::Display for TranslationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}",
            self.resource_id, self.group_id, self.record_id
        )?;
        if self.part != 0 {
            write!(formatter, ":{:02}", self.part)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct MissingKeyArena {
    storage: Bump,
    scratch: String,
}

impl MissingKeyArena {
    fn store(&mut self, key: TranslationKey) -> *const u8 {
        self.scratch.clear();
        self.scratch.push('[');
        write!(&mut self.scratch, "{key}").expect("writing to a String cannot fail");
        self.scratch.push(']');
        self.scratch.push('\0');
        self.storage
            .alloc_slice_copy(self.scratch.as_bytes())
            .as_ptr()
    }
}

#[derive(Clone, Copy)]
struct ResourceLayout {
    id: &'static str,
    magic: u32,
    format_version: u32,
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
    root: u32,
    records: u32,
    text_offset: u16,
    parts: u16,
    stride: u16,
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
                "push {resource_index}",
                "call {dispatch}",
                "add esp, 4",
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
    // Release the DLL before the buffers it may still reference in DllMain.
    _module: ModuleReference,
    module_base: usize,
    locale: RuntimeLocale,
    missing: MissingTranslation,
    missing_keys: Mutex<MissingKeyArena>,
    current_stage: AtomicU16,
    rendering: rendering::RenderingHooks,
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
    locale: &str,
    missing: MissingTranslation,
    font_name: &[u8],
) -> Result<HookGuard<HookState>, String> {
    let mut hooks = HOOK_STATE.prepare()?;
    let dictionary = &TRANSLATION_DICTIONARY;
    let compiled_locale = dictionary.locale(locale).ok_or_else(|| {
        let available = dictionary.locale_ids().collect::<Vec<_>>().join(", ");
        format!("translation locale {locale:?} is not embedded; available locales: {available}")
    })?;
    let font_name = CStr::from_bytes_until_nul(font_name)
        .map_err(|_| "configured font name is not NUL-terminated".to_owned())?;

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

    let locale = dictionary.encode_locale(compiled_locale);

    for hook in main_resource_hooks
        .into_iter()
        .chain(dynamic_resource_hooks)
    {
        let target = module_base + hook.rva;
        let trampoline = unsafe { hooks.create(hook.name, target as *mut c_void, hook.detour) }?;
        hook.original.store(trampoline as usize, Ordering::Release);
    }

    let rendering = unsafe { rendering::create_hooks(&mut hooks, font_name) }?;

    let state = HookState {
        _module: retained_module,
        module_base,
        locale,
        missing,
        missing_keys: Mutex::new(MissingKeyArena::default()),
        current_stage: AtomicU16::new(tlk::UNKNOWN_STAGE),
        rendering,
    };
    unsafe { hooks.install(state) }
}

fn replacement_for_key(
    state: &HookState,
    missing_keys: Option<&mut MissingKeyArena>,
    key: TranslationKey,
) -> Option<*const u8> {
    if let Some(translation) = state.locale.translation(key) {
        return Some(translation.as_ptr());
    }
    match state.missing {
        MissingTranslation::Original => None,
        MissingTranslation::Empty => Some(EMPTY_RECORD.as_ptr()),
        MissingTranslation::Key => missing_keys.map(|arena| arena.store(key)),
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
mod tests {
    use super::{MissingKeyArena, TranslationKey};

    fn resource_key(record_id: u32) -> TranslationKey {
        TranslationKey {
            resource_id: "mhfdat",
            group_id: "table",
            translation_group: 7,
            record_id,
            part: 0,
        }
    }

    #[test]
    fn missing_key_arena_keeps_record_pointers_stable() {
        let mut arena = MissingKeyArena::default();
        let first = arena.store(resource_key(0));
        let expected = b"[mhfdat:table:0]\0";
        let second = arena.store(resource_key(1));
        let second_expected = b"[mhfdat:table:1]\0";

        for record_id in 2..32_768 {
            arena.store(resource_key(record_id));
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
    }

    #[test]
    fn translation_keys_include_the_stable_group_identity() {
        let resource = TranslationKey {
            resource_id: "mhfdat",
            group_id: "table_016",
            translation_group: 16,
            record_id: 42,
            part: 1,
        };
        let quest = TranslationKey {
            resource_id: "mhfinf",
            group_id: "quest",
            translation_group: 17,
            record_id: 25001,
            part: 7,
        };

        assert_eq!(resource.to_string(), "mhfdat:table_016:42:01");
        assert_eq!(quest.to_string(), "mhfinf:quest:25001:07");
    }
}
