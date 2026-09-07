use std::{
    ffi::{CStr, c_void},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::{CodeHook, HOOK_STATE, utf8};

const EDITOR_POINTER_RVA: usize = 0x0EDB_A1BC;
const TEXT_OFFSET: usize = 43;
const CURSOR_OFFSET: usize = 16;
const TEXT_CAPACITY: usize = 256;
const MAX_TEXT_BYTES: usize = TEXT_CAPACITY - 1;

static INITIALIZE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static INSERT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static PREVIOUS_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static NEXT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

// F330: AX = byte limit, ECX = style, stack = mode, source. FBA0:
// ECX = source, stack = destination, cursor, limit. FD40: EDX = source,
// stack = cursor. All three leave their native arguments for the caller to pop.
macro_rules! initialize_bridge {
    ($name:ident, $dispatch:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push dword ptr [esp + 8]",
                "push dword ptr [esp + 8]",
                "push ecx",
                "movzx eax, ax",
                "push eax",
                "call {dispatch}",
                "add esp, 16",
                "ret",
                dispatch = sym $dispatch,
            );
        }
    };
}

macro_rules! insertion_bridge {
    ($name:ident, $dispatch:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push dword ptr [esp + 12]",
                "push dword ptr [esp + 12]",
                "push dword ptr [esp + 12]",
                "push ecx",
                "call {dispatch}",
                "add esp, 16",
                "ret",
                dispatch = sym $dispatch,
            );
        }
    };
}

macro_rules! previous_bridge {
    ($name:ident, $dispatch:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push dword ptr [esp + 4]",
                "push edx",
                "call {dispatch}",
                "add esp, 8",
                "ret",
                dispatch = sym $dispatch,
            );
        }
    };
}

initialize_bridge!(initialize_detour, initialize_dispatch);
insertion_bridge!(insert_detour, insert_dispatch);
previous_bridge!(previous_detour, previous_dispatch);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "mhfo UTF-8 editor initialization",
            rva: 0x0080_F330,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x8D),
                (4, 0x0C),
                (5, 0xC9),
                (6, 0x53),
                (7, 0x8D),
                (8, 0x14),
                (9, 0x8D),
                (14, 0x56),
                (15, 0x8B),
                (16, 0x35),
                (21, 0x33),
                (22, 0xDB),
                (23, 0xB9),
                (24, 0x00),
                (25, 0x01),
                (26, 0x00),
                (27, 0x00),
            ],
            detour: initialize_detour as *const () as *mut c_void,
            original: &INITIALIZE_ORIGINAL,
        },
        CodeHook {
            name: "mhfo UTF-8 editor insertion",
            rva: 0x0080_FBA0,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x81),
                (4, 0xEC),
                (5, 0x20),
                (6, 0x02),
                (7, 0x00),
                (8, 0x00),
                (9, 0xA1),
                (14, 0x33),
                (15, 0xC5),
                (16, 0x89),
                (17, 0x45),
                (18, 0xFC),
                (19, 0x8B),
                (20, 0x45),
                (21, 0x0C),
                (22, 0x53),
                (23, 0x8B),
                (24, 0x5D),
                (25, 0x08),
                (26, 0x56),
            ],
            detour: insert_detour as *const () as *mut c_void,
            original: &INSERT_ORIGINAL,
        },
        CodeHook {
            name: "mhfo UTF-8 previous character",
            rva: 0x0080_FD40,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x56),
                (4, 0x33),
                (5, 0xC0),
                (6, 0x33),
                (7, 0xF6),
                (8, 0x38),
                (9, 0x02),
                (10, 0x74),
                (11, 0x48),
                (12, 0x53),
                (13, 0x57),
                (14, 0x8B),
                (15, 0xFF),
                (16, 0x3B),
                (17, 0x75),
                (18, 0x08),
            ],
            detour: previous_detour as *const () as *mut c_void,
            original: &PREVIOUS_ORIGINAL,
        },
        CodeHook {
            name: "mhfo UTF-8 next character",
            rva: 0x0080_FDA0,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x51),
                (4, 0x57),
                (5, 0x8B),
                (6, 0x7D),
                (7, 0x08),
                (8, 0x8B),
                (9, 0xC7),
                (10, 0x8D),
                (11, 0x50),
                (12, 0x01),
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
            ],
            detour: next_detour as *const () as *mut c_void,
            original: &NEXT_ORIGINAL,
        },
    ]
}

unsafe extern "C" fn initialize_dispatch(
    limit: u32,
    style: u32,
    mode: u32,
    source: *const u8,
) -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let original = INITIALIZE_ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        return 0;
    }
    let limit = usize::from(limit as u16).min(MAX_TEXT_BYTES);
    // Source may point into the current editor, which initialization clears.
    let initial = unsafe { native_text(source) }
        .map(|text| initial_text(text, limit))
        .unwrap_or_default();
    // Keep the native style, state reset, mode and activation notification, while
    // bypassing only its legacy source-copy and character-validation branch.
    let result = unsafe { call_initialize(original, limit as u32, style, mode, ptr::null()) };
    let editor =
        unsafe { ptr::read_volatile((state.module_base + EDITOR_POINTER_RVA) as *const u32) }
            as usize;
    if editor != 0 {
        unsafe { set_initial_text(editor, &initial) };
    }
    result
}

