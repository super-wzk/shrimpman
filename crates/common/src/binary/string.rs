use std::{
    ffi::{CString, NulError},
    marker::PhantomData,
};

use binrw::{
    BinRead, BinResult, BinWrite, Endian, Error as BinError,
    io::{Read, Seek, Write},
};
use derive_more::Into;
use thiserror::Error;

use super::{counted_vec::CountedVec, custom_error, length::LengthEncoding};

/// A fixed-width null-terminated and zero-padded byte string.
#[repr(transparent)]
#[derive(Into)]
pub struct FixedCString<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> FixedCString<N> {
    pub fn new(value: impl AsRef<[u8]>) -> Result<Self, FixedCStringLengthError> {
        let value = value.as_ref();
        if N == 0 || value.len() >= N {
            return Err(FixedCStringLengthError {
                length: value.len(),
                width: N,
            });
        }

        let mut bytes = [0; N];
        bytes[..value.len()].copy_from_slice(value);
        Ok(Self { bytes })
    }
}

impl<const N: usize> BinRead for FixedCString<N> {
    type Args<'args> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        _endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let position = reader.stream_position()?;
        let mut bytes = [0; N];
        reader.read_exact(&mut bytes)?;

        if bytes.last() != Some(&0) {
            return Err(BinError::AssertFail {
                pos: position,
                message: "fixed C string is not null terminated".into(),
            });
        }

        Ok(Self { bytes })
    }
}

impl<const N: usize> BinWrite for FixedCString<N> {
    type Args<'args> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        writer.write_all(&self.bytes)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
#[error(
    "value is {length} bytes but the {width}-byte field reserves one byte for its null terminator"
)]
pub struct FixedCStringLengthError {
    length: usize,
    width: usize,
}

/// A C string encoded with its byte length before its null-terminated contents.
#[repr(transparent)]
#[derive(Into)]
#[into(CString)]
pub struct PrefixedCString<Encoding> {
    value: CString,
    #[into(skip)]
    encoding: PhantomData<fn() -> Encoding>,
}

impl<Encoding> PrefixedCString<Encoding> {
    pub fn new(value: impl Into<Vec<u8>>) -> Result<Self, NulError> {
        Ok(Self {
            value: CString::new(value)?,
            encoding: PhantomData,
        })
    }
}

impl<Encoding> From<CString> for PrefixedCString<Encoding> {
    fn from(value: CString) -> Self {
        Self {
            value,
            encoding: PhantomData,
        }
    }
}

impl<Encoding> BinRead for PrefixedCString<Encoding>
where
    Encoding: LengthEncoding,
{
    type Args<'args> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let position = reader.stream_position()?;
        let bytes = CountedVec::<Encoding, u8>::read_options(reader, endian, ())?;
        let value = CString::from_vec_with_nul(Vec::from(bytes))
            .map_err(|error| custom_error(position, error))?;

        Ok(value.into())
    }
}

impl<Encoding> BinWrite for PrefixedCString<Encoding>
where
    Encoding: LengthEncoding,
{
    type Args<'args> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let bytes = self.value.as_bytes_with_nul();
        Encoding::write_length(bytes.len(), writer, endian)?;
        writer.write_all(bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, mem::size_of};

    use binrw::{BinRead, BinWrite};

    use super::*;

    #[test]
    fn reads_fixed_c_string() {
        let mut input = Cursor::new(b"abc\0\0");

        let value = FixedCString::<5>::read_be(&mut input).unwrap();

        assert_eq!(<[u8; 5]>::from(value), *b"abc\0\0");
    }

    #[test]
    fn writes_fixed_c_string() {
        let value = FixedCString::<5>::new("abc").unwrap();
        let mut output = Cursor::new(Vec::new());

        value.write_be(&mut output).unwrap();

        assert_eq!(output.into_inner(), b"abc\0\0");
    }

    #[test]
    fn rejects_value_filling_fixed_c_string() {
        assert!(FixedCString::<3>::new("abc").is_err());
    }

    #[test]
    fn rejects_fixed_c_string_without_terminator() {
        let mut input = Cursor::new(b"abc");

        assert!(matches!(
            FixedCString::<3>::read_be(&mut input),
            Err(binrw::Error::AssertFail { .. })
        ));
    }

    #[test]
    fn fixed_c_string_has_the_same_size_as_its_bytes() {
        assert_eq!(size_of::<FixedCString<5>>(), size_of::<[u8; 5]>());
    }

    #[test]
    fn reads_length_prefixed_c_string() {
        let mut input = Cursor::new(b"\x04abc\0");

        let value = PrefixedCString::<u8>::read_be(&mut input).unwrap();

        assert_eq!(CString::from(value).as_bytes(), b"abc");
    }

    #[test]
    fn writes_length_prefixed_c_string() {
        let mut output = Cursor::new(Vec::new());

        PrefixedCString::<u8>::new("abc")
            .unwrap()
            .write_be(&mut output)
            .unwrap();

        assert_eq!(output.into_inner(), b"\x04abc\0");
    }

    #[test]
    fn rejects_length_prefixed_c_string_without_terminator() {
        let mut input = Cursor::new(b"\x03abc");

        assert!(PrefixedCString::<u8>::read_be(&mut input).is_err());
    }

    #[test]
    fn rejects_c_string_exceeding_its_length_prefix() {
        let value = PrefixedCString::<u8>::new(vec![b'a'; 255]).unwrap();
        let mut output = Cursor::new(Vec::new());

        assert!(matches!(
            value.write_be(&mut output),
            Err(binrw::Error::Custom { .. })
        ));
    }
}
