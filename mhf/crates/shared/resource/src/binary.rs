//! Typed binary reads retain their original bytes and writable buffer locations.

use crate::{Error, Result};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Endian {
    #[default]
    Little,
    Big,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl ScalarType {
    pub const fn size(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Scalar(ScalarType),
    Array {
        element: &'static ValueKind,
        len: usize,
    },
}

/// One codec supplies parsing, writing and field metadata. Slices must have
/// exactly SIZE bytes; codec errors use offsets relative to that slice.
pub trait BinaryValue: Sized {
    const SIZE: usize;
    const KIND: ValueKind;

    fn decode(bytes: &[u8], endian: Endian) -> Result<Self>;
    fn encode(&self, bytes: &mut [u8], endian: Endian) -> Result<()>;
}

fn check_size(bytes: &[u8], expected: usize) -> Result<()> {
    if bytes.len() != expected {
        return Err(Error::new(
            0,
            format!("expected {expected} binary bytes, got {}", bytes.len()),
        ));
    }
    Ok(())
}

// Every primitive has the same safe byte-array conversion; floating-point
// conversions preserve NaN payloads and signed zero without numeric coercion.
macro_rules! numbers {
    ($($ty:ty => $kind:ident),* $(,)?) => { $(
        impl BinaryValue for $ty {
            const SIZE: usize = ScalarType::$kind.size();
            const KIND: ValueKind = ValueKind::Scalar(ScalarType::$kind);

            fn decode(bytes: &[u8], endian: Endian) -> Result<Self> {
                check_size(bytes, Self::SIZE)?;
                let bytes = bytes.try_into().expect("checked binary scalar width");
                Ok(match endian {
                    Endian::Little => Self::from_le_bytes(bytes),
                    Endian::Big => Self::from_be_bytes(bytes),
                })
            }

            fn encode(&self, bytes: &mut [u8], endian: Endian) -> Result<()> {
                check_size(bytes, Self::SIZE)?;
                bytes.copy_from_slice(&match endian {
                    Endian::Little => self.to_le_bytes(),
                    Endian::Big => self.to_be_bytes(),
                });
                Ok(())
            }
        }
    )* };
}

numbers! {
    u8 => U8, u16 => U16, u32 => U32, u64 => U64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64,
    f32 => F32, f64 => F64,
}

impl<T: BinaryValue, const N: usize> BinaryValue for [T; N] {
    const SIZE: usize = T::SIZE * N;
    const KIND: ValueKind = ValueKind::Array {
        element: &T::KIND,
        len: N,
    };

    fn decode(bytes: &[u8], endian: Endian) -> Result<Self> {
        check_size(bytes, Self::SIZE)?;
        let mut values = [const { None }; N];
        for (index, value) in values.iter_mut().enumerate() {
            let offset = index * T::SIZE;
            *value = Some(
                T::decode(&bytes[offset..offset + T::SIZE], endian)
                    .map_err(|error| relocated(error, offset))?,
            );
        }
        Ok(values.map(|value| value.expect("every binary array element was decoded")))
    }

    fn encode(&self, bytes: &mut [u8], endian: Endian) -> Result<()> {
        check_size(bytes, Self::SIZE)?;
        for (index, value) in self.iter().enumerate() {
            let offset = index * T::SIZE;
            value
                .encode(&mut bytes[offset..offset + T::SIZE], endian)
                .map_err(|error| relocated(error, offset))?;
        }
        Ok(())
    }
}

fn relocated(mut error: Error, base: usize) -> Error {
    error.offset = base.saturating_add(error.offset);
    error
}

#[derive(Clone, Debug)]
pub struct Field<'a, T> {
    pub value: T,
    /// Absolute offsets in the complete backing buffer, including Reader::base.
    pub range: Range<usize>,
    pub endian: Endian,
    original: &'a [u8],
}

impl<'a, T: BinaryValue> Field<'a, T> {
    pub fn kind(&self) -> ValueKind {
        T::KIND
    }

    /// Borrow the exact source bytes, including noncanonical float bit patterns.
    pub fn bytes(&self) -> &'a [u8] {
        self.original
    }

    /// Write into the COMPLETE backing buffer, not the Reader's subslice.
    /// Unrelated bytes and the immutable source snapshot remain untouched.
    pub fn write(&self, buffer: &mut [u8], value: T) -> Result<()> {
        let target = buffer.get_mut(self.range.clone()).ok_or_else(|| {
            Error::new(self.range.start, "binary field exceeds destination buffer")
        })?;
        value
            .encode(target, self.endian)
            .map_err(|error| relocated(error, self.range.start))
    }
}