unsafe extern "C" fn insert_dispatch(
    source: *const u8,
    destination: *mut u8,
    cursor: i32,
    limit: i32,
) -> u32 {
    let invocation = HOOK_STATE.enter();
    if invocation.state().is_none() || destination.is_null() {
        return 0;
    }
    let (Ok(cursor), Ok(limit)) = (usize::try_from(cursor), usize::try_from(limit)) else {
        return 0;
    };
    let Some(text) = (unsafe { native_text(source) }) else {
        return 0;
    };
    // Source and destination can overlap. End the shared native borrow before
    // creating a mutable slice, and reserve one byte for the trailing NUL.
    let text = text.to_owned();
    let limit = limit.min(MAX_TEXT_BYTES);
    let buffer = unsafe { std::slice::from_raw_parts_mut(destination, limit + 1) };
    utf8::insert(buffer, cursor, limit, &text).unwrap_or(0) as u32
}

unsafe extern "C" fn previous_dispatch(source: *const u8, cursor: i32) -> i32 {
    let invocation = HOOK_STATE.enter();
    if invocation.state().is_none() {
        return 0;
    }
    unsafe { native_text(source) }.map_or(0, |text| previous_span(text, cursor))
}

unsafe extern "C" fn next_detour(source: *const u8, cursor: i32) -> i32 {
    let invocation = HOOK_STATE.enter();
    if invocation.state().is_none() {
        return -1;
    }
    unsafe { native_text(source) }.map_or(-1, |text| next_span(text, cursor))
}

/// The native callers already require a readable NUL-terminated C string.
/// The returned view is used only while the enclosing hook invocation is held.
unsafe fn native_text<'a>(source: *const u8) -> Option<&'a str> {
    if source.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(source.cast()) }.to_str().ok()
}

fn initial_text(source: &str, limit: usize) -> String {
    // The original initialization applies the filter's flag 1: control codes
    // 1..31, DEL and U+0080 become spaces. Other field-specific rules stay with
    // their native validators.
    let normalized = source
        .chars()
        .map(|character| match character {
            '\u{1}'..='\u{1f}' | '\u{7f}' | '\u{80}' => ' ',
            _ => character,
        })
        .collect::<String>();
    utf8::truncate(&normalized, limit.min(MAX_TEXT_BYTES)).to_owned()
}

unsafe fn set_initial_text(editor: usize, text: &str) {
    let buffer =
        unsafe { std::slice::from_raw_parts_mut((editor + TEXT_OFFSET) as *mut u8, TEXT_CAPACITY) };
    buffer.fill(0);
    buffer[..text.len()].copy_from_slice(text.as_bytes());
    unsafe { ptr::write_unaligned((editor + CURSOR_OFFSET) as *mut u16, text.len() as u16) };
}

fn previous_span(text: &str, cursor: i32) -> i32 {
    let Ok(cursor) = usize::try_from(cursor) else {
        return 0;
    };
    if !text.is_char_boundary(cursor) {
        return 0;
    }
    (cursor - utf8::prev_boundary(text, cursor)) as i32
}

fn next_span(text: &str, cursor: i32) -> i32 {
    let Ok(cursor) = usize::try_from(cursor) else {
        return -1;
    };
    if !text.is_char_boundary(cursor) {
        return -1;
    }
    if text.is_empty() {
        return 0;
    }
    if cursor == text.len() {
        // FDA0 returns one for the terminator of a nonempty native string.
        return 1;
    }
    (utf8::next_boundary(text, cursor) - cursor) as i32
}

