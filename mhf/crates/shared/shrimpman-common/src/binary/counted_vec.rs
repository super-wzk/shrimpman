use std::marker::PhantomData;

use binrw::{
    BinRead, BinResult, BinWrite, Endian, VecArgs,
    io::{Read, Seek, Write},
};
use derive_more::Into;

use super::length::LengthEncoding;

/// A vector encoded with its item count before its elements.
#[repr(transparent)]
#[derive(Into)]
#[into(Vec<T>)]
pub struct CountedVec<Encoding, T> {
    items: Vec<T>,
    #[into(skip)]
    encoding: PhantomData<fn() -> Encoding>,
}

impl<Encoding, T> CountedVec<Encoding, T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            encoding: PhantomData,
        }
    }
}

impl<Encoding, T> From<Vec<T>> for CountedVec<Encoding, T> {
    fn from(items: Vec<T>) -> Self {
        Self::new(items)
    }
}

impl<Encoding, T> BinRead for CountedVec<Encoding, T>
where
    Encoding: LengthEncoding,
    T: BinRead + 'static,
    for<'args> T::Args<'args>: Clone,
{
    type Args<'args> = T::Args<'args>;

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let count = Encoding::read_length(reader, endian)?;
        let items = Vec::read_options(reader, endian, VecArgs { count, inner: args })?;

        Ok(Self::new(items))
    }
}

impl<Encoding, T> BinWrite for CountedVec<Encoding, T>
where
    Encoding: LengthEncoding,
    T: BinWrite + 'static,
    for<'args> T::Args<'args>: Clone,
{
    type Args<'args> = T::Args<'args>;

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<()> {
        Encoding::write_length(self.items.len(), writer, endian)?;
        self.items.write_options(writer, endian, args)
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, mem::size_of};

    use binrw::{BinRead, BinWrite};

    use super::*;
    use crate::binary::{U8OrU16Length, U16OrU32Length};

    #[test]
    fn reads_length_prefixed_items() {
        let mut input = Cursor::new([0, 2, 0, 1, 0, 2]);

        let items = CountedVec::<u16, u16>::read_be(&mut input).unwrap();

        assert_eq!(Vec::from(items), [1, 2]);
    }

    #[test]
    fn writes_length_prefixed_items() {
        let items = CountedVec::<u8, u16>::new(vec![1, 2]);
        let mut output = Cursor::new(Vec::new());

        items.write_be(&mut output).unwrap();

        assert_eq!(output.into_inner(), [2, 0, 1, 0, 2]);
    }

    #[test]
    fn reads_u8_or_u16_length_prefixed_items() {
        let mut input = Cursor::new([0xFF, 0, 2, 1, 2]);

        let items = CountedVec::<U8OrU16Length, u8>::read_be(&mut input).unwrap();

        assert_eq!(Vec::from(items), [1, 2]);
    }

    #[test]
    fn reads_u16_or_u32_length_prefixed_items() {
        let mut input = Cursor::new([0xFF, 0xFF, 0, 0, 0, 2, 1, 2]);

        let items = CountedVec::<U16OrU32Length, u8>::read_be(&mut input).unwrap();

        assert_eq!(Vec::from(items), [1, 2]);
    }

    #[test]
    fn rejects_item_count_exceeding_its_length_prefix() {
        let items = CountedVec::<u8, u8>::new(vec![0; 256]);
        let mut output = Cursor::new(Vec::new());

        assert!(matches!(
            items.write_be(&mut output),
            Err(binrw::Error::Custom { .. })
        ));
    }

    #[test]
    fn has_the_same_size_as_vec() {
        assert_eq!(size_of::<CountedVec<u16, u8>>(), size_of::<Vec<u8>>());
        assert_eq!(
            size_of::<CountedVec<U8OrU16Length, u8>>(),
            size_of::<Vec<u8>>()
        );
    }
}
