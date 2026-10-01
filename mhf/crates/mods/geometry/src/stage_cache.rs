//! Sizing and instruction edits for independently growing stage workspaces.

pub(crate) const RAW_GLOBAL: usize = 0x0ed528bc;
pub(crate) const DECODED_GLOBAL: usize = 0x0ed528d8;
pub(crate) const ORIGINAL_RAW: usize = 0x0c404740;
pub(crate) const ORIGINAL_DECODED: usize = 0x02b04740;
pub(crate) const SHARED_RESET: usize = 0x008e33bd;
// BC is also native scratch space outside stage loading. Retain its original floor.
pub(crate) const RAW_MINIMUM: usize = 14 * 1024 * 1024;
pub(crate) const DECODE_CALLERS: [usize; 4] = [0x004114bb, 0x0060e0b9, 0x0089f19f, 0x0089f3a2];
pub(crate) const BUILD_CALLERS: [usize; 4] = [0x00411539, 0x0060e12c, 0x0089f2c4, 0x0089f422];

/// Native reads append the relative filename and return the length without its NUL.
/// Campaign TXB starts at that return offset, overwriting the first filename's NUL.
pub(crate) fn raw_size(
    file_size: usize,
    path_length: usize,
    campaign: Option<(usize, usize)>,
) -> Result<usize, String> {
    let mut required = file_size
        .checked_add(path_length)
        .ok_or("stage PAC size overflow")?;
    if let Some((size, path)) = campaign {
        required = required
            .checked_add(size)
            .and_then(|n| n.checked_add(path))
            .ok_or("stage campaign size overflow")?;
    }
    Ok(required
        .checked_add(1)
        .ok_or("stage PAC size overflow")?
        .max(RAW_MINIMUM))
}

/// Match 108DF570's output sizing without treating a compressed byte count as capacity.
pub(crate) fn decoded_size(prefix: &[u8], stored_size: usize) -> Result<usize, String> {
    let size = if prefix.starts_with(b"JKR\x1a") {
        if stored_size < 16 {
            return Err("truncated stage JKR header".into());
        }
        let codec = u16::from_le_bytes(
            prefix
                .get(6..8)
                .ok_or("truncated stage JKR header")?
                .try_into()
                .unwrap(),
        );
        let offset = crate::fmod::word(prefix, 8)? as usize;
        let decoded = crate::fmod::word(prefix, 12)? as usize;
        if !matches!(codec, 0 | 2 | 3 | 4)
            || !(16..=stored_size).contains(&offset)
            || (decoded != 0 && offset == stored_size)
            || (codec == 0 && decoded > stored_size - offset)
            // Native Huffman initialization prefetches even with zero output.
            || (matches!(codec, 2 | 4) && stored_size - offset < 7)
        {
            return Err("invalid or unsupported stage JKR header".into());
        }
        decoded
    } else {
        stored_size
    };
    crate::mesh::byte_size(size, 1)?;
    if size != 0 && size < 12 {
        return Err("stage model is smaller than its block header".into());
    }
    Ok(size)
}

pub(crate) fn pair_sizes(bytes: &[u8]) -> Result<[usize; 2], String> {
    if crate::fmod::word(bytes, 0)? < 2 || bytes.len() < 20 {
        return Err("stage model pair has no FMOD/FSKL entries".into());
    }
    let mut sizes = [0; 2];
    for (index, size) in sizes.iter_mut().enumerate() {
        let offset = crate::fmod::word(bytes, 4 + index * 8)? as usize;
        let stored = crate::fmod::word(bytes, 8 + index * 8)? as usize;
        let tail = bytes
            .get(offset..)
            .ok_or("stage model pair offset out of bounds")?;
        if tail.len() < stored.max(4) {
            return Err("truncated stage model pair".into());
        }
        *size = decoded_size(&tail[..tail.len().min(16)], stored)?;
    }
    Ok(sizes)
}

pub(crate) fn shared_reset(base: usize) -> [u8; 5] {
    let mut bytes = [0xb8, 0, 0, 0, 0];
    bytes[1..].copy_from_slice(&((base + ORIGINAL_DECODED) as u32).to_le_bytes());
    bytes
}

