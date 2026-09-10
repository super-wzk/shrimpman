//! UTF-8 text processing for the supported native client.
//!
//! Native pointers, buffers and cursors retain their byte-based ABI. Character
//! boundaries and display widths are handled separately from those byte counts.

mod editor;
mod gdi;
pub(crate) mod ime;
mod input;
mod native;
mod rendering;
pub(crate) mod resources;
pub(crate) mod utf8;

pub use gdi::renderer as gdi_renderer;
mod module;
pub use module::UnicodeMod;

use std::{
    ffi::c_void,
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use mhf_hooks::{HookGuard, HookSlot, ModuleReference};
use windows::Win32::Foundation::HMODULE;

pub(super) struct CodeHook {
    pub(super) name: &'static str,
    pub(super) rva: usize,
    pub(super) signature: &'static [(usize, u8)],
    pub(super) detour: *mut c_void,
    pub(super) original: &'static AtomicUsize,
}

pub(crate) struct HookState {
    // Release after callbacks stop; keep glyph data through the final DllMain.
    module: ModuleReference,
    pub(super) module_base: usize,
    glyphs: Mutex<rendering::GlyphCache>,
}

impl HookState {
    /// Call after all native hooks detach, while the host still owns the DLL.
    pub(crate) unsafe fn prepare_release(&mut self) -> Result<(), String> {
        unsafe { self.module.release() }
    }
}

pub(super) static HOOK_STATE: HookSlot<HookState> = HookSlot::new();

/// Install after the IME adapter has validated the original editor entrypoints.
/// All native callers must stop before this guard is uninstalled.
pub(crate) unsafe fn install(module: HMODULE) -> Result<HookGuard<HookState>, String> {
    let mut hooks = HOOK_STATE.prepare()?;
    let base = module.0 as usize;
    let size = unsafe { image_size(base) }
        .ok_or_else(|| "UTF-8 text hooks require a valid i686 PE image".to_owned())?;
    unsafe { native::validate(base, size) }?;
    let entries = native::code_hooks()
        .into_iter()
        .chain(editor::code_hooks())
        .chain(input::code_hooks())
        .chain(rendering::code_hooks())
        .collect::<Vec<_>>();
    for entry in &entries {
        let end = entry
            .signature
            .iter()
            .map(|(offset, _)| offset + 1)
            .max()
            .unwrap_or(1);
        if entry.rva.checked_add(end).is_none_or(|end| end > size)
            || !entry.signature.iter().all(|(offset, byte)| unsafe {
                ptr::read((base + entry.rva + offset) as *const u8) == *byte
            })
        {
            return Err(format!(
                "unsupported {} at RVA {:#010X}",
                entry.name, entry.rva
            ));
        }
    }
    let retained = unsafe { ModuleReference::acquire(module) }?;
    for entry in entries {
        let original =
            unsafe { hooks.create(entry.name, (base + entry.rva) as *mut c_void, entry.detour) }?;
        entry.original.store(original as usize, Ordering::Release);
    }
    unsafe {
        hooks.install(HookState {
            module: retained,
            module_base: base,
            glyphs: Mutex::new(rendering::GlyphCache::default()),
        })
    }
}

unsafe fn image_size(base: usize) -> Option<usize> {
    if base == 0 || unsafe { ptr::read_unaligned(base as *const u16) } != 0x5A4D {
        return None;
    }
    let pe = unsafe { ptr::read_unaligned((base + 0x3C) as *const u32) } as usize;
    if pe > 0x1000
        || unsafe { ptr::read_unaligned((base + pe) as *const u32) } != 0x4550
        || unsafe { ptr::read_unaligned((base + pe + 24) as *const u16) } != 0x010B
    {
        return None;
    }
    let size = unsafe { ptr::read_unaligned((base + pe + 24 + 56) as *const u32) } as usize;
    (size >= rendering::required_image_end()).then_some(size)
}

#[cfg(test)]
mod tests {
    use windows::{
        Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW},
        core::PCWSTR,
    };

    #[test]
    #[ignore = "set MHF_UTF8_TEST_CLIENT to the supported game DLL; loads its real DllMain"]
    fn supported_client_installs_and_removes_text_hooks() {
        let path = std::env::var("MHF_UTF8_TEST_CLIENT").expect("MHF_UTF8_TEST_CLIENT");
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let module =
            unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
                .expect("load native game DLL and its adjacent dependencies");
        let retained = unsafe { super::ModuleReference::from_owned(module) };
        for _ in 0..2 {
            let mut resources = unsafe { super::resources::install(module, None) }
                .expect("install native and resource Unicode text");
            let mut font = unsafe {
                mhf_font::install_game(c"Arial".to_bytes_with_nul(), Some(super::gdi_renderer()))
            }
            .expect("install font corrections with Unicode rendering");
            let mut hooks = unsafe { super::install(module) }
                .expect("install hooks against the unpacked native image");
            unsafe { super::native::verify_short_mail_crt(module.0 as usize) }
                .expect("round-trip UTF-8 short-mail paths through the game's own CRT");
            unsafe { super::native::verify_controller_name_crt(module.0 as usize) }
                .expect("serialize the controller name through the game's own CRT");
            type Printf = unsafe extern "C" fn(*mut u8, usize, *const u8, ...) -> i32;
            let printf: Printf = unsafe { std::mem::transmute(module.0 as usize + 0x015AC59C) };
            let mut buffer = [0u8; 128];
            let count = unsafe {
                printf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"名:%-6.6s/%04d%%".as_ptr().cast(),
                    c"中文A".as_ptr(),
                    7i32,
                )
            };
            let output = std::ffi::CStr::from_bytes_until_nul(&buffer).unwrap();
            assert_eq!(output.to_str().unwrap(), "名:中文  /0007%");
            assert_eq!(count as usize, output.to_bytes().len());
            unsafe {
                printf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*.*s/%I64x/%.*f".as_ptr().cast(),
                    -6i32,
                    6i32,
                    c"中文A".as_ptr(),
                    0x0123456789ABCDEFu64,
                    2i32,
                    2.0f64,
                );
            }
            assert_eq!(
                std::ffi::CStr::from_bytes_until_nul(&buffer)
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "中文  /123456789abcdef/2.00"
            );
            hooks.uninstall().expect("remove native text hooks");
            font.uninstall().expect("remove native font hooks");
            resources
                .uninstall()
                .expect("restore native text pointers and resource hooks");
        }
        drop(retained);
    }
}
