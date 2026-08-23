use binrw::{
    BinRead, BinResult, BinWrite, Endian,
    io::{Read, Seek, Write},
};

use super::custom_error;

/// Encodes the number of elements preceding a length-prefixed value.
pub trait LengthEncoding {
    fn read_length<R: Read + Seek>(reader: &mut R, endian: Endian) -> BinResult<usize>;

    fn write_length<W: Write + Seek>(
        length: usize,
        writer: &mut W,
        endian: Endian,
    ) -> BinResult<()>;
}

macro_rules! impl_fixed_length_encoding {
    ($($length:ty),+ $(,)?) => {
        $(
            impl LengthEncoding for $length {
                fn read_length<R: Read + Seek>(
                    reader: &mut R,
                    endian: Endian,
                ) -> BinResult<usize> {
                    let position = reader.stream_position()?;
                    let length = Self::read_options(reader, endian, ())?;

                    length
                        .try_into()
                        .map_err(|error| custom_error(position, error))
                }

                fn write_length<W: Write + Seek>(
                    length: usize,
                    writer: &mut W,
                    endian: Endian,
                ) -> BinResult<()> {
                    let position = writer.stream_position()?;
                    let length = Self::try_from(length)
                        .map_err(|error| custom_error(position, error))?;

                    length.write_options(writer, endian, ())
                }
            }
        )+
    };
}

impl_fixed_length_encoding!(u8, u16, u32);

/// A length encoded as `u8`, or as `0xFF` followed by `u16`.
pub struct U8OrU16Length;

impl LengthEncoding for U8OrU16Length {
    fn read_length<R: Read + Seek>(reader: &mut R, endian: Endian) -> BinResult<usize> {
        let length = u8::read_options(reader, endian, ())?;
        if length == u8::MAX {
            Ok(usize::from(u16::read_options(reader, endian, ())?))
        } else {
            Ok(usize::from(length))
        }
    }

    fn write_length<W: Write + Seek>(
        length: usize,
        writer: &mut W,
        endian: Endian,
    ) -> BinResult<()> {
        if length < usize::from(u8::MAX) {
            return u8::write_length(length, writer, endian);
        }

        let position = writer.stream_position()?;
        let extended = u16::try_from(length).map_err(|error| custom_error(position, error))?;
        u8::MAX.write_options(writer, endian, ())?;
        extended.write_options(writer, endian, ())
    }
}

/// A length encoded as `u16`, or as `0xFFFF` followed by `u32`.
pub struct U16OrU32Length;

impl LengthEncoding for U16OrU32Length {
    fn read_length<R: Read + Seek>(reader: &mut R, endian: Endian) -> BinResult<usize> {
        let length = u16::read_options(reader, endian, ())?;
        if length == u16::MAX {
            u32::read_length(reader, endian)
        } else {
            Ok(usize::from(length))
        }
    }

    fn write_length<W: Write + Seek>(
        length: usize,
        writer: &mut W,
        endian: Endian,
    ) -> BinResult<()> {
        if length < usize::from(u16::MAX) {
            return u16::write_length(length, writer, endian);
        }

        let position = writer.stream_position()?;
        let extended = u32::try_from(length).map_err(|error| custom_error(position, error))?;
        u16::MAX.write_options(writer, endian, ())?;
        extended.write_options(writer, endian, ())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::Endian;

    use super::*;

    #[test]
    fn writes_u8_or_u16_length_at_the_sentinel() {
        let mut output = Cursor::new(Vec::new());

        U8OrU16Length::write_length(usize::from(u8::MAX), &mut output, Endian::Big).unwrap();

        assert_eq!(output.into_inner(), [0xFF, 0, 0xFF]);
    }

    #[test]
    fn writes_u16_or_u32_length_at_the_sentinel() {
        let mut output = Cursor::new(Vec::new());

        U16OrU32Length::write_length(usize::from(u16::MAX), &mut output, Endian::Big).unwrap();

        assert_eq!(output.into_inner(), [0xFF, 0xFF, 0, 0, 0xFF, 0xFF]);
    }

    #[test]
    fn rejects_extended_length_exceeding_its_long_prefix() {
        let mut output = Cursor::new(Vec::new());

        assert!(matches!(
            U8OrU16Length::write_length(usize::from(u16::MAX) + 1, &mut output, Endian::Big),
            Err(binrw::Error::Custom { .. })
        ));
        assert!(output.into_inner().is_empty());
    }
}
