//! Parse native pointer records and bind scattered constants to localized UTF-8.
//!
//! The source strings remain untouched: some have additional Win32 or fixed-byte
//! consumers. Only the address operands and pointer cells described by the layouts and bindings
//! are replaced. Layouts and bindings were checked against the unpacked IDA image.

use std::{ffi::CStr, ptr, sync::Mutex};

use windows::Win32::{
    Foundation::HMODULE,
    System::{
        Diagnostics::Debug::FlushInstructionCache,
        Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
        Threading::GetCurrentProcess,
    },
};

mod layout;
#[cfg(feature = "translation")]
use super::dictionary::RuntimeLocale;
use super::{
    MemoryRange, ModuleReference, NATIVE_GROUPS, TextArena, TranslationKey, replacement,
    resource::read_image_string,
};
#[cfg(feature = "translation")]
use crate::MissingTranslation;
use layout::{LITERALS, Literal, TABLES, Table};

fn key(group: &'static str, record_id: u32) -> TranslationKey {
    TranslationKey::Resource {
        resource_id: "native",
        group_id: group,
        translation_group: NATIVE_GROUPS
            .iter()
            .find(|(name, _)| *name == group)
            .expect("compiled native group")
            .1,
        record_id,
        part: 0,
    }
}

struct PointerPatch {
    address: usize,
    original: u32,
    replacement: u32,
    /// Retain the original protection if a restoration API call fails.
    protection: Option<PAGE_PROTECTION_FLAGS>,
}

/// Native callers must be stopped while installing or restoring this guard.
/// Its module reference keeps the patched addresses valid through cleanup.
pub(super) struct NativeTextGuard {
    patches: Vec<PointerPatch>,
    module: Option<ModuleReference>,
    text: Option<Mutex<TextArena>>,
}

/// Read image text and prepare pointer replacements before writing any pointer.
/// `base..base + size` must be the readable, unpacked native DLL image.
pub(super) unsafe fn install(
    base: usize,
    size: usize,
    #[cfg(feature = "translation")] locale: Option<&RuntimeLocale>,
    #[cfg(feature = "translation")] missing: MissingTranslation,
) -> Result<NativeTextGuard, String> {
    let mut text = TextArena::default();
    let mut resolve = |group, id, source: &CStr| {
        replacement(
            #[cfg(feature = "translation")]
            locale,
            #[cfg(feature = "translation")]
            missing,
            &mut text,
            key(group, id),
            source,
            932,
        )
    };
    let mut patches = unsafe { prepare(base, size, LITERALS, &mut resolve) }?;
    patches.extend(unsafe { prepare_tables(base, size, TABLES, &mut resolve) }?);
    let module = unsafe { ModuleReference::acquire(HMODULE(base as *mut _)) }?;
    unsafe { apply(patches, Some(module), text) }
}

