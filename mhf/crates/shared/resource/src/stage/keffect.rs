//! KEFFECT key records used by stage objects and native effect previews.
//!
//! 105EECB0 checks seven signature bytes. 105EECF0 searches consecutive 112-byte
//! records by the first two DWORDs and their float frame at +8; 105EED60 finds
//! the maximum frame. 105EEED0 consumes the render mode at record +64.

use crate::{Error, Result};

pub const MAGIC: &[u8; 7] = b"KEFFECT";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KEffectRecord {
    pub offset: usize,
    pub kind: u32,
    pub target_id: u32,
    pub frame_bits: u32,
    /// Thirteen original words between the frame and render mode. Their mixed
    /// transform/color semantics depend on kind; preserve them without guessing.
    pub parameters_0c: [u32; 13],
    pub render_mode: u32,
    pub unknown_44: [u32; 11],
}

impl KEffectRecord {
    pub const SIZE: usize = 112;

    pub fn frame(&self) -> f32 {
        f32::from_bits(self.frame_bits)
    }
}

#[derive(Clone, Debug)]
pub struct KEffect<'a> {
    /// The native magic check does not inspect byte 7.
    pub unknown_07: u8,
    /// 105EECF0/105EED60 use +12 and records at +16, without consuming +8.
    /// Preserve it as a scalar; no version, length or reference role is known.
    pub unknown_08: u32,
    pub count: u32,
    pub records: Vec<KEffectRecord>,
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> KEffect<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..16)
            .ok_or_else(|| Error::new(0, "truncated KEFFECT header"))?;
        if !header.starts_with(MAGIC) {
            return Err(Error::new(0, "expected KEFFECT signature"));
        }
        let word = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap());
        let count = word(&header[12..16]);
        let end = (count as usize)
            .checked_mul(KEffectRecord::SIZE)
            .and_then(|size| size.checked_add(16))
            .ok_or_else(|| Error::new(12, "KEFFECT record count overflow"))?;
        let records = source
            .get(16..end)
            .ok_or_else(|| Error::new(12, "KEFFECT records exceed resource"))?
            .as_chunks::<{ KEffectRecord::SIZE }>()
            .0
            .iter()
            .enumerate()
            .map(|(index, record)| KEffectRecord {
                offset: 16 + index * KEffectRecord::SIZE,
                kind: word(&record[..4]),
                target_id: word(&record[4..8]),
                frame_bits: word(&record[8..12]),
                parameters_0c: std::array::from_fn(|index| {
                    word(&record[12 + index * 4..16 + index * 4])
                }),
                render_mode: word(&record[64..68]),
                unknown_44: std::array::from_fn(|index| {
                    word(&record[68 + index * 4..72 + index * 4])
                }),
            })
            .collect();
        Ok(Self {
            unknown_07: header[7],
            unknown_08: word(&header[8..12]),
            count,
            records,
            trailing: &source[end..],
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = b"KEFFECT\x7f".to_vec();
        bytes.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        for (kind, target, frame) in [
            (1u32, 7u32, 0x8000_0000u32),
            (1, 7, 10f32.to_bits()),
            (3, 0, 0x7fc0_1234),
        ] {
            let mut words = [0xffff_ffffu32; 28];
            words[0] = kind;
            words[1] = target;
            words[2] = frame;
            words[16] = 6;
            for word in words {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn keeps_order_float_bits_render_mode_and_unknown_header() {
        let bytes = fixture();
        let file = KEffect::parse(&bytes).unwrap();
        assert_eq!(file.unknown_07, 0x7f);
        assert_eq!(file.unknown_08, 0x1234_5678);
        assert_eq!(file.count, 3);
        assert_eq!(file.records[0].frame().to_bits(), 0x8000_0000);
        assert_eq!(file.records[1].frame(), 10.0);
        assert_eq!(file.records[2].frame_bits, 0x7fc0_1234);
        assert_eq!(file.records[1].offset, 128);
        assert_eq!(file.records[2].render_mode, 6);
        assert_eq!(file.records[2].unknown_44, [u32::MAX; 11]);
        assert_eq!(file.as_bytes(), bytes);
    }

    #[test]
    fn checks_declared_records_and_retains_trailing_bytes() {
        let bytes = fixture();
        for length in 0..bytes.len() {
            assert!(KEffect::parse(&bytes[..length]).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(KEffect::parse(&invalid).is_err());
        let mut tailed = bytes;
        tailed.extend_from_slice(&[0xde, 0xad]);
        assert_eq!(KEffect::parse(&tailed).unwrap().trailing, [0xde, 0xad]);
    }
}
