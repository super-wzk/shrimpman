use super::{CodeHook, HOOK_STATE, MAX_TEXT_BYTES, Registers, copy_z, read_z, strings};
use crate::text::utf8::display_columns;
use std::{
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

// These two business fields require ASCII. The old test checked a Shift-JIS
// lead-byte bitmap; its UTF-8 replacement rejects every non-ASCII byte without
// changing the shared bitmap or any other caller's character policy.
static NON_ASCII: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255,
];

macro_rules! inline_hook {
    ($original:ident, $detour:ident, $operation:literal) => {
        inline_hook!($original, $detour, $operation, dispatch);
    };
    ($original:ident, $detour:ident, $operation:literal, $dispatch:path) => {
        static $original: AtomicUsize = AtomicUsize::new(0);
        #[unsafe(naked)]
        unsafe extern "C" fn $detour() {
            core::arch::naked_asm!(
                // These hooks interrupt basic blocks, where XMM/x87 values can
                // still be live. Rust calls may freely overwrite those values.
                "pushfd", "pushad", "mov eax, esp",
                "sub esp, 528", "and esp, -16", "fxsave [esp]", "mov [esp + 512], eax",
                "push {operation}", "push eax", "call {dispatch}", "add esp, 8",
                "fxrstor [esp]", "mov esp, [esp + 512]", "test eax, eax", "jz 2f",
                "popad", "popfd", "jmp dword ptr [esp - 24]",
                "2:", "popad", "popfd", "jmp dword ptr [{original}]",
                operation = const $operation, dispatch = sym $dispatch, original = sym $original,
            );
        }
    }
}

inline_hook!(SHORT_CODE_TARGET, short_code_detour, 0);
inline_hook!(INVITE_CODE_TARGET, invite_code_detour, 1);
inline_hook!(DESCRIPTION_TARGET, description_detour, 2);
inline_hook!(NAME_PADDING_TARGET, name_padding_detour, 3);
inline_hook!(MASK_WIDTH_TARGET, mask_width_detour, 4);
inline_hook!(TITLE_CENTER_TARGET, title_center_detour, 5);
inline_hook!(SCREENSHOT_ENCODING_TARGET, screenshot_encoding_detour, 6);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "UTF-8 screenshot Win32 conversion",
            rva: 0x014D9280,
            signature: &[(0, 0xFF), (1, 0x15)],
            detour: screenshot_encoding_detour as *const () as *mut _,
            original: &SCREENSHOT_ENCODING_TARGET,
        },
        CodeHook {
            name: "UTF-8 title-menu centering",
            rva: 0x0083CA72,
            signature: &[
                (0, 0x8B),
                (1, 0xC6),
                (2, 0x8D),
                (3, 0x48),
                (4, 0x01),
                (5, 0x8A),
                (6, 0x10),
                (7, 0x40),
                (8, 0x84),
                (9, 0xD2),
                (10, 0x75),
                (11, 0xF9),
            ],
            detour: title_center_detour as *const () as *mut _,
            original: &TITLE_CENTER_TARGET,
        },
        CodeHook {
            name: "UTF-8 ASCII short-code restriction",
            rva: 0x003DFCF2,
            signature: &[
                (0, 0x8B),
                (1, 0x14),
                (2, 0x95),
                (7, 0x0F),
                (8, 0xB6),
                (9, 0xC8),
            ],
            detour: short_code_detour as *const () as *mut _,
            original: &SHORT_CODE_TARGET,
        },
        CodeHook {
            name: "UTF-8 ASCII invite-code restriction",
            rva: 0x00695535,
            signature: &[
                (0, 0x8B),
                (1, 0x14),
                (2, 0x8D),
                (7, 0xB8),
                (8, 0x88),
                (9, 0xFB),
                (10, 0xFD),
                (11, 0xFF),
            ],
            detour: invite_code_detour as *const () as *mut _,
            original: &INVITE_CODE_TARGET,
        },
        CodeHook {
            name: "UTF-8 bounded description line",
            rva: 0x015A9BB0,
            signature: &[
                (0, 0x8A),
                (1, 0x17),
                (2, 0x84),
                (3, 0xD2),
                (4, 0x74),
                (5, 0x74),
                (6, 0x8B),
                (7, 0x35),
            ],
            detour: description_detour as *const () as *mut _,
            original: &DESCRIPTION_TARGET,
        },
        CodeHook {
            name: "UTF-8 party-name padding",
            rva: 0x0088603D,
            signature: &[
                (0, 0x8D),
                (1, 0x50),
                (2, 1),
                (3, 0x8A),
                (4, 8),
                (5, 0x40),
                (6, 0x84),
                (7, 0xC9),
                (8, 0x75),
                (9, 0xF9),
            ],
            detour: name_padding_detour as *const () as *mut _,
            original: &NAME_PADDING_TARGET,
        },
        CodeHook {
            name: "UTF-8 hidden-name display width",
            rva: 0x007ACBB3,
            signature: &[
                (0, 0x8D),
                (1, 0x50),
                (2, 1),
                (3, 0x8A),
                (4, 8),
                (5, 0x40),
                (6, 0x84),
                (7, 0xC9),
                (8, 0x75),
                (9, 0xF9),
            ],
            detour: mask_width_detour as *const () as *mut _,
            original: &MASK_WIDTH_TARGET,
        },
    ]
}