#[derive(Clone, Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    base: usize,
    position: usize,
    endian: Endian,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self::with_base(bytes, 0)
    }

    /// `bytes` is a borrowed resource slice. `base` locates its first byte in
    /// the complete buffer; all returned ranges and errors include this base.
    pub fn with_base(bytes: &'a [u8], base: usize) -> Self {
        Self {
            bytes,
            base,
            position: 0,
            endian: Endian::Little,
        }
    }

    pub fn with_endian(mut self, endian: Endian) -> Self {
        self.endian = endian;
        self
    }
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    pub fn base(&self) -> usize {
        self.base
    }
    /// Cursor relative to the supplied resource slice, excluding base.
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn endian(&self) -> Endian {
        self.endian
    }

    pub fn seek(&mut self, position: usize) -> Result<()> {
        self.range(position, 0)?;
        self.position = position;
        Ok(())
    }

    /// Advance only after a successful read, so a failed read is retryable.
    pub fn read<T: BinaryValue>(&mut self) -> Result<Field<'a, T>> {
        let value = self.read_at(self.position)?;
        self.position += T::SIZE;
        Ok(value)
    }

    /// `offset` is relative to the supplied slice. The cursor is unchanged.
    pub fn read_at<T: BinaryValue>(&self, offset: usize) -> Result<Field<'a, T>> {
        let range = self.range(offset, T::SIZE)?;
        let original = &self.bytes[offset..offset + T::SIZE];
        let value =
            T::decode(original, self.endian).map_err(|error| relocated(error, range.start))?;
        Ok(Field {
            value,
            range,
            endian: self.endian,
            original,
        })
    }

    fn range(&self, offset: usize, size: usize) -> Result<Range<usize>> {
        let start = self
            .base
            .checked_add(offset)
            .ok_or_else(|| Error::new(self.base, "binary offset overflow"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| Error::new(start, "binary range overflow"))?;
        if offset
            .checked_add(size)
            .is_none_or(|end| end > self.bytes.len())
        {
            return Err(Error::new(start, "binary field exceeds source buffer"));
        }
        Ok(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subresource_fields_keep_absolute_ranges_original_bytes_and_endianness() {
        let source = [0xaa, 0x34, 0x12, 0x01, 0x02, 0xbb];
        let mut reader = Reader::with_base(&source[1..5], 1);
        let little = reader.read::<u16>().unwrap();
        assert_eq!(little.value, 0x1234);
        assert_eq!(little.range, 1..3);
        assert_eq!(little.bytes().as_ptr(), source[1..].as_ptr());
        assert_eq!(little.kind(), ValueKind::Scalar(ScalarType::U16));
        let big = reader.with_endian(Endian::Big).read_at::<u16>(2).unwrap();
        assert_eq!(big.value, 0x0102);
        assert_eq!(big.range, 3..5);
        let mut output = source;
        little.write(&mut output, 0x5678).unwrap();
        big.write(&mut output, 0x9abc).unwrap();
        assert_eq!(output, [0xaa, 0x78, 0x56, 0x9a, 0xbc, 0xbb]);
        assert_eq!(little.bytes(), &[0x34, 0x12]);
    }

    #[test]
    fn failed_reads_writes_and_overflow_preserve_the_cursor_and_destination() {
        let mut reader = Reader::with_base(&[1, 2, 3], 100);
        reader.read::<u16>().unwrap();
        assert_eq!(reader.read::<u16>().unwrap_err().offset, 102);
        assert_eq!(reader.position(), 2);
        assert_eq!(reader.seek(4).unwrap_err().offset, 104);
        assert_eq!(reader.position(), 2);
        let mut output = [9; 8];
        let field = reader.read_at::<u16>(0).unwrap();
        assert_eq!(field.write(&mut output, 7).unwrap_err().offset, 100);
        assert_eq!(output, [9; 8]);
        assert_eq!(
            Reader::with_base(&[1], usize::MAX)
                .read::<u8>()
                .unwrap_err()
                .offset,
            usize::MAX
        );
        assert!(Reader::new(&[0; 4]).read_at::<u64>(usize::MAX).is_err());
    }

    #[test]
    fn const_arrays_share_the_scalar_codec_including_empty_and_nested_arrays() {
        let source = [0xff, 0xfe, 0x12, 0x34];
        let field = Reader::new(&source)
            .with_endian(Endian::Big)
            .read::<[i16; 2]>()
            .unwrap();
        assert_eq!(field.value, [-2, 0x1234]);
        assert_eq!(field.range, 0..4);
        assert_eq!(
            field.kind(),
            ValueKind::Array {
                element: &ValueKind::Scalar(ScalarType::I16),
                len: 2
            }
        );
        let mut output = source;
        field.write(&mut output, [i16::MIN, i16::MAX]).unwrap();
        assert_eq!(output, [0x80, 0, 0x7f, 0xff]);
        assert_eq!(Reader::new(&[]).read::<[u32; 0]>().unwrap().value, []);
        assert_eq!(
            Reader::new(&[]).read::<[[u8; 0]; 2]>().unwrap().value,
            [[], []]
        );
        assert_eq!(
            Reader::new(&source).read::<[[u8; 2]; 2]>().unwrap().value,
            [[0xff, 0xfe], [0x12, 0x34]]
        );
        assert!(<[u16; 2]>::decode(&[0; 3], Endian::Little).is_err());
    }

    #[test]
    fn nested_array_codec_errors_keep_absolute_offsets_and_do_not_advance() {
        #[derive(Debug)]
        struct Checked(u8);

        impl BinaryValue for Checked {
            const SIZE: usize = u8::SIZE;
            const KIND: ValueKind = u8::KIND;

            fn decode(bytes: &[u8], endian: Endian) -> Result<Self> {
                match u8::decode(bytes, endian)? {
                    u8::MAX => Err(Error::new(0, "reserved byte")),
                    value => Ok(Self(value)),
                }
            }

            fn encode(&self, bytes: &mut [u8], endian: Endian) -> Result<()> {
                self.0.encode(bytes, endian)
            }
        }

        let mut reader = Reader::with_base(&[1, 2, 3, u8::MAX], 100);
        let error = reader.read::<[[Checked; 2]; 2]>().unwrap_err();
        assert_eq!(error, Error::new(103, "reserved byte"));
        assert_eq!(reader.position(), 0);
        assert_eq!(reader.read::<Checked>().unwrap().value.0, 1);
    }

    #[test]
    fn float_reads_and_writes_preserve_nan_payloads_and_signed_zero() {
        for endian in [Endian::Little, Endian::Big] {
            for bits in [0x7fc0_1234_u32, 0xffc0_4321, 0x8000_0000] {
                let source = match endian {
                    Endian::Little => bits.to_le_bytes(),
                    Endian::Big => bits.to_be_bytes(),
                };
                let field = Reader::new(&source)
                    .with_endian(endian)
                    .read::<f32>()
                    .unwrap();
                assert_eq!(field.value.to_bits(), bits);
                let mut output = [0; 4];
                field.write(&mut output, field.value).unwrap();
                assert_eq!(output, source);
            }
            for bits in [0x7ff8_0000_0000_1234_u64, 0x8000_0000_0000_0000] {
                let source = match endian {
                    Endian::Little => bits.to_le_bytes(),
                    Endian::Big => bits.to_be_bytes(),
                };
                let field = Reader::new(&source)
                    .with_endian(endian)
                    .read::<f64>()
                    .unwrap();
                assert_eq!(field.value.to_bits(), bits);
                let mut output = [0; 8];
                field.write(&mut output, field.value).unwrap();
                assert_eq!(output, source);
            }
        }
    }
}
