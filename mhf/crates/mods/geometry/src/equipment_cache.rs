//! Raw equipment files are cached before ECD decoding and FMOD construction.

// Weapon slots first, followed by six asynchronous and six synchronous parts.
pub(crate) const CACHE_RVAS: [usize; 14] = [
    0x01d31690, 0x01e157d0, 0x01c71690, 0x01c91690, 0x01cb1690, 0x01cd1690, 0x01cf1690, 0x01d11690,
    0x01d557d0, 0x01d757d0, 0x01d957d0, 0x01db57d0, 0x01dd57d0, 0x01df57d0,
];
pub(crate) const ORIGINAL_CAPACITY: usize = 0x20000;

/// File loaders append a path and NUL; queued mode 7 first stores a 76-byte job.
pub(crate) fn capacity(file_size: usize, path_length: usize) -> Result<usize, String> {
    if file_size == 0 {
        return Err("equipment resource is empty or missing".into());
    }
    let required = file_size
        .checked_add(path_length)
        .and_then(|size| size.checked_add(1))
        .ok_or("equipment cache size overflow")?
        .max(ORIGINAL_CAPACITY);
    crate::mesh::byte_size(required, 1)?;
    Ok(required
        .checked_next_power_of_two()
        .filter(|&size| size <= i32::MAX as usize)
        .unwrap_or(required))
}

#[derive(Default)]
pub(crate) struct Buffer {
    // Grow geometrically and retain earlier addresses until native callers stop.
    // Queued I/O may still hold an address even after its load has completed.
    allocations: Vec<Vec<u8>>,
}

impl Buffer {
    pub(crate) fn prepare(&mut self, size: usize) -> Result<*mut u8, String> {
        if self
            .allocations
            .last()
            .is_none_or(|bytes| bytes.len() < size)
        {
            self.allocations.try_reserve(1).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(size).map_err(|e| e.to_string())?;
            bytes.resize(size, 0);
            self.allocations.push(bytes);
        }
        Ok(self.allocations.last_mut().unwrap().as_mut_ptr())
    }

    pub(crate) fn invalidate(&mut self) {
        for bytes in &mut self.allocations {
            bytes[..INVALID_RESOURCE.len()].copy_from_slice(&INVALID_RESOURCE);
        }
    }

    #[cfg(all(windows, target_arch = "x86"))]
    pub(crate) fn retain_for_native(&mut self) {
        std::mem::forget(std::mem::take(&mut self.allocations));
    }
}