pub(crate) enum Edit {
    RawReset,
    DecodedReset,
    FsklPointer,
}

pub(crate) struct Patch {
    pub rva: usize,
    pub original: &'static [u8],
    pub edit: Edit,
}

impl Patch {
    /// Rebase the two absolute reset operands, matching the loaded PE image.
    pub(crate) fn original_at(&self, base: usize) -> Vec<u8> {
        let mut bytes = self.original.to_vec();
        match self.edit {
            Edit::RawReset => {
                bytes[2..6].copy_from_slice(&((base + RAW_GLOBAL) as u32).to_le_bytes());
                bytes[6..10].copy_from_slice(&((base + ORIGINAL_RAW) as u32).to_le_bytes());
            }
            Edit::DecodedReset => {
                bytes[1..5].copy_from_slice(&((base + DECODED_GLOBAL) as u32).to_le_bytes());
            }
            Edit::FsklPointer => {}
        }
        bytes
    }

    pub(crate) fn replacement(&self, base: usize, fskl_pointer_cell: usize) -> Vec<u8> {
        let mut bytes = self.original_at(base);
        match self.edit {
            // Preserve EAX for the neighbouring D4/E0/DC workspace aliases.
            Edit::RawReset | Edit::DecodedReset => bytes.fill(0x90),
            Edit::FsklPointer => {
                bytes[..2].copy_from_slice(&[0x8b, 0x0d]);
                bytes[2..6].copy_from_slice(&(fskl_pointer_cell as u32).to_le_bytes());
            }
        }
        bytes
    }
}

