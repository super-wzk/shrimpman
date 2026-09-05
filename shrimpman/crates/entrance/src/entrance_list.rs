use binrw::{
    BinResult, BinWrite, Endian,
    io::{self, Cursor, Seek, Write},
};

const SUM32_TABLE_0: [u8; 7] = [0x35, 0x7a, 0xaa, 0x97, 0x53, 0x66, 0x12];
const SUM32_TABLE_1: [u8; 9] = [0x7a, 0xaa, 0x97, 0x53, 0x66, 0x12, 0xde, 0xde, 0x35];

pub(crate) struct EntranceList<Entry, Metadata = ()> {
    pub(crate) entries: Vec<Entry>,
    pub(crate) metadata: Metadata,
}

impl<Entry> From<Vec<Entry>> for EntranceList<Entry> {
    fn from(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            metadata: (),
        }
    }
}

impl<Entry, Metadata> BinWrite for EntranceList<Entry, Metadata>
where
    Entry: for<'args> BinWrite<Args<'args> = ()>,
    Metadata: for<'args> BinWrite<Args<'args> = ()>,
{
    type Args<'args> = ();

    fn write_options<Writer: Write + Seek>(
        &self,
        writer: &mut Writer,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let entry_count = u16::try_from(self.entries.len())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut body = Cursor::new(Vec::new());
        for entry in &self.entries {
            entry.write_options(&mut body, endian, ())?;
        }
        self.metadata.write_options(&mut body, endian, ())?;
        let body = body.into_inner();
        let body_len = u16::try_from(body.len())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

        entry_count.write_options(writer, endian, ())?;
        body_len.write_options(writer, endian, ())?;
        if !body.is_empty() {
            sum32(&body).write_options(writer, endian, ())?;
        }
        writer.write_all(&body)?;
        Ok(())
    }
}

fn sum32(data: &[u8]) -> u32 {
    let Some(middle) = data.get(data.len() / 2) else {
        return 0;
    };

    let table_index_0 = data.len().wrapping_add(1) & 0xff;
    let table_index_1 = usize::from(middle.wrapping_add(1));
    let mut output = [0_u8; 4];

    for (index, byte) in data.iter().copied().enumerate() {
        let value = byte
            ^ SUM32_TABLE_0[(table_index_0 + index) % SUM32_TABLE_0.len()]
            ^ SUM32_TABLE_1[(table_index_1 + index) % SUM32_TABLE_1.len()];
        output[index & 3] = output[index & 3].wrapping_add(value);
    }

    u32::from_be_bytes(output)
}

#[cfg(test)]
mod tests {
    use binrw::{BinWriterExt, io::Cursor};

    use super::*;

    #[test]
    fn writes_the_entry_count_checksum_and_body() {
        let list = EntranceList::from(vec![*b"Lorem "]);
        let mut output = Cursor::new(Vec::new());

        output.write_be(&list).unwrap();

        assert_eq!(output.into_inner(), b"\0\x01\0\x06\x0a\xe6\xca\x2cLorem ");
    }

    #[test]
    fn omits_the_checksum_for_an_empty_body() {
        let list = EntranceList::<u8>::from(Vec::new());
        let mut output = Cursor::new(Vec::new());

        output.write_be(&list).unwrap();

        assert_eq!(output.into_inner(), b"\0\0\0\0");
    }

    #[test]
    fn matches_erupe_sum32_vectors() {
        for (input, expected) in [
            (b"Lorem ".as_slice(), 0x0ae6_ca2c),
            (b"ipsum dolor sit amet, ".as_slice(), 0xce5f_1e96),
            (b"consectetur adipiscing elit, ".as_slice(), 0xf3ee_cebb),
        ] {
            assert_eq!(sum32(input), expected);
        }
    }
}
