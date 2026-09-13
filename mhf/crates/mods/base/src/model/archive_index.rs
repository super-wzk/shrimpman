//! The native ABN lookup needs only the MHA directory, not decoded payloads.

use mhf_resource::container::MhaHeader;
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Index {
    pub first: i16,
    pub entries: Vec<[u32; 2]>,
}

impl Index {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, String> {
        let size = reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        reader.rewind().map_err(|e| e.to_string())?;
        let mut header = [0; MhaHeader::SIZE];
        reader.read_exact(&mut header).map_err(|e| e.to_string())?;
        let header = MhaHeader::parse(&header).map_err(|e| e.to_string())?;
        if header.file_id_count > i16::MAX as u16 {
            return Err("MHA 资源 ID 槽数超出原生范围".into());
        }
        let directory_end = u64::from(header.entries_offset) + u64::from(header.count) * 20;
        let names_end = u64::from(header.names_offset) + u64::from(header.names_size);
        if header.entries_offset < 24
            || header.names_offset < 24
            || directory_end > size
            || names_end > size
        {
            return Err("MHA 目录或名称范围超出文件".into());
        }
        let mut index = Self {
            first: header.first_file_id,
            entries: vec![[0; 2]; usize::from(header.file_id_count)],
        };
        reader
            .seek(SeekFrom::Start(header.entries_offset.into()))
            .map_err(|e| e.to_string())?;
        for _ in 0..header.count {
            let mut record = [0; 20];
            reader.read_exact(&mut record).map_err(|e| e.to_string())?;
            let word = |at| u32::from_le_bytes(record[at..at + 4].try_into().unwrap());
            let (offset, length, padded) = (word(4), word(8), word(12));
            if word(0) >= header.names_size
                || padded < length
                || u64::from(offset) + u64::from(padded) > size
                || (length != 0 && offset < 24)
            {
                return Err("MHA 成员范围无效".into());
            }
            let slot = i32::from(word(16) as i16) - i32::from(index.first);
            let entry = usize::try_from(slot)
                .ok()
                .and_then(|slot| index.entries.get_mut(slot))
                .ok_or("MHA 成员 ID 超出目录范围")?;
            // Match 1158C300: the last record wins, including empty entries.
            *entry = [offset, length];
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhf_resource::container::MhaArchive;
    use std::io::Cursor;

    fn archive(size: u32) -> Vec<u8> {
        let mut bytes = vec![0; 128 + size as usize + 12];
        bytes[..4].copy_from_slice(b"mha\x01");
        for (at, value) in [(4, 24u32), (8, 2), (12, 64), (16, 2)] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[20..22].copy_from_slice(&521i16.to_le_bytes());
        bytes[22..24].copy_from_slice(&3u16.to_le_bytes());
        bytes[64] = b'a';
        for (row, values) in [[0, 128, size, size, 521], [0, 128 + size, 12, 12, 523]]
            .into_iter()
            .enumerate()
        {
            for (column, value) in values.into_iter().enumerate() {
                let at = 24 + row * 20 + column * 4;
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn refreshed_directory_tracks_growth_shrinkage_and_later_members() {
        for size in [23725, 35316, 100] {
            let bytes = archive(size);
            let parsed = MhaArchive::parse(&bytes, 100).unwrap();
            let file_ids = parsed.file_id_index().unwrap();
            let refreshed = Index::read(&mut Cursor::new(&bytes)).unwrap();
            assert_eq!(refreshed.first, 521);
            for (slot, entry) in file_ids.slots.iter().enumerate() {
                assert_eq!(
                    refreshed.entries[slot],
                    entry.map_or([0, 0], |i| {
                        let entry = parsed.entries[i].entry;
                        [entry.offset, entry.size]
                    })
                );
            }
            assert_eq!(refreshed.entries, [[128, size], [0, 0], [128 + size, 12]]);
        }
    }

    #[test]
    fn duplicate_native_ids_keep_the_last_empty_record_and_ignore_high_word() {
        let mut bytes = archive(10);
        bytes[48..52].copy_from_slice(&0u32.to_le_bytes());
        bytes[52..60].fill(0);
        bytes[60..64].copy_from_slice(&0xabcd0209u32.to_le_bytes());
        let index = Index::read(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(index.entries, [[0, 0]; 3]);
    }

    #[test]
    fn malformed_indices_and_truncated_files_fail_before_publication() {
        for (at, value) in [
            (4, u32::MAX),
            (12, u32::MAX),
            (32, 99999),
            (36, 1),
            (40, 520),
        ] {
            let mut bytes = archive(10);
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                Index::read(&mut Cursor::new(bytes)).is_err(),
                "accepted field {at}"
            );
        }
        let mut bytes = archive(10);
        bytes[22..24].copy_from_slice(&32768u16.to_le_bytes());
        assert!(Index::read(&mut Cursor::new(bytes)).is_err());
        assert!(Index::read(&mut Cursor::new(&archive(10)[..60])).is_err());
    }

    #[test]
    fn index_refresh_does_not_read_resource_payloads() {
        struct Counted {
            source: Cursor<Vec<u8>>,
            bytes: usize,
        }
        impl Read for Counted {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                let count = self.source.read(bytes)?;
                self.bytes += count;
                Ok(count)
            }
        }
        impl Seek for Counted {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.source.seek(position)
            }
        }
        let mut source = Counted {
            source: Cursor::new(archive(1_000_000)),
            bytes: 0,
        };
        Index::read(&mut source).unwrap();
        assert_eq!(source.bytes, 24 + 2 * 20);
    }
}
