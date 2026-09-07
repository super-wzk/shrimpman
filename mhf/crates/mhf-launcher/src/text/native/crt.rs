use super::{CodeHook, HOOK_STATE, MAX_TEXT_BYTES, Registers, copy_z, read_z};
use crate::text::utf8::truncate;
use std::{cmp::Ordering, ptr, sync::atomic::AtomicUsize};

macro_rules! crt_hooks {
    ($(($original:ident, $detour:ident, $operation:literal, $name:literal, $rva:literal, $signature:expr)),+ $(,)?) => {
        $(
            static $original: AtomicUsize = AtomicUsize::new(0);
            #[unsafe(naked)]
            unsafe extern "C" fn $detour() {
                core::arch::naked_asm!(
                    "pushfd", "pushad", "mov eax, esp", "push {operation}", "push eax",
                    "call {dispatch}", "add esp, 8", "test eax, eax", "jz 2f",
                    "popad", "popfd", "ret",
                    "2:", "popad", "popfd", "jmp dword ptr [{original}]",
                    operation = const $operation, dispatch = sym dispatch, original = sym $original,
                );
            }
        )+
        pub(super) fn code_hooks() -> Vec<CodeHook> {
            vec![$(CodeHook { name: $name, rva: $rva, signature: $signature,
                detour: $detour as *const () as *mut _, original: &$original }),+]
        }
    }
}

const TWO_ARGS: &[(usize, u8)] = &[
    (0, 0x8B),
    (1, 0xFF),
    (2, 0x55),
    (3, 0x8B),
    (4, 0xEC),
    (5, 0x6A),
    (6, 0),
    (7, 0xFF),
    (8, 0x75),
    (9, 0x0C),
];
const THREE_ARGS: &[(usize, u8)] = &[
    (0, 0x8B),
    (1, 0xFF),
    (2, 0x55),
    (3, 0x8B),
    (4, 0xEC),
    (5, 0x6A),
    (6, 0),
    (7, 0xFF),
    (8, 0x75),
    (9, 0x10),
];
const FOUR_ARGS: &[(usize, u8)] = &[
    (0, 0x8B),
    (1, 0xFF),
    (2, 0x55),
    (3, 0x8B),
    (4, 0xEC),
    (5, 0x6A),
    (6, 0),
    (7, 0xFF),
    (8, 0x75),
    (9, 0x14),
];

crt_hooks!(
    (
        FIND_TARGET,
        find_detour,
        0,
        "UTF-8 game mbschr",
        0x015AE2D5,
        TWO_ARGS
    ),
    (
        UPPER_TARGET,
        upper_detour,
        1,
        "UTF-8 game mbsupr_s",
        0x015AE4A7,
        TWO_ARGS
    ),
    (
        COMPARE_TARGET,
        compare_detour,
        2,
        "UTF-8 game mbscmp",
        0x015AE5BD,
        TWO_ARGS
    ),
    (
        COPY_TARGET,
        copy_detour,
        3,
        "UTF-8 game mbsnbcpy",
        0x015AEBFB,
        THREE_ARGS
    ),
    (
        SUBSTRING_TARGET,
        substring_detour,
        4,
        "UTF-8 game mbsstr",
        0x015AED3F,
        TWO_ARGS
    ),
    (
        COMPARE_BYTES_TARGET,
        compare_bytes_detour,
        5,
        "UTF-8 game mbsnbcmp",
        0x015AEEA2,
        THREE_ARGS
    ),
    (
        COPY_SAFE_TARGET,
        copy_safe_detour,
        6,
        "UTF-8 game mbsnbcpy_s",
        0x015AEEEF,
        FOUR_ARGS
    ),
    (
        REJECT_SPAN_TARGET,
        reject_span_detour,
        7,
        "UTF-8 game mbscspn",
        0x015AEFC8,
        TWO_ARGS
    ),
    (
        APPEND_TARGET,
        append_detour,
        8,
        "UTF-8 game mbsnbcat",
        0x015AF5C8,
        THREE_ARGS
    ),
    (
        WIDE_TARGET,
        wide_detour,
        9,
        "UTF-8 game mbstowcs",
        0x015B118B,
        &[
            (0, 0x8B),
            (1, 0xFF),
            (2, 0x55),
            (3, 0x8B),
            (4, 0xEC),
            (5, 0x83),
            (6, 0x3D)
        ]
    ),
    (
        COMPARE_CASE_TARGET,
        compare_case_detour,
        10,
        "UTF-8 game mbsicmp",
        0x015B13AC,
        TWO_ARGS
    ),
    (
        ACCEPT_SPAN_TARGET,
        accept_span_detour,
        11,
        "UTF-8 game mbsspn",
        0x015B16C4,
        TWO_ARGS
    ),
);

fn order(left: &[u8], right: &[u8]) -> u32 {
    match left.cmp(right) {
        Ordering::Less => u32::MAX,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

fn prefix(source: &[u8], maximum: usize) -> &[u8] {
    let valid = match std::str::from_utf8(source) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&source[..error.valid_up_to()]).unwrap_or_default(),
    };
    truncate(valid, maximum).as_bytes()
}