unsafe fn prepare(
    base: usize,
    size: usize,
    literals: &[Literal],
    resolve: &mut impl FnMut(&'static str, u32, &CStr) -> Result<*const u8, String>,
) -> Result<Vec<PointerPatch>, String> {
    let image = MemoryRange {
        start: base,
        end: base
            .checked_add(size)
            .ok_or_else(|| "native literal image range overflows".to_owned())?,
    };
    let mut patches = Vec::new();
    for literal in literals {
        let source = checked_address(base, size, literal.rva, 1)?;
        let text = unsafe { read_image_string(image, source) }
            .ok_or_else(|| format!("unterminated native literal at RVA {:#010X}", literal.rva))?;
        let original = u32::try_from(source)
            .map_err(|_| "native literals require a 32-bit image".to_owned())?;
        let replacement = u32::try_from(resolve("literal", literal.rva as u32, text)? as usize)
            .map_err(|_| "native literals require 32-bit replacement pointers".to_owned())?;
        for &site in literal.sites {
            let address = checked_address(base, size, site, size_of::<u32>())?;
            if unsafe { ptr::read_unaligned(address as *const u32) } != original {
                return Err(format!(
                    "unsupported native literal pointer at RVA {:#010X}",
                    site
                ));
            }
            patches.push(PointerPatch {
                address,
                original,
                replacement,
                protection: None,
            });
        }
    }
    Ok(patches)
}

unsafe fn prepare_tables(
    base: usize,
    size: usize,
    tables: &[Table],
    resolve: &mut impl FnMut(&'static str, u32, &CStr) -> Result<*const u8, String>,
) -> Result<Vec<PointerPatch>, String> {
    let mut patches = Vec::new();
    for table in tables {
        let source_start = checked_address(
            base,
            size,
            table.source_start,
            table.source_end - table.source_start,
        )?;
        let source_image = MemoryRange {
            start: source_start,
            end: base + table.source_end,
        };
        for record in 0..table.records {
            let address = checked_address(
                base,
                size,
                table.root + record as usize * table.stride + table.text_offset,
                4,
            )?;
            let original = unsafe { ptr::read_unaligned(address as *const u32) };
            let source = original as usize;
            if !source_image.contains(source) {
                return Err(format!(
                    "unsupported native {} record {record} text pointer",
                    table.id
                ));
            }
            let source = unsafe { read_image_string(source_image, source) }
                .ok_or_else(|| format!("unterminated native {} record {record}", table.id))?;
            let replacement = resolve(table.id, record, source)? as usize;
            patches.push(PointerPatch {
                address,
                original,
                replacement: u32::try_from(replacement)
                    .map_err(|_| "native text requires 32-bit pointers")?,
                protection: None,
            });
        }
    }
    Ok(patches)
}

fn checked_address(base: usize, size: usize, rva: usize, length: usize) -> Result<usize, String> {
    if rva.checked_add(length).is_none_or(|end| end > size) {
        return Err(format!("native literal RVA {rva:#010X} exceeds the image"));
    }
    base.checked_add(rva)
        .ok_or_else(|| "native literal address overflows".to_owned())
}

unsafe fn apply(
    patches: Vec<PointerPatch>,
    module: Option<ModuleReference>,
    text: TextArena,
) -> Result<NativeTextGuard, String> {
    let mut guard = NativeTextGuard {
        patches: Vec::with_capacity(patches.len()),
        module,
        text: Some(Mutex::new(text)),
    };
    for patch in patches {
        // Journal before writing: failures may occur after the pointer changed.
        guard.patches.push(patch);
        let patch = guard.patches.last_mut().unwrap();
        let replacement = patch.replacement;
        if let Err(error) = unsafe { write_pointer(patch, replacement) } {
            let rollback = unsafe { guard.restore() };
            return Err(match rollback {
                Ok(()) => error,
                Err(rollback) => format!("{error}; literal rollback failed: {rollback}"),
            });
        }
    }
    Ok(guard)
}

impl NativeTextGuard {
    /// Restore only our own replacements. An unrelated pointer change is an
    /// error, and is left intact so cleanup cannot overwrite another owner.
    pub(super) unsafe fn restore(&mut self) -> Result<(), String> {
        while let Some(patch) = self.patches.last_mut() {
            let current = unsafe { ptr::read_unaligned(patch.address as *const u32) };
            if current != patch.original && current != patch.replacement {
                return Err(format!(
                    "native literal pointer at {:#010X} changed after installation",
                    patch.address
                ));
            }
            if current != patch.original || patch.protection.is_some() {
                let original = patch.original;
                unsafe { write_pointer(patch, original) }?;
            }
            self.patches.pop();
        }
        Ok(())
    }
}

impl Drop for NativeTextGuard {
    fn drop(&mut self) {
        if let Err(error) = unsafe { self.restore() } {
            // Retain the arena and target DLL while a native pointer still uses them.
            if let Some(text) = self.text.take() {
                std::mem::forget(text);
            }
            if let Some(module) = self.module.take() {
                std::mem::forget(module);
            }
            eprintln!("failed to restore native UTF-8 text: {error}");
        }
    }
}

unsafe fn write_pointer(patch: &mut PointerPatch, value: u32) -> Result<(), String> {
    let address = patch.address as *mut u32;
    let mut old = PAGE_PROTECTION_FLAGS::default();
    unsafe {
        VirtualProtect(
            address.cast(),
            size_of::<u32>(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        )
    }
    .map_err(|error| format!("failed to make literal pointer writable: {error}"))?;
    let protection = *patch.protection.get_or_insert(old);
    unsafe { ptr::write_unaligned(address, value) };
    let flushed = unsafe {
        FlushInstructionCache(GetCurrentProcess(), Some(address.cast()), size_of::<u32>())
    };
    let restored =
        unsafe { VirtualProtect(address.cast(), size_of::<u32>(), protection, &mut old) };
    if restored.is_ok() {
        patch.protection = None;
    }
    flushed
        .map_err(|error| format!("failed to flush native literal instruction cache: {error}"))?;
    restored.map_err(|error| format!("failed to restore native literal page protection: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use windows::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
        PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualQuery,
    };

    #[test]
    fn record_reader_uses_stride_and_identity_even_when_text_pointers_are_shared() {
        let page = Page::new();
        let base = page.0 as usize;
        let tables = [Table {
            id: "rank",
            root: 0x40,
            records: 2,
            stride: 8,
            text_offset: 0,
            source_start: 0x80,
            source_end: 0x85,
        }];
        unsafe {
            ptr::copy_nonoverlapping(c"\x82\xc6%s".as_ptr().cast(), page.0.add(0x80), 5);
            for (index, metadata) in [(0, 2u32), (1, 3)] {
                ptr::write_unaligned(
                    page.0.add(0x40 + index * 8).cast::<u32>(),
                    (base + 0x80) as u32,
                );
                ptr::write_unaligned(page.0.add(0x44 + index * 8).cast::<u32>(), metadata);
            }
            let patches = prepare_tables(base, 4096, &tables, &mut |group, id, source| {
                assert_eq!(group, "rank");
                assert_eq!(source.to_bytes(), b"\x82\xc6%s");
                Ok(if id == 0 { c"第一" } else { c"第二" }.as_ptr().cast())
            })
            .unwrap();
            let mut guard = apply(patches, None, TextArena::default()).unwrap();
            for (index, expected) in [(0, c"第一"), (1, c"第二")] {
                let value = ptr::read_unaligned(page.0.add(0x40 + index * 8).cast::<u32>());
                assert_eq!(CStr::from_ptr(value as *const _), expected);
                assert_eq!(
                    ptr::read_unaligned(page.0.add(0x44 + index * 8).cast::<u32>()),
                    index as u32 + 2
                );
            }
            guard.restore().unwrap();
            for index in 0..2 {
                assert_eq!(
                    ptr::read_unaligned(page.0.add(0x40 + index * 8).cast::<u32>()),
                    (base + 0x80) as u32
                );
            }
            ptr::write_unaligned(page.0.add(0x48).cast::<u32>(), (base + 0x90) as u32);
            assert!(
                prepare_tables(base, 4096, &tables, &mut |_, _, _| Ok(c"X".as_ptr().cast()))
                    .is_err()
            );
            assert_eq!(
                ptr::read_unaligned(page.0.add(0x40).cast::<u32>()),
                (base + 0x80) as u32
            );
        }
    }

    #[cfg(feature = "translation")]
    #[test]
    fn native_keys_share_locale_selection_and_missing_policy() {
        let locale = super::super::TRANSLATION_DICTIONARY
            .locale("ja-JP")
            .unwrap();
        let mut arena = TextArena::default();
        let original = c"\x82\xc6";
        for (group, id, expected) in [("rank", 4, "ＧＲ"), ("room", 0, "空き")] {
            let pointer = replacement(
                Some(&locale),
                MissingTranslation::Empty,
                &mut arena,
                key(group, id),
                original,
                932,
            )
            .unwrap();
            assert_eq!(
                unsafe { CStr::from_ptr(pointer.cast()) }.to_str().unwrap(),
                expected
            );
        }
        for (missing, expected) in [
            (MissingTranslation::Original, "と"),
            (MissingTranslation::Empty, ""),
            (MissingTranslation::Key, "[native:rank:0]"),
        ] {
            let pointer =
                replacement(None, missing, &mut arena, key("rank", 0), original, 932).unwrap();
            assert_eq!(
                unsafe { CStr::from_ptr(pointer.cast()) }.to_str().unwrap(),
                expected
            );
        }
    }

    #[test]
    fn bindings_have_unique_pointer_sites() {
        let mut sites = HashSet::new();
        for literal in LITERALS {
            for &site in literal.sites {
                assert!(sites.insert(site), "duplicate pointer {:#x}", site);
                // No audited four-byte operand crosses a native x86 page.
                assert!(site & 0xFFF <= 0xFFC);
            }
        }
    }

    struct Page(*mut u8);

    impl Page {
        fn new() -> Self {
            let page =
                unsafe { VirtualAlloc(None, 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
            assert!(!page.is_null());
            Self(page.cast())
        }

        unsafe fn protection(&self) -> PAGE_PROTECTION_FLAGS {
            let mut info = MEMORY_BASIC_INFORMATION::default();
            assert_ne!(
                unsafe {
                    VirtualQuery(
                        Some(self.0.cast()),
                        &mut info,
                        size_of::<MEMORY_BASIC_INFORMATION>(),
                    )
                },
                0
            );
            info.Protect
        }
    }

    impl Drop for Page {
        fn drop(&mut self) {
            unsafe { VirtualFree(self.0.cast(), 0, MEM_RELEASE) }.unwrap();
        }
    }

    const TEST_LITERALS: &[Literal] = &[Literal {
        rva: 0x80,
        sites: &[0x11, 0x40],
    }];

    #[test]
    fn patches_code_and_data_pointers_and_restores_bytes_and_page_protection() {
        let page = Page::new();
        let base = page.0 as usize;
        unsafe {
            page.0.add(0x10).write(0x68);
            ptr::copy_nonoverlapping(c"\x82\xc6%s".as_ptr().cast(), page.0.add(0x80), 5);
            ptr::write_unaligned(page.0.add(0x11).cast::<u32>(), (base + 0x80) as u32);
            ptr::write_unaligned(page.0.add(0x40).cast::<u32>(), (base + 0x80) as u32);
            let mut old = PAGE_PROTECTION_FLAGS::default();
            VirtualProtect(page.0.cast(), 4096, PAGE_EXECUTE_READ, &mut old).unwrap();
            let patches = prepare(base, 4096, TEST_LITERALS, &mut |_, _, source| {
                assert_eq!(source, c"\x82\xc6%s");
                Ok(c"と%s".as_ptr().cast())
            })
            .unwrap();
            let mut guard = apply(patches, None, TextArena::default()).unwrap();
            for rva in [0x11, 0x40] {
                assert_eq!(
                    CStr::from_ptr(ptr::read_unaligned(page.0.add(rva).cast::<u32>()) as *const _),
                    c"と%s"
                );
            }
            assert_eq!(page.protection(), PAGE_EXECUTE_READ);
            assert_eq!(
                std::slice::from_raw_parts(page.0.add(0x80), 5),
                b"\x82\xc6%s\0"
            );
            guard.restore().unwrap();
            for rva in [0x11, 0x40] {
                assert_eq!(
                    ptr::read_unaligned(page.0.add(rva).cast::<u32>()),
                    (base + 0x80) as u32
                );
            }
            assert_eq!(page.protection(), PAGE_EXECUTE_READ);
        }
    }

    #[test]
    fn rejects_an_unsupported_pointer_before_changing_any_site() {
        let page = Page::new();
        let base = page.0 as usize;
        unsafe {
            page.0.add(0x10).write(0x68);
            ptr::copy_nonoverlapping(c"\x82\xc6%s".as_ptr().cast(), page.0.add(0x80), 5);
            ptr::write_unaligned(page.0.add(0x11).cast::<u32>(), (base + 0x80) as u32);
            ptr::write_unaligned(page.0.add(0x40).cast::<u32>(), 0x1234);
            assert!(
                prepare(base, 4096, TEST_LITERALS, &mut |_, _, _| Ok(c"と%s"
                    .as_ptr()
                    .cast()))
                .is_err()
            );
            assert_eq!(
                ptr::read_unaligned(page.0.add(0x11).cast::<u32>()),
                (base + 0x80) as u32
            );
            assert_eq!(ptr::read_unaligned(page.0.add(0x40).cast::<u32>()), 0x1234);
        }
    }
}
