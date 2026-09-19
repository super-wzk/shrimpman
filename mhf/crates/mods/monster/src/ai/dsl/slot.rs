//! Source annotations that preserve native subscript table positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NativeSlot {
    pub table: usize,
    pub index: u8,
}

impl NativeSlot {
    pub fn from_call(bytes: &[u8]) -> Option<Self> {
        match bytes {
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

    pub fn call(self) -> Vec<u8> {
        if self.table == 1 {
            vec![0x81, self.index]
        } else {
            vec![0x82, (self.table - 15) as u8, self.index]
        }
    }

    pub fn ending(self) -> u8 {
        if self.table == 1 { 1 } else { 2 }
    }

    pub fn is_same_level_call(self, bytes: &[u8]) -> bool {
        matches!((self.table, bytes), (1, [0x81, _]) | (15.., [0x82, _, _]))
    }
}
