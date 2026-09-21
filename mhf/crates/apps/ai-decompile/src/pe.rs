//! Disk-backed PE32 VA reader. Never maps executable code or invents BSS bytes.
use mhf_monster::ai::{Error, Result, decompile::Memory};

struct Section {
    rva: u32,
    size: u32,
    offset: usize,
    raw_size: usize,
}

pub struct Image {
    data: Vec<u8>,
    base: u32,
    headers: usize,
    sections: Vec<Section>,
}

impl Image {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let bytes = |offset: usize, length: usize| data.get(offset..offset.checked_add(length)?);
        let word = |offset| -> Result<u16> {
            Ok(u16::from_le_bytes(
                bytes(offset, 2)
                    .ok_or_else(|| Error::new("truncated PE header"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        let dword = |offset| -> Result<u32> {
            Ok(u32::from_le_bytes(
                bytes(offset, 4)
                    .ok_or_else(|| Error::new("truncated PE header"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        if bytes(0, 2) != Some(b"MZ") {
            return Err(Error::new("not a PE file (missing MZ)"));
        }
        let pe = dword(0x3c)? as usize;
        let optional = pe
            .checked_add(24)
            .ok_or_else(|| Error::new("PE offset overflow"))?;
        if bytes(pe, 4) != Some(b"PE\0\0") || word(pe + 4)? != 0x14c {
            return Err(Error::new("expected an i386 PE image"));
        }
        let optional_size = usize::from(word(pe + 20)?);
        if optional_size < 96
            || bytes(optional, optional_size).is_none()
            || word(optional)? != 0x10b
        {
            return Err(Error::new("expected a complete PE32 optional header"));
        }
        let base = dword(optional + 28)?;
        let headers = dword(optional + 60)? as usize;
        let table = optional
            .checked_add(optional_size)
            .ok_or_else(|| Error::new("PE offset overflow"))?;
        let count = usize::from(word(pe + 6)?);
        if count == 0
            || count > 96
            || bytes(table, count * 40).is_none()
            || headers > data.len()
            || headers < table + count * 40
        {
            return Err(Error::new("invalid PE section/header extent"));
        }
        let mut sections = Vec::new();
        for i in 0..count {
            let entry = table + i * 40;
            let rva = dword(entry + 12)?;
            let raw_size = dword(entry + 16)?;
            let offset = dword(entry + 20)? as usize;
            let size = dword(entry + 8)?.max(raw_size);
            let end = rva
                .checked_add(size)
                .ok_or_else(|| Error::new("PE section VA overflow"))?;
            base.checked_add(end)
                .ok_or_else(|| Error::new("PE image VA overflow"))?;
            if (rva as usize) < headers || bytes(offset, raw_size as usize).is_none() {
                return Err(Error::new("invalid PE section raw extent"));
            }
            sections.push(Section {
                rva,
                size,
                offset,
                raw_size: raw_size as usize,
            });
        }
        sections.sort_by_key(|s| s.rva);
        if sections.windows(2).any(|s| s[0].rva + s[0].size > s[1].rva) {
            return Err(Error::new("overlapping PE sections"));
        }
        Ok(Self {
            data,
            base,
            headers,
            sections,
        })
    }
}

impl Memory for Image {
    fn bytes(&self, address: u32, length: usize) -> Result<Vec<u8>> {
        let unavailable = || {
            Error::new(format!(
                "VA 0x{address:08X} ({length} bytes) is not backed by file data; runtime memory cannot be recovered offline"
            ))
        };
        let rva = address.checked_sub(self.base).ok_or_else(unavailable)?;
        let length32 = u32::try_from(length).map_err(|_| unavailable())?;
        address.checked_add(length32).ok_or_else(unavailable)?;
        let end = rva.checked_add(length32).ok_or_else(unavailable)?;
        if (rva as usize) < self.headers && end as usize <= self.headers {
            return Ok(self.data[rva as usize..end as usize].to_vec());
        }
        let section = self
            .sections
            .iter()
            .find(|s| rva >= s.rva && rva < s.rva + s.size)
            .ok_or_else(unavailable)?;
        let offset = (rva - section.rva) as usize;
        if end > section.rva + section.size
            || offset
                .checked_add(length)
                .is_none_or(|end| end > section.raw_size)
        {
            return Err(unavailable());
        }
        Ok(self.data[section.offset + offset..section.offset + offset + length].to_vec())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x300];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&0x14cu16.to_le_bytes());
        b[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&0xe0u16.to_le_bytes());
        b[0x98..0x9a].copy_from_slice(&0x10bu16.to_le_bytes());
        b[0xb4..0xb8].copy_from_slice(&0x10000000u32.to_le_bytes());
        b[0xd4..0xd8].copy_from_slice(&0x200u32.to_le_bytes());
        for (o, v) in [
            (0x180, 0x200u32),
            (0x184, 0x1000),
            (0x188, 0x100),
            (0x18c, 0x200),
        ] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[0x200] = 0x42;
        b
    }

    #[test]
    fn reads_raw_sections_but_never_bss_or_gaps() {
        let image = Image::parse(fixture()).unwrap();
        assert_eq!(image.bytes(0x10001000, 1).unwrap(), [0x42]);
        assert!(image.bytes(0x10001100, 1).is_err());
        assert!(image.bytes(0x100010ff, 2).is_err());
        assert!(image.bytes(0x10000400, 4).is_err());
        assert!(image.bytes(0, 4).is_err());
        assert!(image.bytes(u32::MAX, 4).is_err());
    }

    #[test]
    fn rejects_truncated_or_non_pe32_files() {
        let original = fixture();
        for n in 0..original.len() {
            assert!(Image::parse(original[..n].to_vec()).is_err(), "length {n}");
        }
        let mut b = original;
        b[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        assert!(Image::parse(b).is_err());
    }
}
