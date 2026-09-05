use binrw::{
    BinRead, BinResult, BinWrite, Endian,
    io::{self, Read, Seek, SeekFrom, Write},
};

const DEFAULT_SEED: u8 = 0;
const MASK: [u8; 8] = [0x01, 0x23, 0x34, 0x45, 0x56, 0xAB, 0xCD, 0xEF];
const ROTATION_MULTIPLIER: u32 = 54_323;
const WRITE_BUFFER_LEN: usize = 1_024;

/// A value encoded using MHF's Bin8 byte transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MhfBin8<T> {
    seed: u8,
    inner: T,
}

impl<T> MhfBin8<T> {
    pub const fn new(inner: T) -> Self {
        Self {
            seed: DEFAULT_SEED,
            inner,
        }
    }
}

impl<T> From<T> for MhfBin8<T> {
    fn from(inner: T) -> Self {
        Self::new(inner)
    }
}

impl<T> BinRead for MhfBin8<T>
where
    T: BinRead,
{
    type Args<'args> = T::Args<'args>;

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let seed = u8::read_options(reader, endian, ())?;
        let mut stream = Bin8Stream::new(reader, seed)?;
        let inner = T::read_options(&mut stream, endian, args)?;

        Ok(Self { seed, inner })
    }
}

impl<T> BinWrite for MhfBin8<T>
where
    T: BinWrite,
{
    type Args<'args> = T::Args<'args>;

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<()> {
        self.seed.write_options(writer, endian, ())?;
        let mut stream = Bin8Stream::new(writer, self.seed)?;
        self.inner.write_options(&mut stream, endian, args)
    }
}

struct Bin8Stream<'stream, Stream> {
    stream: &'stream mut Stream,
    origin: u64,
    state: Bin8State,
}

impl<'stream, Stream> Bin8Stream<'stream, Stream>
where
    Stream: Seek,
{
    fn new(stream: &'stream mut Stream, seed: u8) -> io::Result<Self> {
        let origin = stream.stream_position()?;

        Ok(Self {
            stream,
            origin,
            state: Bin8State::new(seed),
        })
    }

    fn seek_absolute(&mut self, absolute: u64) -> io::Result<u64> {
        let absolute = self.stream.seek(SeekFrom::Start(absolute))?;
        let position = absolute.checked_sub(self.origin).ok_or_else(invalid_seek)?;
        self.state.seek(position);
        Ok(position)
    }
}

impl<Stream> Read for Bin8Stream<'_, Stream>
where
    Stream: Read,
{
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.stream.read(buffer)?;
        self.state.apply(&mut buffer[..read]);
        Ok(read)
    }
}