#[unsafe(naked)]
unsafe extern "C" fn call_initialize(
    _target: usize,
    _limit: u32,
    _style: u32,
    _mode: u32,
    _source: *const u8,
) -> i32 {
    core::arch::naked_asm!(
        "mov edx, [esp + 4]",
        "mov eax, [esp + 8]",
        "mov ecx, [esp + 12]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "call edx",
        "add esp, 8",
        "ret",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_navigation_returns_utf8_byte_spans_and_boundary_sentinels() {
        let text = "Aé中😀";
        for (cursor, span) in [(0, 1), (1, 2), (3, 3), (6, 4)] {
            assert_eq!(next_span(text, cursor), span);
            assert_eq!(previous_span(text, cursor + span), span);
        }
        for cursor in [-1, 2, 4, 5, 7, 8, 9, 11] {
            assert_eq!(next_span(text, cursor), -1);
            assert_eq!(previous_span(text, cursor), 0);
        }
        assert_eq!(next_span(text, 10), 1);
        assert_eq!(previous_span(text, 0), 0);
        assert_eq!(next_span("", 0), 0);
    }

    #[test]
    fn initialization_filters_controls_and_keeps_cursor_within_the_byte_capacity() {
        let text = initial_text("A中😀\nB\u{80}C", 10);
        assert_eq!(text, "A中😀 B");
        assert_eq!(initial_text("\t\u{7f}\u{80}😀", 255), "   😀");
        let long = format!("{}😀", "A".repeat(253));
        let text = initial_text(&long, 256);
        assert_eq!(text.len(), 253);
        let mut editor = [0xCC; 334];
        unsafe { set_initial_text(editor.as_mut_ptr() as usize, &text) };
        assert_eq!(u16::from_ne_bytes([editor[16], editor[17]]), 253);
        assert_eq!(&editor[43..43 + 253], text.as_bytes());
        assert_eq!(&editor[43 + 253..43 + 256], &[0; 3]);
        assert_eq!(editor[42], 0xCC);
        assert_eq!(editor[299], 0xCC);
    }

    unsafe extern "C" fn record_initialization(
        limit: u32,
        style: u32,
        mode: u32,
        output: *const u8,
    ) -> i32 {
        let output = output.cast_mut().cast::<u32>();
        unsafe {
            output.write(limit);
            output.add(1).write(style);
            output.add(2).write(mode);
        }
        0x1234_5678
    }

    initialize_bridge!(checked_initialize_detour, record_initialization);

    #[test]
    fn initialization_bridge_preserves_ax_ecx_stack_and_eax_result() {
        let mut output = [0u32; 3];
        let result = unsafe {
            call_initialize(
                checked_initialize_detour as *const () as usize,
                0xABCD_00EF,
                0x1234_5678,
                0x9988_7766,
                output.as_mut_ptr().cast(),
            )
        };
        assert_eq!(output, [0xEF, 0x1234_5678, 0x9988_7766]);
        assert_eq!(result, 0x1234_5678);
    }

    unsafe extern "C" fn record_insertion(
        source: *const u8,
        output: *mut u32,
        cursor: u32,
        limit: u32,
    ) -> u32 {
        unsafe {
            output.write(u32::from(source.read()));
            output.add(1).write(cursor);
            output.add(2).write(limit);
        }
        0x1234_ABCD
    }

    insertion_bridge!(checked_insert_detour, record_insertion);

    #[unsafe(naked)]
    unsafe extern "C" fn call_insertion_checked(
        _target: usize,
        _source: *const u8,
        _output: *mut u32,
        _cursor: u32,
        _limit: u32,
    ) -> u64 {
        core::arch::naked_asm!(
            "push ebp",
            "mov ebp, esp",
            "push ebx",
            "push esi",
            "push edi",
            "mov ebx, 0x11223344",
            "mov esi, 0x55667788",
            "mov edi, 0x99AABBCC",
            "mov eax, [ebp + 8]",
            "mov ecx, [ebp + 12]",
            "push dword ptr [ebp + 24]",
            "push dword ptr [ebp + 20]",
            "push dword ptr [ebp + 16]",
            "call eax",
            "add esp, 12",
            "xor edx, edx",
            "cmp ebx, 0x11223344",
            "jne 2f",
            "cmp esi, 0x55667788",
            "jne 2f",
            "cmp edi, 0x99AABBCC",
            "jne 2f",
            "inc edx",
            "2:",
            "pop edi",
            "pop esi",
            "pop ebx",
            "pop ebp",
            "ret",
        );
    }

    #[test]
    fn insertion_bridge_preserves_custom_arguments_result_and_nonvolatile_registers() {
        let mut output = [0u32; 3];
        let result = unsafe {
            call_insertion_checked(
                checked_insert_detour as *const () as usize,
                c"x".as_ptr().cast(),
                output.as_mut_ptr(),
                23,
                201,
            )
        };
        assert_eq!(output, [u32::from(b'x'), 23, 201]);
        assert_eq!(result, 0x0000_0001_1234_ABCD);
    }

    unsafe extern "C" fn record_previous(output: *mut u32, cursor: i32) -> i32 {
        unsafe { output.write(cursor as u32) };
        0x1234_5678
    }

    previous_bridge!(checked_previous_detour, record_previous);

    #[unsafe(naked)]
    unsafe extern "C" fn call_previous(_target: usize, _output: *mut u32, _cursor: i32) -> i32 {
        core::arch::naked_asm!(
            "mov eax, [esp + 4]",
            "mov edx, [esp + 8]",
            "push dword ptr [esp + 12]",
            "call eax",
            "add esp, 4",
            "ret",
        );
    }

    #[test]
    fn previous_bridge_preserves_edx_stack_argument_and_eax_result() {
        let mut cursor = 0;
        let result = unsafe {
            call_previous(
                checked_previous_detour as *const () as usize,
                &mut cursor,
                -1234,
            )
        };
        assert_eq!(cursor, (-1234i32) as u32);
        assert_eq!(result, 0x1234_5678);
    }
}
