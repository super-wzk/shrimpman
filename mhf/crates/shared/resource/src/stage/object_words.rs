//! Counted DWORDs observed in stage object-package members with kind 14.
//!
//! Native `113DA8C0` skips this member instead of interpreting its values.
//! Their meanings remain unknown; the object descriptor supplies the context.

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct ObjectWordTable<'a> {
    pub unknown_00: u32,
    pub count: u32,
    pub values: &'a [u8],
    pub trailing: &'a [u8],
    pub source: &'a [u8],
}

impl<'a> ObjectWordTable<'a> {
    /// Parse a member explicitly identified by its kind-14 object descriptor.
    /// The unknown header word and any unconsumed bytes are retained as-is.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..8)
            .ok_or_else(|| Error::new(0, "truncated stage object word table header"))?;
        let count = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let end = (count as usize)
            .checked_mul(4)
            .and_then(|size| size.checked_add(8))
            .ok_or_else(|| Error::new(4, "stage object word table length overflow"))?;
        let values = source
            .get(8..end)
            .ok_or_else(|| Error::new(4, "stage object word table values exceed resource"))?;
        Ok(Self {
            unknown_00: u32::from_le_bytes(header[..4].try_into().unwrap()),
            count,
            values,
            trailing: &source[end..],
            source,
        })
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = u32> + DoubleEndedIterator + 'a {
        self.values
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes)
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        [0x1234_5678u32, 4, 0, 0x8000_0000, 0x7fc0_1234, u32::MAX]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    #[test]
    fn preserves_unknown_header_words_source_and_trailing_bytes() {
        let mut bytes = fixture();
        bytes.extend_from_slice(&[0xde, 0xad, 0xbe]);
        let file = ObjectWordTable::parse(&bytes).unwrap();
        assert_eq!(file.unknown_00, 0x1234_5678);
        assert_eq!(file.count, 4);
        assert_eq!(file.values().len(), 4);
        assert_eq!(
            file.values().collect::<Vec<_>>(),
            [0, 0x8000_0000, 0x7fc0_1234, u32::MAX]
        );
        assert_eq!(file.values().next_back(), Some(u32::MAX));
        assert_eq!(file.values.as_ptr(), bytes[8..].as_ptr());
        assert_eq!(file.values, &bytes[8..24]);
        assert_eq!(file.trailing, [0xde, 0xad, 0xbe]);
        assert_eq!(file.trailing.as_ptr(), bytes[24..].as_ptr());
        assert_eq!(file.as_bytes().as_ptr(), bytes.as_ptr());
        assert_eq!(file.as_bytes(), bytes);
    }

    #[test]
    fn checks_every_truncation_and_oversized_count() {
        let bytes = fixture();
        for length in 0..bytes.len() {
            assert!(
                ObjectWordTable::parse(&bytes[..length]).is_err(),
                "length {length}"
            );
        }
        let mut bytes = bytes;
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(ObjectWordTable::parse(&bytes).unwrap_err().offset, 4);
    }

    #[test]
    fn declared_count_bounds_values_without_discarding_extra_words() {
        let mut bytes = fixture();
        for count in [0u32, 1] {
            bytes[4..8].copy_from_slice(&count.to_le_bytes());
            let file = ObjectWordTable::parse(&bytes).unwrap();
            assert_eq!(file.count, count);
            assert_eq!(file.values().len(), count as usize);
            assert_eq!(file.values, &bytes[8..8 + count as usize * 4]);
            assert_eq!(file.trailing, &bytes[8 + count as usize * 4..]);
        }
        let file = ObjectWordTable::parse(&bytes[..8]).unwrap_err();
        assert_eq!(file.offset, 4);
        bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
        let file = ObjectWordTable::parse(&bytes[..8]).unwrap();
        assert!(file.values().next().is_none());
        assert!(file.trailing.is_empty());
    }
}