unsafe extern "C" fn dispatch(registers: *mut Registers, operation: u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let registers = unsafe { &mut *registers };
    let continuation = match operation {
        0 | 1 => {
            registers.edx = NON_ASCII.as_ptr() as u32;
            if operation == 0 {
                0x003DFCF9
            } else {
                0x0069553C
            }
        }
        2 => {
            // At 115A9BB0: EDI=source, [ESP+40h]=char buffer[128],
            // [ESP+34h]=color, [ESP+20h]=saved source, [ESP+2Ch]=newline.
            // Replacing the complete copy loop also removes its unchecked
            // single/double-byte writes into that fixed local buffer.
            let stack = registers.esp as usize + 4;
            let source_pointer = registers.edi as *const u8;
            let source = unsafe { read_z(source_pointer, MAX_TEXT_BYTES) }.unwrap_or_default();
            let length = source
                .iter()
                .position(|&byte| byte == b'\n')
                .map_or(source.len(), |index| index + 1);
            let line = String::from_utf8_lossy(&source[..length]);
            let (prefix, suffix) = strings::shortened(&line, 128, "…");
            let line = [prefix.as_bytes(), suffix.as_bytes()].concat();
            unsafe { copy_z((stack + 0x40) as *mut u8, 128, &line) };
            registers.eax = line.len() as u32;
            registers.edi = unsafe { source_pointer.add(length) } as u32;
            registers.esi = unsafe { ptr::read_unaligned((stack + 0x34) as *const u32) };
            unsafe {
                ptr::write_unaligned((stack + 0x20) as *mut u32, registers.edi);
                ptr::write_unaligned((stack + 0x2C) as *mut u32, 1);
            }
            0x015A9C31
        }
        3 => {
            let destination = registers.edi as *mut u8;
            let capacity = registers.ebx as usize;
            if let Some(source) = unsafe { read_z(destination, capacity) } {
                let columns = display_columns(&String::from_utf8_lossy(source));
                let length = source.len();
                let padding = 12usize
                    .saturating_sub(columns)
                    .min(capacity.saturating_sub(length + 1));
                unsafe {
                    ptr::write_bytes(destination.add(length), b' ', padding);
                    ptr::write(destination.add(length + padding), 0);
                }
                registers.eax = 0;
            } else {
                registers.eax = 22
            }
            // The original strcat argument pushes and pops are both skipped.
            0x0088605A
        }
        4 => {
            let source_pointer = registers.eax as *const u8;
            let source = unsafe { read_z(source_pointer, MAX_TEXT_BYTES) }.unwrap_or_default();
            registers.eax = display_columns(&String::from_utf8_lossy(source)) as u32;
            registers.edx = unsafe { source_pointer.add(1) } as u32;
            registers.ecx &= !0xFF;
            0x007ACBBF
        }
        5 => {
            // ESI points at title_menu[index]. The skipped strlen loop feeds
            // EAX-ECX into font_width/4, i.e. half the label's screen width.
            // Preserve that ABI while replacing bytes with display columns.
            let source =
                unsafe { read_z(registers.esi as *const u8, MAX_TEXT_BYTES) }.unwrap_or_default();
            title_menu_columns(registers, &String::from_utf8_lossy(source));
            0x0083CA7E
        }
        6 => {
            // Arguments are already pushed for MultiByteToWideChar. Its stdcall
            // trampoline performs the original call and stack cleanup.
            unsafe { ptr::write_unaligned((registers.esp as usize + 4) as *mut u32, 65001) };
            registers.esp = SCREENSHOT_ENCODING_TARGET.load(Ordering::Acquire) as u32;
            return 1;
        }
        _ => return 0,
    };
    // POPAD ignores its saved ESP slot. Use that slot for the exact continuation
    // address and jump through it after restoring all registers and flags.
    registers.esp = (state.module_base + continuation) as u32;
    1
}