/// Bounded CRT sources need not have a NUL within their byte count.
unsafe fn read_bounded<'a>(source: *const u8, maximum: usize) -> Option<&'a [u8]> {
    if maximum == 0 {
        return Some(&[]);
    }
    if source.is_null() {
        return None;
    }
    let maximum = maximum.min(MAX_TEXT_BYTES);
    let mut length = 0;
    while length < maximum && unsafe { ptr::read(source.add(length)) } != 0 {
        length += 1;
    }
    Some(unsafe { std::slice::from_raw_parts(source, length) })
}

unsafe fn copy_safe(destination: *mut u8, capacity: usize, source: *const u8, maximum: u32) -> u32 {
    if maximum == 0 && destination.is_null() && capacity == 0 {
        return 0;
    }
    if destination.is_null() || capacity == 0 || capacity > MAX_TEXT_BYTES {
        return 22;
    }
    let truncate_output = maximum == u32::MAX;
    let read_limit = if truncate_output {
        capacity
    } else {
        (maximum as usize).min(capacity)
    };
    let Some(source) = (unsafe { read_bounded(source, read_limit) }) else {
        unsafe { ptr::write(destination, 0) };
        return 22;
    };
    let count = if truncate_output {
        capacity - 1
    } else {
        maximum as usize
    };
    let copied = prefix(source, count);
    if copied.len() >= capacity
        || (!truncate_output && maximum as usize > capacity && source.len() == capacity)
    {
        unsafe { ptr::write(destination, 0) };
        return 34;
    }
    unsafe { copy_z(destination, capacity, copied) };
    if truncate_output && copied.len() < source.len() {
        80
    } else {
        0
    }
}

fn find(source: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    source
        .windows(needle.len())
        .position(|candidate| candidate == needle)
}