// ECD v4 with an empty filename and a deliberately incorrect filename checksum.
// 1158F510 rejects it before decryption, so failed loads skip equipment construction.
pub(crate) const INVALID_RESOURCE: [u8; 17] = [
    b'e', b'c', b'd', 0x1a, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

pub(crate) struct CacheReference {
    pub rva: usize,
    pub original: &'static [u8],
    pub operand_offset: usize,
    pub cache: usize,
    pub offset: usize,
}

impl CacheReference {
    pub(crate) fn bytes(&self, address: usize) -> [u8; 7] {
        let mut bytes = [0; 7];
        bytes[..self.original.len()].copy_from_slice(self.original);
        bytes[self.operand_offset..self.operand_offset + 4]
            .copy_from_slice(&((address + self.offset) as u32).to_le_bytes());
        bytes
    }
}

// Only consumers move. Producers keep the original address as an I/O marker;
// queued requests are rebound when dispatched, never while being enqueued.
pub(crate) const CACHE_REFERENCES: &[CacheReference] = &[
    CacheReference {
        rva: 0x8e11b7,
        original: &[0xbe, 0x90, 0x16, 0xc7, 0x11],
        operand_offset: 1,
        cache: 2,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e11c6,
        original: &[0xbe, 0x90, 0x16, 0xc9, 0x11],
        operand_offset: 1,
        cache: 3,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e11d5,
        original: &[0xbe, 0x90, 0x16, 0xcb, 0x11],
        operand_offset: 1,
        cache: 4,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e11dc,
        original: &[0xbe, 0x90, 0x16, 0xcd, 0x11],
        operand_offset: 1,
        cache: 5,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e11e3,
        original: &[0xbe, 0x90, 0x16, 0xcf, 0x11],
        operand_offset: 1,
        cache: 6,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e11ea,
        original: &[0xbe, 0x90, 0x16, 0xd1, 0x11],
        operand_offset: 1,
        cache: 7,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e1be0,
        original: &[0xbe, 0xd0, 0x57, 0xd5, 0x11],
        operand_offset: 1,
        cache: 8,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e1bef,
        original: &[0xbe, 0xd0, 0x57, 0xd7, 0x11],
        operand_offset: 1,
        cache: 9,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e1bfe,
        original: &[0xbe, 0xd0, 0x57, 0xd9, 0x11],
        operand_offset: 1,
        cache: 10,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e1c05,
        original: &[0xbe, 0xd0, 0x57, 0xdb, 0x11],
        operand_offset: 1,
        cache: 11,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e1c0c,
        original: &[0xbe, 0xd0, 0x57, 0xdd, 0x11],
        operand_offset: 1,
        cache: 12,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e1c13,
        original: &[0xbe, 0xd0, 0x57, 0xdf, 0x11],
        operand_offset: 1,
        cache: 13,
        offset: 0,
    },
    CacheReference {
        rva: 0x8e12a6,
        original: &[0xbe, 0x90, 0x16, 0xd3, 0x11],
        operand_offset: 1,
        cache: 0,
        offset: 0x0,
    },
    CacheReference {
        rva: 0x8e12bb,
        original: &[0xa1, 0x94, 0x16, 0xd3, 0x11],
        operand_offset: 1,
        cache: 0,
        offset: 0x4,
    },
    CacheReference {
        rva: 0x8e12c6,
        original: &[0x38, 0x88, 0x90, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x0,
    },
    CacheReference {
        rva: 0x8e12ce,
        original: &[0x38, 0x90, 0x91, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x1,
    },
    CacheReference {
        rva: 0x8e12d6,
        original: &[0x38, 0x98, 0x92, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x2,
    },
    CacheReference {
        rva: 0x8e12de,
        original: &[0x80, 0xb8, 0x93, 0x16, 0xd3, 0x11, 0x1a],
        operand_offset: 2,
        cache: 0,
        offset: 0x3,
    },
    CacheReference {
        rva: 0x8e12e7,
        original: &[0x8b, 0xb0, 0x9c, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0xc,
    },
    CacheReference {
        rva: 0x8e12ef,
        original: &[0x8b, 0x35, 0x98, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x8,
    },
    CacheReference {
        rva: 0x8e12f5,
        original: &[0xa1, 0x9c, 0x16, 0xd3, 0x11],
        operand_offset: 1,
        cache: 0,
        offset: 0xc,
    },
    CacheReference {
        rva: 0x8e12fa,
        original: &[0x38, 0x88, 0x90, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x0,
    },
    CacheReference {
        rva: 0x8e1302,
        original: &[0x38, 0x90, 0x91, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x1,
    },
    CacheReference {
        rva: 0x8e130a,
        original: &[0x38, 0x98, 0x92, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x2,
    },
    CacheReference {
        rva: 0x8e1312,
        original: &[0x80, 0xb8, 0x93, 0x16, 0xd3, 0x11, 0x1a],
        operand_offset: 2,
        cache: 0,
        offset: 0x3,
    },
    CacheReference {
        rva: 0x8e131b,
        original: &[0x8b, 0x80, 0x9c, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0xc,
    },
    CacheReference {
        rva: 0x8e1323,
        original: &[0xa1, 0xa0, 0x16, 0xd3, 0x11],
        operand_offset: 1,
        cache: 0,
        offset: 0x10,
    },
    CacheReference {
        rva: 0x8e133c,
        original: &[0xbe, 0x90, 0x16, 0xd3, 0x11],
        operand_offset: 1,
        cache: 0,
        offset: 0x0,
    },
    CacheReference {
        rva: 0x8e134d,
        original: &[0x8b, 0x15, 0xa4, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x14,
    },
    CacheReference {
        rva: 0x8e136d,
        original: &[0x8d, 0x92, 0x90, 0x16, 0xd3, 0x11],
        operand_offset: 2,
        cache: 0,
        offset: 0x0,
    },
    CacheReference {
        rva: 0x8e1cbc,
        original: &[0xbe, 0xd0, 0x57, 0xe1, 0x11],
        operand_offset: 1,
        cache: 1,
        offset: 0x0,
    },
    CacheReference {
        rva: 0x8e1d15,
        original: &[0x8b, 0x15, 0xe4, 0x57, 0xe1, 0x11],
        operand_offset: 2,
        cache: 1,
        offset: 0x14,
    },
    CacheReference {
        rva: 0x8e1d1f,
        original: &[0x8d, 0x92, 0xd0, 0x57, 0xe1, 0x11],
        operand_offset: 2,
        cache: 1,
        offset: 0x0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_at_the_old_boundary_include_the_filename_terminator() {
        let path = b"dat\\weapon\\we001.bin\0";
        for size in [
            ORIGINAL_CAPACITY - 1,
            ORIGINAL_CAPACITY,
            130 * 1024,
            1024 * 1024,
        ] {
            let mut cache = Buffer::default();
            let length = capacity(size, path.len() - 1).unwrap();
            let pointer = cache.prepare(length).unwrap();
            let bytes = unsafe { std::slice::from_raw_parts_mut(pointer, length) };
            bytes[..size].fill(0x5a);
            bytes[size..size + path.len()].copy_from_slice(path);
            assert_eq!(&bytes[size..size + path.len()], path);
            assert!(length > ORIGINAL_CAPACITY);
        }
    }

    #[test]
    fn growing_a_cache_retains_previous_native_addresses() {
        let mut cache = Buffer::default();
        let old = cache.prepare(ORIGINAL_CAPACITY).unwrap();
        unsafe { old.write(0x5a) };
        let new = cache.prepare(ORIGINAL_CAPACITY * 2).unwrap();
        assert_ne!(old, new);
        assert_eq!(unsafe { old.read() }, 0x5a);
        assert_eq!(cache.prepare(ORIGINAL_CAPACITY).unwrap(), new);
        cache.invalidate();
        for pointer in [old, new] {
            assert_eq!(
                unsafe { std::slice::from_raw_parts(pointer, 17) },
                INVALID_RESOURCE
            );
        }
    }

    #[test]
    fn invalid_file_sizes_do_not_wrap_allocations() {
        assert!(capacity(0, 20).is_err());
        assert!(capacity(usize::MAX, 20).is_err());
        assert!(capacity(i32::MAX as usize, 20).is_err());
    }
}
