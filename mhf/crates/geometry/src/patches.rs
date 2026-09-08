//! Verified instruction edits for the ZZ HD model renderers.
//! Length loads become DWORD loads; each descriptor cell grows from 2 to 4 bytes.
//! Material and variant values retain their native WORD parameter ABI.
//! All edits preserve instruction-span sizes and existing branch destinations.

pub(crate) struct Patch {
    pub rva: usize,
    pub original: &'static [u8],
    pub replacement: &'static [u8],
}

pub(crate) const PATCHES: &[Patch] = &[
    // 10018F3A: batch length DWORD; movzx   edx, word ptr [ebx]
    Patch {
        rva: 0x00018f3a,
        original: &[0x0f, 0xb7, 0x13],
        replacement: &[0x8b, 0x13, 0x90],
    },
    // 10018F62: batch length DWORD; movzx   ecx, word ptr [ebx]
    Patch {
        rva: 0x00018f62,
        original: &[0x0f, 0xb7, 0x0b],
        replacement: &[0x8b, 0x0b, 0x90],
    },
    // 10018F6F: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x00018f6f,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 10018F74: batch cursor stride; add     ebx, 2
    Patch {
        rva: 0x00018f74,
        original: &[0x83, 0xc3, 0x02],
        replacement: &[0x83, 0xc3, 0x04],
    },
    // 10019076: material or variant offset; mov     dx, [ebx+2]
    Patch {
        rva: 0x00019076,
        original: &[0x66, 0x8b, 0x53, 0x02],
        replacement: &[0x66, 0x8b, 0x53, 0x04],
    },
    // 10019086: material or variant offset; mov     ax, [ebx+4]
    Patch {
        rva: 0x00019086,
        original: &[0x66, 0x8b, 0x43, 0x04],
        replacement: &[0x66, 0x8b, 0x43, 0x08],
    },
    // 1001908F: batch length DWORD; movzx   edx, word ptr [ebx]
    Patch {
        rva: 0x0001908f,
        original: &[0x0f, 0xb7, 0x13],
        replacement: &[0x8b, 0x13, 0x90],
    },
    // 100190BA: batch length DWORD; movzx   ecx, word ptr [ebx]
    Patch {
        rva: 0x000190ba,
        original: &[0x0f, 0xb7, 0x0b],
        replacement: &[0x8b, 0x0b, 0x90],
    },
    // 100190CB: batch cursor stride; add     ebx, 6
    Patch {
        rva: 0x000190cb,
        original: &[0x83, 0xc3, 0x06],
        replacement: &[0x83, 0xc3, 0x0c],
    },
    // 100191DF: material or variant offset; mov     ax, [ebx+2]
    Patch {
        rva: 0x000191df,
        original: &[0x66, 0x8b, 0x43, 0x02],
        replacement: &[0x66, 0x8b, 0x43, 0x04],
    },
    // 100191E9: batch length DWORD; movzx   ecx, word ptr [ebx]
    Patch {
        rva: 0x000191e9,
        original: &[0x0f, 0xb7, 0x0b],
        replacement: &[0x8b, 0x0b, 0x90],
    },
    // 10019214: batch length DWORD; movzx   eax, word ptr [ebx]
    Patch {
        rva: 0x00019214,
        original: &[0x0f, 0xb7, 0x03],
        replacement: &[0x8b, 0x03, 0x90],
    },
    // 10019225: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x00019225,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 10019339: material or variant offset; mov     dx, [ebx+2]
    Patch {
        rva: 0x00019339,
        original: &[0x66, 0x8b, 0x53, 0x02],
        replacement: &[0x66, 0x8b, 0x53, 0x04],
    },
    // 1001938C: batch length DWORD; movzx   edx, word ptr [ebx]
    Patch {
        rva: 0x0001938c,
        original: &[0x0f, 0xb7, 0x13],
        replacement: &[0x8b, 0x13, 0x90],
    },
    // 100193B4: batch length DWORD; movzx   ecx, word ptr [ebx]
    Patch {
        rva: 0x000193b4,
        original: &[0x0f, 0xb7, 0x0b],
        replacement: &[0x8b, 0x0b, 0x90],
    },
    // 100193C1: batch cursor stride; add     ebx, 6
    Patch {
        rva: 0x000193c1,
        original: &[0x83, 0xc3, 0x06],
        replacement: &[0x83, 0xc3, 0x0c],
    },
    // 100193C6: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x000193c6,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 1001A5B8: batch length DWORD; movzx   edx, word ptr [edx]
    Patch {
        rva: 0x0001a5b8,
        original: &[0x0f, 0xb7, 0x12],
        replacement: &[0x8b, 0x12, 0x90],
    },
    // 1001A66C: batch length DWORD; movzx   edx, word ptr [ecx]
    Patch {
        rva: 0x0001a66c,
        original: &[0x0f, 0xb7, 0x11],
        replacement: &[0x8b, 0x11, 0x90],
    },
    // 1001A679: batch cursor stride; add     [esp+150h+var_13C], 4
    Patch {
        rva: 0x0001a679,
        original: &[0x83, 0x44, 0x24, 0x14, 0x04],
        replacement: &[0x83, 0x44, 0x24, 0x14, 0x08],
    },
    // 1001A680: batch cursor stride; add     [esp+150h+var_13C], 2
    Patch {
        rva: 0x0001a680,
        original: &[0x83, 0x44, 0x24, 0x14, 0x02],
        replacement: &[0x83, 0x44, 0x24, 0x14, 0x04],
    },
    // 1001A75B: batch cursor stride; add     ecx, 4
    Patch {
        rva: 0x0001a75b,
        original: &[0x83, 0xc1, 0x04],
        replacement: &[0x83, 0xc1, 0x08],
    },
    // 1001A7EF: material or variant offset; mov     dx, [edi-2]
    Patch {
        rva: 0x0001a7ef,
        original: &[0x66, 0x8b, 0x57, 0xfe],
        replacement: &[0x66, 0x8b, 0x57, 0xfc],
    },
    // 1001A987: batch length DWORD; movzx   ecx, word ptr [ecx-4]
    Patch {
        rva: 0x0001a987,
        original: &[0x0f, 0xb7, 0x49, 0xfc],
        replacement: &[0x8b, 0x49, 0xf8, 0x90],
    },
    // 1001AA38: batch length DWORD; movzx   ecx, word ptr [eax-4]
    Patch {
        rva: 0x0001aa38,
        original: &[0x0f, 0xb7, 0x48, 0xfc],
        replacement: &[0x8b, 0x48, 0xf8, 0x90],
    },
    // 1001AA40: batch cursor stride; add     eax, 6
    Patch {
        rva: 0x0001aa40,
        original: &[0x83, 0xc0, 0x06],
        replacement: &[0x83, 0xc0, 0x0c],
    },
    // 1001ABD9: material or variant offset; mov     ax, [esi+2]
    Patch {
        rva: 0x0001abd9,
        original: &[0x66, 0x8b, 0x46, 0x02],
        replacement: &[0x66, 0x8b, 0x46, 0x04],
    },
    // 1001ABE7: variant comparison offset; cmp     word ptr [esi+2], 0
    Patch {
        rva: 0x0001abe7,
        original: &[0x66, 0x83, 0x7e, 0x02, 0x00],
        replacement: &[0x66, 0x83, 0x7e, 0x04, 0x00],
    },
    // 1001AD55: batch length DWORD; movzx   ecx, word ptr [ecx]
    Patch {
        rva: 0x0001ad55,
        original: &[0x0f, 0xb7, 0x09],
        replacement: &[0x8b, 0x09, 0x90],
    },
    // 1001AE05: batch length DWORD; movzx   ecx, word ptr [eax]
    Patch {
        rva: 0x0001ae05,
        original: &[0x0f, 0xb7, 0x08],
        replacement: &[0x8b, 0x08, 0x90],
    },
    // 1001AE0C: batch cursor stride; add     eax, 4
    Patch {
        rva: 0x0001ae0c,
        original: &[0x83, 0xc0, 0x04],
        replacement: &[0x83, 0xc0, 0x08],
    },
    // 1001AF3C: material or variant offset; mov     dx, [eax+2]
    Patch {
        rva: 0x0001af3c,
        original: &[0x66, 0x8b, 0x50, 0x02],
        replacement: &[0x66, 0x8b, 0x50, 0x04],
    },
    // 1001B0CE: batch length DWORD; movzx   ecx, word ptr [ecx]
    Patch {
        rva: 0x0001b0ce,
        original: &[0x0f, 0xb7, 0x09],
        replacement: &[0x8b, 0x09, 0x90],
    },
    // 1001B17E: batch length DWORD; movzx   edx, word ptr [ecx]
    Patch {
        rva: 0x0001b17e,
        original: &[0x0f, 0xb7, 0x11],
        replacement: &[0x8b, 0x11, 0x90],
    },
    // 1001B18F: batch cursor stride; add     [esp+150h+var_13C], 6
    Patch {
        rva: 0x0001b18f,
        original: &[0x83, 0x44, 0x24, 0x14, 0x06],
        replacement: &[0x83, 0x44, 0x24, 0x14, 0x0c],
    },
    // 1001B196: batch cursor stride; add     [esp+150h+var_13C], 4
    Patch {
        rva: 0x0001b196,
        original: &[0x83, 0x44, 0x24, 0x14, 0x04],
        replacement: &[0x83, 0x44, 0x24, 0x14, 0x08],
    },
    // 1001B437: batch length DWORD; movzx   edx, word ptr [ebx]
    Patch {
        rva: 0x0001b437,
        original: &[0x0f, 0xb7, 0x13],
        replacement: &[0x8b, 0x13, 0x90],
    },
    // 1001B487: batch length DWORD; movzx   ecx, word ptr [ebx]
    Patch {
        rva: 0x0001b487,
        original: &[0x0f, 0xb7, 0x0b],
        replacement: &[0x8b, 0x0b, 0x90],
    },
    // 1001B494: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x0001b494,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 1001B499: batch cursor stride; add     ebx, 2
    Patch {
        rva: 0x0001b499,
        original: &[0x83, 0xc3, 0x02],
        replacement: &[0x83, 0xc3, 0x04],
    },
    // 1001B576: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x0001b576,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 1001B5FA: material or variant offset; mov     dx, [ebx-2]
    Patch {
        rva: 0x0001b5fa,
        original: &[0x66, 0x8b, 0x53, 0xfe],
        replacement: &[0x66, 0x8b, 0x53, 0xfc],
    },
    // 1001B698: batch length DWORD; movzx   edx, word ptr [ebx-4]
    Patch {
        rva: 0x0001b698,
        original: &[0x0f, 0xb7, 0x53, 0xfc],
        replacement: &[0x8b, 0x53, 0xf8, 0x90],
    },
    // 1001B708: batch length DWORD; movzx   edx, word ptr [ebx-4]
    Patch {
        rva: 0x0001b708,
        original: &[0x0f, 0xb7, 0x53, 0xfc],
        replacement: &[0x8b, 0x53, 0xf8, 0x90],
    },
    // 1001B71A: batch cursor stride; add     ebx, 6
    Patch {
        rva: 0x0001b71a,
        original: &[0x83, 0xc3, 0x06],
        replacement: &[0x83, 0xc3, 0x0c],
    },
    // 1001B814: batch cursor stride; add     ebx, 2
    Patch {
        rva: 0x0001b814,
        original: &[0x83, 0xc3, 0x02],
        replacement: &[0x83, 0xc3, 0x04],
    },
    // 1001B95F: batch length DWORD; movzx   edx, word ptr [ebx-2]
    Patch {
        rva: 0x0001b95f,
        original: &[0x0f, 0xb7, 0x53, 0xfe],
        replacement: &[0x8b, 0x53, 0xfc, 0x90],
    },
    // 1001B9BC: material or variant offset; mov     ax, [ebx+2]
    Patch {
        rva: 0x0001b9bc,
        original: &[0x66, 0x8b, 0x43, 0x02],
        replacement: &[0x66, 0x8b, 0x43, 0x04],
    },
    // 1001B9CF: batch length DWORD; movzx   edx, word ptr [ebx-2]
    Patch {
        rva: 0x0001b9cf,
        original: &[0x0f, 0xb7, 0x53, 0xfe],
        replacement: &[0x8b, 0x53, 0xfc, 0x90],
    },
    // 1001B9E1: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x0001b9e1,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 1001BAF3: material or variant offset; mov     dx, [ebx+2]
    Patch {
        rva: 0x0001baf3,
        original: &[0x66, 0x8b, 0x53, 0x02],
        replacement: &[0x66, 0x8b, 0x53, 0x04],
    },
    // 1001BB77: batch length DWORD; movzx   edx, word ptr [ebx]
    Patch {
        rva: 0x0001bb77,
        original: &[0x0f, 0xb7, 0x13],
        replacement: &[0x8b, 0x13, 0x90],
    },
    // 1001BBF4: batch length DWORD; movzx   ecx, word ptr [ebx]
    Patch {
        rva: 0x0001bbf4,
        original: &[0x0f, 0xb7, 0x0b],
        replacement: &[0x8b, 0x0b, 0x90],
    },
    // 1001BC01: batch cursor stride; add     ebx, 6
    Patch {
        rva: 0x0001bc01,
        original: &[0x83, 0xc3, 0x06],
        replacement: &[0x83, 0xc3, 0x0c],
    },
    // 1001BC06: batch cursor stride; add     ebx, 4
    Patch {
        rva: 0x0001bc06,
        original: &[0x83, 0xc3, 0x04],
        replacement: &[0x83, 0xc3, 0x08],
    },
    // 1001BF08: batch length DWORD; movzx   edx, word ptr [edx]
    Patch {
        rva: 0x0001bf08,
        original: &[0x0f, 0xb7, 0x12],
        replacement: &[0x8b, 0x12, 0x90],
    },
    // 1001C00C: batch length DWORD; movzx   edx, word ptr [esi]
    Patch {
        rva: 0x0001c00c,
        original: &[0x0f, 0xb7, 0x16],
        replacement: &[0x8b, 0x16, 0x90],
    },
    // 1001C039: batch length DWORD; movzx   edx, word ptr [esi]
    Patch {
        rva: 0x0001c039,
        original: &[0x0f, 0xb7, 0x16],
        replacement: &[0x8b, 0x16, 0x90],
    },
    // 1001C10D: batch length DWORD; movzx   ecx, word ptr [ecx]
    Patch {
        rva: 0x0001c10d,
        original: &[0x0f, 0xb7, 0x09],
        replacement: &[0x8b, 0x09, 0x90],
    },
    // 1001C161: batch length DWORD; movzx   ecx, word ptr [edi]
    Patch {
        rva: 0x0001c161,
        original: &[0x0f, 0xb7, 0x0f],
        replacement: &[0x8b, 0x0f, 0x90],
    },
    // 1001C16E: batch cursor stride; add     edi, 4
    Patch {
        rva: 0x0001c16e,
        original: &[0x83, 0xc7, 0x04],
        replacement: &[0x83, 0xc7, 0x08],
    },
    // 1001C173: batch cursor stride; add     edi, 2
    Patch {
        rva: 0x0001c173,
        original: &[0x83, 0xc7, 0x02],
        replacement: &[0x83, 0xc7, 0x04],
    },
    // 1001C2D4: material or variant offset; mov     dx, [esi+2]
    Patch {
        rva: 0x0001c2d4,
        original: &[0x66, 0x8b, 0x56, 0x02],
        replacement: &[0x66, 0x8b, 0x56, 0x04],
    },
    // 1001C2E6: material or variant offset; mov     ax, [esi+4]
    Patch {
        rva: 0x0001c2e6,
        original: &[0x66, 0x8b, 0x46, 0x04],
        replacement: &[0x66, 0x8b, 0x46, 0x08],
    },
    // 1001C324: variant comparison offset; cmp     word ptr [esi+4], 0
    Patch {
        rva: 0x0001c324,
        original: &[0x66, 0x83, 0x7e, 0x04, 0x00],
        replacement: &[0x66, 0x83, 0x7e, 0x08, 0x00],
    },
    // 1001C43F: batch length DWORD; movzx   ecx, word ptr [ecx]
    Patch {
        rva: 0x0001c43f,
        original: &[0x0f, 0xb7, 0x09],
        replacement: &[0x8b, 0x09, 0x90],
    },
    // 1001C4BF: variant comparison offset; cmp     word ptr [esi+4], 0
    Patch {
        rva: 0x0001c4bf,
        original: &[0x66, 0x83, 0x7e, 0x04, 0x00],
        replacement: &[0x66, 0x83, 0x7e, 0x08, 0x00],
    },
    // 1001C5BE: batch length DWORD; movzx   ecx, word ptr [esi]
    Patch {
        rva: 0x0001c5be,
        original: &[0x0f, 0xb7, 0x0e],
        replacement: &[0x8b, 0x0e, 0x90],
    },
    // 1001C5EB: batch length DWORD; movzx   edx, word ptr [esi]
    Patch {
        rva: 0x0001c5eb,
        original: &[0x0f, 0xb7, 0x16],
        replacement: &[0x8b, 0x16, 0x90],
    },
    // 1001C663: variant comparison offset; cmp     word ptr [esi+4], 0
    Patch {
        rva: 0x0001c663,
        original: &[0x66, 0x83, 0x7e, 0x04, 0x00],
        replacement: &[0x66, 0x83, 0x7e, 0x08, 0x00],
    },
    // 1001C94E: batch length DWORD; movzx   edx, word ptr [edx]
    Patch {
        rva: 0x0001c94e,
        original: &[0x0f, 0xb7, 0x12],
        replacement: &[0x8b, 0x12, 0x90],
    },
    // 1001C989: material or variant offset; mov     ax, [esi+4]
    Patch {
        rva: 0x0001c989,
        original: &[0x66, 0x8b, 0x46, 0x04],
        replacement: &[0x66, 0x8b, 0x46, 0x08],
    },
    // 1001C99C: batch length DWORD; movzx   eax, word ptr [esi]
    Patch {
        rva: 0x0001c99c,
        original: &[0x0f, 0xb7, 0x06],
        replacement: &[0x8b, 0x06, 0x90],
    },
    // 1001C9A4: batch cursor stride; add     esi, 6
    Patch {
        rva: 0x0001c9a4,
        original: &[0x83, 0xc6, 0x06],
        replacement: &[0x83, 0xc6, 0x0c],
    },
    // 1001CB12: material or variant offset; mov     ax, [ecx+2]
    Patch {
        rva: 0x0001cb12,
        original: &[0x66, 0x8b, 0x41, 0x02],
        replacement: &[0x66, 0x8b, 0x41, 0x04],
    },
    // 1001CB87: variant comparison offset; cmp     word ptr [edx+2], 0
    Patch {
        rva: 0x0001cb87,
        original: &[0x66, 0x83, 0x7a, 0x02, 0x00],
        replacement: &[0x66, 0x83, 0x7a, 0x04, 0x00],
    },
    // 1001CC9F: batch length DWORD; movzx   edx, word ptr [edx]
    Patch {
        rva: 0x0001cc9f,
        original: &[0x0f, 0xb7, 0x12],
        replacement: &[0x8b, 0x12, 0x90],
    },
    // 1001CD54: variant comparison offset; cmp     word ptr [edx+2], 0
    Patch {
        rva: 0x0001cd54,
        original: &[0x66, 0x83, 0x7a, 0x02, 0x00],
        replacement: &[0x66, 0x83, 0x7a, 0x04, 0x00],
    },
    // 1001CE53: batch length DWORD; movzx   edx, word ptr [esi]
    Patch {
        rva: 0x0001ce53,
        original: &[0x0f, 0xb7, 0x16],
        replacement: &[0x8b, 0x16, 0x90],
    },
    // 1001CE80: batch length DWORD; movzx   edx, word ptr [esi]
    Patch {
        rva: 0x0001ce80,
        original: &[0x0f, 0xb7, 0x16],
        replacement: &[0x8b, 0x16, 0x90],
    },
    // 1001CF33: variant comparison offset; cmp     word ptr [edx+2], 0
    Patch {
        rva: 0x0001cf33,
        original: &[0x66, 0x83, 0x7a, 0x02, 0x00],
        replacement: &[0x66, 0x83, 0x7a, 0x04, 0x00],
    },
    // 1001D274: batch length DWORD; movzx   edx, word ptr [edx]
    Patch {
        rva: 0x0001d274,
        original: &[0x0f, 0xb7, 0x12],
        replacement: &[0x8b, 0x12, 0x90],
    },
    // 1001D2AF: material or variant offset; mov     ax, [eax+4]
    Patch {
        rva: 0x0001d2af,
        original: &[0x66, 0x8b, 0x40, 0x04],
        replacement: &[0x66, 0x8b, 0x40, 0x08],
    },
    // 1001D2C6: batch length DWORD; movzx   ecx, word ptr [eax]
    Patch {
        rva: 0x0001d2c6,
        original: &[0x0f, 0xb7, 0x08],
        replacement: &[0x8b, 0x08, 0x90],
    },
    // 1001D2CE: batch cursor stride; add     eax, 4
    Patch {
        rva: 0x0001d2ce,
        original: &[0x83, 0xc0, 0x04],
        replacement: &[0x83, 0xc0, 0x08],
    },
    // 1001D3E9: material or variant offset; mov     dx, [eax+2]
    Patch {
        rva: 0x0001d3e9,
        original: &[0x66, 0x8b, 0x50, 0x02],
        replacement: &[0x66, 0x8b, 0x50, 0x04],
    },
    // 1001D4D6: batch length DWORD; movzx   edx, word ptr [edx]
    Patch {
        rva: 0x0001d4d6,
        original: &[0x0f, 0xb7, 0x12],
        replacement: &[0x8b, 0x12, 0x90],
    },
    // 1001D60A: batch length DWORD; movzx   edx, word ptr [edi]
    Patch {
        rva: 0x0001d60a,
        original: &[0x0f, 0xb7, 0x17],
        replacement: &[0x8b, 0x17, 0x90],
    },
    // 1001D637: batch length DWORD; movzx   edx, word ptr [edi]
    Patch {
        rva: 0x0001d637,
        original: &[0x0f, 0xb7, 0x17],
        replacement: &[0x8b, 0x17, 0x90],
    },
    // 1001D763: batch length DWORD; movzx   ecx, word ptr [ecx]
    Patch {
        rva: 0x0001d763,
        original: &[0x0f, 0xb7, 0x09],
        replacement: &[0x8b, 0x09, 0x90],
    },
    // 1001D7BF: batch length DWORD; movzx   edx, word ptr [ecx]
    Patch {
        rva: 0x0001d7bf,
        original: &[0x0f, 0xb7, 0x11],
        replacement: &[0x8b, 0x11, 0x90],
    },
    // 1001D7CC: batch cursor stride; add     [esp+180h+var_174], 6
    Patch {
        rva: 0x0001d7cc,
        original: &[0x83, 0x44, 0x24, 0x0c, 0x06],
        replacement: &[0x83, 0x44, 0x24, 0x0c, 0x0c],
    },
    // 1001D7D3: batch cursor stride; add     [esp+180h+var_174], 4
    Patch {
        rva: 0x0001d7d3,
        original: &[0x83, 0x44, 0x24, 0x0c, 0x04],
        replacement: &[0x83, 0x44, 0x24, 0x0c, 0x08],
    },
];
