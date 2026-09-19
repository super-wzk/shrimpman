//! The eight species-limit instructions read from the verified build.
//!
//! Each entry keeps the bytes it replaces, so `prepare` can prove the image is
//! still the build this table came from and `restore` can put the instruction
//! back exactly.

pub(crate) struct Patch {
    pub rva: usize,
    pub original: &'static [u8],
    pub replacement: &'static [u8],
}

pub(crate) const PATCHES: &[Patch] = &[
    // 1086A640: action dispatch; cmp byte ptr [esi+3], 177.
    Patch {
        rva: 0x0086a662,
        original: &[0x80, 0x7e, 0x03, 0xb1],
        replacement: &[0x80, 0x7e, 0x03, 0xff],
    },
    // 1086E490: initialization; cmp byte ptr [esi+3], 177.
    Patch {
        rva: 0x0086e494,
        original: &[0x80, 0x7e, 0x03, 0xb1],
        replacement: &[0x80, 0x7e, 0x03, 0xff],
    },
    // 1086E8A0: per-frame dispatch; cmp byte ptr [esi+3], 177.
    Patch {
        rva: 0x0086e8c5,
        original: &[0x80, 0x7e, 0x03, 0xb1],
        replacement: &[0x80, 0x7e, 0x03, 0xff],
    },
    // 1086E950: action dispatch; cmp byte ptr [esi+3], 177.
    Patch {
        rva: 0x0086e95b,
        original: &[0x80, 0x7e, 0x03, 0xb1],
        replacement: &[0x80, 0x7e, 0x03, 0xff],
    },
    // 108FD380: model parameters; mov ebx, 177 (cmp si, bx).
    Patch {
        rva: 0x008fd3a3,
        original: &[0xbb, 0xb1, 0x00, 0x00, 0x00],
        replacement: &[0xbb, 0xff, 0x00, 0x00, 0x00],
    },
    // 10AAA420: spawn attributes; mov edx, 177 (cmp ax, dx).
    Patch {
        rva: 0x00aaa45a,
        original: &[0xba, 0xb1, 0x00, 0x00, 0x00],
        replacement: &[0xba, 0xff, 0x00, 0x00, 0x00],
    },
    // 10B468B0: sound parameters; mov edx, 177 (cmp cx, dx).
    Patch {
        rva: 0x00b4698c,
        original: &[0xba, 0xb1, 0x00, 0x00, 0x00],
        replacement: &[0xba, 0xff, 0x00, 0x00, 0x00],
    },
    // 10B468B0: alternate sound path; mov edx, 177 (cmp ax, dx).
    Patch {
        rva: 0x00b47849,
        original: &[0xba, 0xb1, 0x00, 0x00, 0x00],
        replacement: &[0xba, 0xff, 0x00, 0x00, 0x00],
    },
];