fn title_menu_columns(registers: &mut Registers, text: &str) {
    registers.eax = display_columns(text) as u32;
    registers.ecx = 0;
}

#[cfg(test)]
mod tests {
    use super::{Registers, title_menu_columns};
    use std::sync::atomic::AtomicUsize;

    inline_hook!(TEST_ORIGINAL, test_detour, 0, clobber_dispatch);

    #[unsafe(naked)]
    unsafe extern "C" fn continuation() {
        core::arch::naked_asm!("ret");
    }

    #[unsafe(naked)]
    unsafe extern "C" fn clobber_dispatch() {
        core::arch::naked_asm!(
            "mov ecx, [esp + 4]", "lea edx, [{continuation}]", "mov [ecx + 12], edx",
            "fninit", "pxor xmm0, xmm0", "pxor xmm1, xmm1", "pxor xmm2, xmm2",
            "pxor xmm3, xmm3", "pxor xmm4, xmm4", "pxor xmm5, xmm5",
            "pxor xmm6, xmm6", "pxor xmm7, xmm7", "mov eax, 1", "ret",
            continuation = sym continuation,
        );
    }

    #[unsafe(naked)]
    unsafe extern "C" fn exercise_fp_state(expected: *mut u8, actual: *mut u8, saved: *mut u8) {
        core::arch::naked_asm!(
            "push ebx", "push esi", "push edi",
            "mov ebx, [esp + 16]", "mov esi, [esp + 20]", "mov edi, [esp + 24]",
            "fxsave [edi]", "fninit", "fld1", "fldz",
            "pcmpeqd xmm0, xmm0", "movaps xmm1, xmm0", "movaps xmm2, xmm0",
            "movaps xmm3, xmm0", "movaps xmm4, xmm0", "movaps xmm5, xmm0",
            "movaps xmm6, xmm0", "movaps xmm7, xmm0", "fxsave [ebx]",
            "call {detour}", "fxsave [esi]", "fxrstor [edi]",
            "pop edi", "pop esi", "pop ebx", "ret",
            detour = sym test_detour,
        );
    }

    #[test]
    fn inline_hooks_restore_live_x87_and_simd_registers() {
        #[repr(align(16))]
        struct FpState([u8; 512]);
        let (mut expected, mut actual, mut saved) =
            (FpState([0; 512]), FpState([0; 512]), FpState([0; 512]));
        unsafe {
            exercise_fp_state(
                expected.0.as_mut_ptr(),
                actual.0.as_mut_ptr(),
                saved.0.as_mut_ptr(),
            )
        };
        assert_eq!(expected.0, actual.0);
    }

    #[test]
    fn title_menu_centers_utf8_labels_without_dropping_leading_spaces() {
        for (text, columns) in [
            ("クイックスタート", 16),
            ("ゲームスタート", 14),
            ("选项", 4),
            ("  结束", 6),
            ("　结束", 6),
        ] {
            let mut registers: Registers = unsafe { std::mem::zeroed() };
            registers.esi = 0x1234;
            registers.eax = text.len() as u32;
            registers.ecx = 0x9876;
            title_menu_columns(&mut registers, text);
            assert_eq!(registers.eax.wrapping_sub(registers.ecx), columns);
            assert_eq!(registers.esi, 0x1234);
            let left = 640 / 2 - (registers.eax - registers.ecx) * (24 / 4);
            assert_eq!(left + columns * (24 / 2) / 2, 320);
        }
    }
}