// Decode interception supplies both destinations; builders read the stable FSKL cell.
// The two reset writes are the only writes to BC/D8 in the verified DLL.
pub(crate) const PATCHES: &[Patch] = &[
    Patch {
        rva: 0x0041152d,
        original: &[0x8d, 0x88, 0x00, 0x00, 0x80, 0x00],
        edit: Edit::FsklPointer,
    },
    Patch {
        rva: 0x0060e120,
        original: &[0x8d, 0x88, 0x00, 0x00, 0x80, 0x00],
        edit: Edit::FsklPointer,
    },
    Patch {
        rva: 0x0089f2b7,
        original: &[0x8d, 0x88, 0x00, 0x00, 0x80, 0x00],
        edit: Edit::FsklPointer,
    },
    Patch {
        rva: 0x0089f40d,
        original: &[0x8d, 0x88, 0x00, 0x00, 0x80, 0x00],
        edit: Edit::FsklPointer,
    },
    Patch {
        rva: 0x008e3451,
        original: &[0xc7, 0x05, 0xbc, 0x28, 0xd5, 0x1e, 0x40, 0x47, 0x40, 0x1c],
        edit: Edit::RawReset,
    },
    Patch {
        rva: 0x008e3492,
        original: &[0xa3, 0xd8, 0x28, 0xd5, 0x1e],
        edit: Edit::DecodedReset,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_size_reserves_campaign_data_and_both_filenames() {
        let file = 64 * 1024 * 1024 - 18;
        assert_eq!(raw_size(file, 17, None).unwrap(), 64 * 1024 * 1024);
        assert_eq!(
            raw_size(file, 17, Some((1, 17))).unwrap(),
            64 * 1024 * 1024 + 18
        );
        assert!(raw_size(usize::MAX, 1, None).is_err());
        assert!(raw_size(1, 1, Some((usize::MAX, 1))).is_err());
        let required = raw_size(i32::MAX as usize, 1, None).unwrap();
        assert!(crate::cache::Buffer::default().prepare(required).is_err());
    }

    #[test]
    fn paired_outputs_use_decoded_sizes_and_reject_truncated_segments() {
        let mut bytes = vec![0u8; 49];
        for (index, word) in [2u32, 20, 17, 37, 12].into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[20..24].copy_from_slice(b"JKR\x1a");
        bytes[26..28].copy_from_slice(&3u16.to_le_bytes());
        bytes[28..32].copy_from_slice(&16u32.to_le_bytes());
        let large = 33 * 1024 * 1024;
        bytes[32..36].copy_from_slice(&(large as u32).to_le_bytes());
        assert_eq!(pair_sizes(&bytes).unwrap(), [large, 12]);
        assert!(pair_sizes(&bytes[..48]).is_err());
        bytes[26..28].copy_from_slice(&0u16.to_le_bytes());
        assert!(
            pair_sizes(&bytes).is_err(),
            "raw JKR must contain its advertised output"
        );
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        assert!(
            pair_sizes(&bytes).is_err(),
            "native pair decoder does not support codec 1"
        );
        bytes[26..28].copy_from_slice(&3u16.to_le_bytes());
        bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(pair_sizes(&bytes).is_err());
    }

    #[test]
    fn huffman_initialization_requires_payload_even_for_empty_output() {
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"JKR\x1a");
        header[8..12].copy_from_slice(&16u32.to_le_bytes());
        for codec in [2u16, 4] {
            header[6..8].copy_from_slice(&codec.to_le_bytes());
            header[12..16].copy_from_slice(&0u32.to_le_bytes());
            assert!(decoded_size(&header, 16).is_err());
            header[12..16].copy_from_slice(&24u32.to_le_bytes());
            assert!(decoded_size(&header, 17).is_err());
            assert_eq!(decoded_size(&header, 23).unwrap(), 24);
        }
    }

    #[test]
    fn reset_edits_leave_the_shared_decoder_aliases_unchanged() {
        let base = 0x20000000;
        let cell = 0x50000000;
        for patch in PATCHES {
            let old = patch.original_at(base);
            let new = patch.replacement(base, cell);
            assert_eq!(old.len(), new.len());
            match patch.edit {
                Edit::RawReset | Edit::DecodedReset => assert!(new.iter().all(|&b| b == 0x90)),
                Edit::FsklPointer => {
                    assert_eq!(&new[..2], &[0x8b, 0x0d]);
                    assert_eq!(
                        u32::from_le_bytes(new[2..6].try_into().unwrap()),
                        cell as u32
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "set MHF_GEOMETRY_TEST_CLIENT to either verified ZZ HD DLL"]
    fn stage_instruction_edits_match_the_actual_client() {
        let data = std::fs::read(std::env::var("MHF_GEOMETRY_TEST_CLIENT").unwrap()).unwrap();
        let word = |offset| crate::fmod::word(&data, offset).unwrap() as usize;
        let half =
            |offset| u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()) as usize;
        let pe = word(0x3c);
        let base = word(pe + 24 + 28);
        let section_start = pe + 24 + half(pe + 20);
        let sections: Vec<_> = (0..half(pe + 6))
            .map(|i| {
                let offset = section_start + i * 40;
                (word(offset + 12), word(offset + 16), word(offset + 20))
            })
            .collect();
        let &(rva, _, offset) = sections
            .iter()
            .find(|&&(rva, size, _)| (rva..rva + size).contains(&SHARED_RESET))
            .unwrap();
        let offset = offset + SHARED_RESET - rva;
        assert_eq!(&data[offset..offset + 5], shared_reset(base));
        for (callers, target) in [(DECODE_CALLERS, 0x008df4b0), (BUILD_CALLERS, 0x008f88e0)] {
            for caller in callers {
                let call = caller - 5;
                let &(rva, _, offset) = sections
                    .iter()
                    .find(|&&(rva, size, _)| (rva..rva + size).contains(&call))
                    .unwrap();
                let offset = offset + call - rva;
                assert_eq!(data[offset], 0xe8);
                assert_eq!(
                    caller.wrapping_add_signed(i32::from_le_bytes(
                        data[offset + 1..offset + 5].try_into().unwrap()
                    ) as isize),
                    target
                );
            }
        }
        let mut previous_end = 0;
        for patch in PATCHES {
            assert!(patch.rva >= previous_end, "overlapping stage patches");
            let old = patch.original_at(base);
            let &(rva, _, offset) = sections
                .iter()
                .find(|&&(rva, size, _)| (rva..rva + size).contains(&patch.rva))
                .unwrap();
            let offset = offset + patch.rva - rva;
            assert_eq!(
                &data[offset..offset + old.len()],
                old,
                "RVA {:#x}",
                patch.rva
            );
            previous_end = patch.rva + old.len();
        }
    }
}