unsafe extern "C" fn dispatch(registers: *mut Registers, operation: u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let registers = unsafe { &mut *registers };
    let caller =
        unsafe { ptr::read_unaligned((registers.esp as usize + 4) as *const u32) } as usize;
    if !(state.module_base..state.module_base + 0x015AB000).contains(&caller) {
        return 0;
    }
    let result = unsafe {
        match operation {
            0 => {
                let source = registers.argument(0) as *const u8;
                let bytes = read_z(source, MAX_TEXT_BYTES).unwrap_or_default();
                let character = registers.argument(1);
                if character == 0 {
                    source.add(bytes.len()) as u32
                } else if let Some(character) = char::from_u32(character) {
                    let mut utf8 = [0; 4];
                    let needle = character.encode_utf8(&mut utf8);
                    find(bytes, needle.as_bytes()).map_or(0, |index| source.add(index) as u32)
                } else {
                    0
                }
            }
            1 => {
                let destination = registers.argument(0) as *mut u8;
                let capacity = registers.argument(1) as usize;
                if let Some(bytes) = read_z(destination, capacity) {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        let upper = text.to_uppercase();
                        if upper.len() < capacity {
                            copy_z(destination, capacity, upper.as_bytes());
                            0
                        } else {
                            copy_z(destination, capacity, &[]);
                            34
                        }
                    } else {
                        42
                    }
                } else {
                    if !destination.is_null() && capacity > 0 {
                        ptr::write(destination, 0)
                    };
                    22
                }
            }
            2 | 5 | 10 => {
                let read = |source| {
                    if operation == 5 {
                        read_bounded(source, registers.argument(2) as usize)
                    } else {
                        read_z(source, MAX_TEXT_BYTES)
                    }
                    .unwrap_or_default()
                };
                let left = read(registers.argument(0) as *const u8);
                let right = read(registers.argument(1) as *const u8);
                match operation {
                    5 => {
                        let count = registers.argument(2) as usize;
                        order(prefix(left, count), prefix(right, count))
                    }
                    10 => order(
                        String::from_utf8_lossy(left).to_lowercase().as_bytes(),
                        String::from_utf8_lossy(right).to_lowercase().as_bytes(),
                    ),
                    _ => order(left, right),
                }
            }
            3 => {
                let destination = registers.argument(0) as *mut u8;
                let count = (registers.argument(2) as usize).min(MAX_TEXT_BYTES);
                let source =
                    read_bounded(registers.argument(1) as *const u8, count).unwrap_or_default();
                if !destination.is_null() {
                    let prefix = prefix(source, count);
                    ptr::copy(prefix.as_ptr(), destination, prefix.len());
                    if prefix.len() < count {
                        ptr::write_bytes(destination.add(prefix.len()), 0, count - prefix.len())
                    }
                }
                destination as u32
            }
            4 => {
                let source = registers.argument(0) as *const u8;
                let bytes = read_z(source, MAX_TEXT_BYTES).unwrap_or_default();
                let needle =
                    read_z(registers.argument(1) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                find(bytes, needle).map_or(0, |index| source.add(index) as u32)
            }
            6 => copy_safe(
                registers.argument(0) as *mut u8,
                registers.argument(1) as usize,
                registers.argument(2) as *const u8,
                registers.argument(3),
            ),
            7 | 11 => {
                let source =
                    read_z(registers.argument(0) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let control =
                    read_z(registers.argument(1) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let source = String::from_utf8_lossy(source);
                let control = String::from_utf8_lossy(control);
                source
                    .char_indices()
                    .find(|(_, character)| control.contains(*character) == (operation == 7))
                    .map_or(source.len(), |(offset, _)| offset) as u32
            }
            8 => {
                let destination = registers.argument(0) as *mut u8;
                let count = registers.argument(2) as usize;
                if count == 0 {
                    registers.eax = destination as u32;
                    return 1;
                }
                let existing = read_z(destination, MAX_TEXT_BYTES)
                    .unwrap_or_default()
                    .len();
                let source =
                    read_bounded(registers.argument(1) as *const u8, count).unwrap_or_default();
                let prefix = prefix(source, count);
                if !destination.is_null() {
                    copy_z(destination.add(existing), prefix.len() + 1, prefix)
                }
                destination as u32
            }
            9 => {
                let destination = registers.argument(0) as *mut u16;
                let source =
                    read_z(registers.argument(1) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let maximum = registers.argument(2) as usize;
                match std::str::from_utf8(source) {
                    Err(_) => u32::MAX,
                    Ok(text) if destination.is_null() => text.encode_utf16().count() as u32,
                    Ok(text) => {
                        let mut count = 0;
                        for character in text.chars() {
                            let mut wide = [0; 2];
                            let wide = character.encode_utf16(&mut wide);
                            if count + wide.len() > maximum {
                                break;
                            }
                            ptr::copy(wide.as_ptr(), destination.add(count), wide.len());
                            count += wide.len();
                        }
                        if count < maximum {
                            ptr::write(destination.add(count), 0)
                        }
                        count as u32
                    }
                }
            }
            _ => return 0,
        }
    };
    registers.eax = result;
    1
}

#[cfg(test)]
mod tests {
    use super::{copy_safe, find, order, prefix, read_bounded};

    #[test]
    fn comparisons_and_byte_limits_keep_complete_utf8() {
        assert_eq!(prefix("A中😀".as_bytes(), 3), b"A");
        assert_eq!(prefix("A中😀".as_bytes(), 7), "A中".as_bytes());
        assert_eq!(find("甲\n😀".as_bytes(), b"\n"), Some(3));
        assert_eq!(find("甲😀乙".as_bytes(), "😀".as_bytes()), Some(3));
        assert_eq!(order("甲".as_bytes(), "甲".as_bytes()), 0);
    }

    #[test]
    fn bounded_crt_reads_stop_at_count_and_secure_copy_stops_at_capacity() {
        use std::ptr;
        use windows::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS,
            PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
        };
        let page = unsafe { VirtualAlloc(None, 8192, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        assert!(!page.is_null());
        struct Pages(*mut std::ffi::c_void);
        impl Drop for Pages {
            fn drop(&mut self) {
                unsafe { VirtualFree(self.0, 0, MEM_RELEASE) }.unwrap();
            }
        }
        let _pages = Pages(page);
        let guard = unsafe { page.cast::<u8>().add(4096) };
        let mut old = PAGE_PROTECTION_FLAGS::default();
        unsafe { VirtualProtect(guard.cast(), 4096, PAGE_NOACCESS, &mut old) }.unwrap();
        let source = unsafe { guard.sub(4) };
        unsafe { ptr::copy_nonoverlapping("A中".as_ptr(), source, 4) };
        assert_eq!(unsafe { read_bounded(source, 4) }, Some("A中".as_bytes()));
        assert_eq!(unsafe { read_bounded(guard, 0) }, Some(&b""[..]));
        assert_eq!(prefix(unsafe { read_bounded(source, 3) }.unwrap(), 3), b"A");
        let mut destination = [0xFF; 8];
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 8, source, 4) },
            0
        );
        assert_eq!(&destination[..5], "A中\0".as_bytes());
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 4, source, 4) },
            34
        );
        assert_eq!(destination[0], 0);
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 4, source, 100) },
            34
        );
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 2, c"中".as_ptr().cast(), 2) },
            0
        );
        assert_eq!(destination[0], 0);
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 2, c"中".as_ptr().cast(), 3) },
            34
        );
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 4, source, u32::MAX) },
            80
        );
        assert_eq!(&destination[..2], b"A\0");
        assert_eq!(
            unsafe { copy_safe(destination.as_mut_ptr(), 8, guard, 0) },
            0
        );
        assert_eq!(destination[0], 0);
        assert_eq!(unsafe { copy_safe(ptr::null_mut(), 0, guard, 0) }, 0);
        assert_eq!(
            unsafe {
                copy_safe(
                    destination.as_mut_ptr(),
                    4,
                    c"abc".as_ptr().cast(),
                    u32::MAX,
                )
            },
            0
        );
        assert_eq!(&destination[..4], b"abc\0");
    }
}
