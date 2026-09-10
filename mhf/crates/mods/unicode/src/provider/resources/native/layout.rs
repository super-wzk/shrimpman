//! Shared by the dictionary compiler and the native image reader.
//! All addresses are RVAs in the supported unpacked client.

pub struct Literal {
    pub rva: usize,
    pub sites: &'static [usize],
}

pub struct Table {
    pub id: &'static str,
    pub root: usize,
    pub records: u32,
    pub stride: usize,
    pub text_offset: usize,
    pub source_start: usize,
    pub source_end: usize,
}

pub const TABLES: &[Table] = &[
    // 10445D06 and 104468AE index { label: char*, rank: u16, padding: u16 }.
    Table {
        id: "rank",
        root: 0x01923254,
        records: 5,
        stride: 8,
        text_offset: 0,
        source_start: 0x01996188,
        source_end: 0x01996195,
    },
    // The six room-status/name fields consumed by 1155B620, 1155C740 and
    // 115717A0. The following pointer is an unrelated debug-name directory.
    Table {
        id: "room",
        root: 0x019DDB0C,
        records: 6,
        stride: 4,
        text_offset: 0,
        source_start: 0x019B286C,
        source_end: 0x019B28B9,
    },
];

#[path = "bindings.rs"]
mod bindings;
pub use bindings::LITERALS;
