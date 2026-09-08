//! Four old layout tables address a suffix by its display-column offset. Keep
//! that address arithmetic at the ingress, then append ordinary UTF-8 spaces.

use super::{CodeHook, HOOK_STATE, signature};
use std::{
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

const TABLES: &[(usize, usize)] = &[
    (0x019A0418, 44),
    (0x019A0448, 36),
    (0x019A0470, 30),
    (0x019A0490, 28),
];

const PADDING: [[u8; 68]; 45] = padding_strings();

const fn padding_strings() -> [[u8; 68]; 45] {
    let mut strings = [[0; 68]; 45];
    let mut columns = 0;
    while columns < strings.len() {
        let mut offset = 0;
        if columns % 2 != 0 {
            strings[columns][0] = b' ';
            offset = 1;
        }
        let end = offset + columns / 2 * 3;
        while offset < end {
            strings[columns][offset] = 0xE3;
            strings[columns][offset + 1] = 0x80;
            strings[columns][offset + 2] = 0x80;
            offset += 3;
        }
        columns += 1;
    }
    strings
}

pub(super) unsafe fn validate(base: usize, size: usize) -> Result<(), String> {
    for &(rva, columns) in TABLES {
        if rva + columns >= size {
            return Err(format!("native padding RVA {rva:#010X} exceeds image"));
        }
        for offset in 0..=columns {
            let expected = if offset == columns {
                0
            } else if offset % 2 == 0 {
                0x81
            } else {
                0x40
            };
            if unsafe { ptr::read((base + rva + offset) as *const u8) } != expected {
                return Err(format!("unsupported native padding at RVA {rva:#010X}"));
            }
        }
    }
    Ok(())
}

fn padding(source_rva: usize) -> Option<&'static [u8; 68]> {
    TABLES.iter().find_map(|&(rva, columns)| {
        let offset = source_rva.checked_sub(rva)?;
        (offset <= columns).then(|| &PADDING[columns - offset])
    })
}

type Append = unsafe extern "C" fn(*mut u8, usize, *const u8) -> i32;
static APPEND_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
const APPEND_SIGNATURE: &[(usize, u8)] =
    &signature([0x8B, 0xFF, 0x55, 0x8B, 0xEC, 0x8B, 0x4D, 0x08]);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![CodeHook {
        name: "UTF-8 indexed layout padding",
        rva: 0x015AD392,
        signature: APPEND_SIGNATURE,
        detour: append_hook as *const () as *mut _,
        original: &APPEND_ORIGINAL,
    }]
}

unsafe extern "C" fn append_hook(destination: *mut u8, capacity: usize, source: *const u8) -> i32 {
    let invocation = HOOK_STATE.enter();
    let original: Append = unsafe { std::mem::transmute(APPEND_ORIGINAL.load(Ordering::Acquire)) };
    let source = invocation
        .state()
        .and_then(|state| (source as usize).checked_sub(state.module_base))
        .and_then(padding)
        .map_or(source, |text| text.as_ptr());
    unsafe { original(destination, capacity, source) }
}

#[cfg(test)]
mod tests {
    use crate::text::utf8::display_columns;
    use std::ffi::CStr;

    #[test]
    fn indexed_padding_keeps_even_and_odd_display_columns() {
        for &(rva, columns) in super::TABLES {
            for offset in 0..=columns {
                let bytes = super::padding(rva + offset).unwrap();
                let text = CStr::from_bytes_until_nul(bytes).unwrap().to_str().unwrap();
                assert_eq!(display_columns(text), columns - offset);
                assert!(text.chars().all(|c| c == ' ' || c == '　'));
            }
            assert!(super::padding(rva + columns + 1).is_none());
        }
    }
}