impl<Stream> Write for Bin8Stream<'_, Stream>
where
    Stream: Write,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let len = buffer.len().min(WRITE_BUFFER_LEN);
        let mut encoded = [0; WRITE_BUFFER_LEN];
        encoded[..len].copy_from_slice(&buffer[..len]);

        let mut state = self.state;
        state.apply(&mut encoded[..len]);

        let written = self.stream.write(&encoded[..len])?;
        self.state.advance(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl<Stream> Seek for Bin8Stream<'_, Stream>
where
    Stream: Seek,
{
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        match position {
            SeekFrom::Start(position) => {
                let absolute = self.origin.checked_add(position).ok_or_else(invalid_seek)?;
                self.seek_absolute(absolute)
            }
            SeekFrom::Current(offset) => {
                let position = self
                    .state
                    .position
                    .checked_add_signed(offset)
                    .ok_or_else(invalid_seek)?;
                let absolute = self.origin.checked_add(position).ok_or_else(invalid_seek)?;
                self.seek_absolute(absolute)
            }
            SeekFrom::End(offset) => {
                let previous = self
                    .origin
                    .checked_add(self.state.position)
                    .ok_or_else(invalid_seek)?;
                let absolute = self.stream.seek(SeekFrom::End(offset))?;
                if absolute < self.origin {
                    self.stream.seek(SeekFrom::Start(previous))?;
                    return Err(invalid_seek());
                }

                self.state.seek(absolute - self.origin);
                Ok(self.state.position)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Bin8State {
    seed: u8,
    rotation: u32,
    position: u64,
}

impl Bin8State {
    const fn new(seed: u8) -> Self {
        Self {
            seed,
            rotation: seed as u32,
            position: 0,
        }
    }

    fn apply(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.rotation = rotate(self.rotation);
            *byte ^= MASK[(self.position & 7) as usize] ^ (self.rotation >> 13) as u8;
            self.position += 1;
        }
    }

    fn advance(&mut self, distance: u64) {
        self.rotation = advance_rotation(self.rotation, distance);
        self.position += distance;
    }

    fn seek(&mut self, position: u64) {
        self.rotation = advance_rotation(u32::from(self.seed), position);
        self.position = position;
    }
}

const fn rotate(rotation: u32) -> u32 {
    rotation.wrapping_mul(ROTATION_MULTIPLIER).wrapping_add(1)
}

fn advance_rotation(rotation: u32, mut distance: u64) -> u32 {
    let mut current_multiplier = ROTATION_MULTIPLIER;
    let mut current_increment = 1_u32;
    let mut accumulated_multiplier = 1_u32;
    let mut accumulated_increment = 0_u32;

    while distance != 0 {
        if distance & 1 != 0 {
            accumulated_multiplier = accumulated_multiplier.wrapping_mul(current_multiplier);
            accumulated_increment = accumulated_increment
                .wrapping_mul(current_multiplier)
                .wrapping_add(current_increment);
        }

        current_increment = current_increment.wrapping_mul(current_multiplier.wrapping_add(1));
        current_multiplier = current_multiplier.wrapping_mul(current_multiplier);
        distance >>= 1;
    }

    accumulated_multiplier
        .wrapping_mul(rotation)
        .wrapping_add(accumulated_increment)
}

fn invalid_seek() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid Bin8 stream seek")
}

#[cfg(test)]
mod tests {
    use binrw::{BinRead, BinWrite, io::Cursor};

    use super::*;

    #[test]
    fn writes_known_bin8_vector() {
        let value = MhfBin8 {
            seed: 0x55,
            inner: *b"Lorem ",
        };
        let mut output = Cursor::new(Vec::new());

        value.write_be(&mut output).unwrap();

        assert_eq!(
            output.into_inner(),
            [0x55, 0x7E, 0x4C, 0x1D, 0x16, 0x9D, 0x46]
        );
    }

    #[test]
    fn reads_known_bin8_vector() {
        let mut input = Cursor::new([0x55, 0x7E, 0x4C, 0x1D, 0x16, 0x9D, 0x46]);

        let value = MhfBin8::<[u8; 6]>::read_be(&mut input).unwrap();

        assert_eq!(value.seed, 0x55);
        assert_eq!(value.inner, *b"Lorem ");
    }

    #[test]
    fn starts_the_transformed_stream_at_the_wrapped_value() {
        let mut output = Cursor::new(vec![0xAA, 0xBB]);
        output.set_position(2);

        MhfBin8 {
            seed: 0x55,
            inner: *b"Lorem ",
        }
        .write_be(&mut output)
        .unwrap();

        assert_eq!(
            output.into_inner(),
            [0xAA, 0xBB, 0x55, 0x7E, 0x4C, 0x1D, 0x16, 0x9D, 0x46]
        );
    }

    #[test]
    fn keeps_the_transformation_aligned_across_seeks() {
        let mut output = Cursor::new(Vec::new());
        output.write_all(&[0]).unwrap();

        {
            let mut stream = Bin8Stream::new(&mut output, 0).unwrap();
            stream.write_all(b"ab00ef").unwrap();
            stream.seek(SeekFrom::Start(2)).unwrap();
            stream.write_all(b"cd").unwrap();
            stream.seek(SeekFrom::End(0)).unwrap();
        }

        output.set_position(0);
        let decoded = MhfBin8::<[u8; 6]>::read_be(&mut output).unwrap();
        assert_eq!(decoded.inner, *b"abcdef");
    }

    #[test]
    fn default_seed_is_zero() {
        assert_eq!(MhfBin8::new(()).seed, 0);
        assert_eq!(MhfBin8::from(()).inner, ());
    }
}
