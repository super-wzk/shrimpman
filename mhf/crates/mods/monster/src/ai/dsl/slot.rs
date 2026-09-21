//! Native subscript slots and their call/return encodings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeSlot {
    pub table: usize,
    pub index: u8,
}

impl NativeSlot {
    pub(crate) fn from_call(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0x16, index] => Some(Self {
                table: 9,
                index: *index,
            }),
            [0x81, index] => Some(Self {
                table: 1,
                index: *index,
            }),
            [0x82, group, index] => Some(Self {
                table: 15 + usize::from(*group),
                index: *index,
            }),
            _ => None,
        }
    }

    pub(crate) fn call(self) -> Vec<u8> {
        match self.table {
            1 => vec![0x81, self.index],
            9 => vec![0x16, self.index],
            _ => vec![0x82, (self.table - 15) as u8, self.index],
        }
    }

    pub(crate) fn ending(self) -> u8 {
        match self.table {
            1 => 1,
            9 => 3,
            _ => 2,
        }
    }

    pub(crate) fn is_same_level_call(self, bytes: &[u8]) -> bool {
        matches!((self.table, bytes), (1, [0x81, _]) | (15.., [0x82, _, _]))
    }
}
