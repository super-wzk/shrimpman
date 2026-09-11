//! HD light-manager input read by native 10021670. Offsets refer to file bytes.
//!
//! The observed 3.2/3.3/3.4 layouts retain all original words. Record families
//! are named from native RTTI; unconfirmed individual parameters remain words.

use crate::{Error, Result};

const V32: u32 = 3.2f32.to_bits();
const V33: u32 = 3.3f32.to_bits();
const V34: u32 = 3.4f32.to_bits();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub offset: usize,
    /// Original little-endian words, including float payloads and signed IDs.
    pub words: Vec<u32>,
}

impl Record {
    pub fn byte_len(&self) -> usize {
        self.words.len() * 4
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightCollision {
    pub header: Record,
    /// Native signed count at header +64; nonpositive counts consume no IDs.
    pub members: Record,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightAnimationChannel {
    /// 3.2 stores three words; 3.3/3.4 store two. The final word is key count.
    pub header: Record,
    pub keys: Vec<Record>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightAnimation {
    /// Seven words; word 5 is the number of channel records that follow.
    pub header: Record,
    pub channels: Vec<LightAnimationChannel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToneMapping {
    pub offset: usize,
    /// Native signed byte count. Entries immediately follow without padding.
    pub count: i8,
    pub points: Vec<Record>,
    pub flags: Record,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostProcessing {
    /// Also carries the light-manager vector preceding Godray's parameters.
    pub god_rays: Record,
    pub height_fog: Record,
    pub depth_fog: Record,
    /// 44 bytes in 3.2/3.3; 52 bytes in 3.4.
    pub depth_of_field: Record,
    pub bloom: Record,
    /// Light-manager shadow parameters followed by native CSM settings.
    pub shadows: Record,
    pub ssao: Record,
    pub gaussian_blur: Record,
    pub tone_mapping: ToneMapping,
}

#[derive(Clone, Debug)]
pub struct Lighting<'a> {
    pub version_bits: u32,
    /// PointLight, CubeMapLight, LightGroup, LightColision, PointLightAnimGroup.
    /// Native 10021670 treats each byte separately as signed, including +8.
    pub counts: [i8; 5],
    /// Observed debug-fill bytes (FE/CC), not part of the animation-group count.
    pub reserved_09: [u8; 3],
    pub point_lights: Vec<Record>,
    /// Two groups of three six-word DirectionalLight records.
    pub directional_lights: [Vec<Record>; 2],
    pub cube_map_lights: Vec<Record>,
    pub light_groups: Vec<Record>,
    pub light_collisions: Vec<LightCollision>,
    pub light_animations: Vec<LightAnimation>,
    pub post_process: PostProcessing,
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> Lighting<'a> {
    /// Recognize only versions whose complete native record layout is known.
    pub fn has_known_version(source: &[u8]) -> bool {
        source.get(..4).is_some_and(|bytes| {
            matches!(
                u32::from_le_bytes(bytes.try_into().unwrap()),
                V32 | V33 | V34
            )
        })
    }

    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor { source, offset: 0 };
        let header = cursor.take(12)?;
        let version_bits = u32::from_le_bytes(header[..4].try_into().unwrap());
        if !matches!(version_bits, V32 | V33 | V34) {
            return Err(Error::new(
                0,
                "unsupported HD lighting version; verified layouts are 3.2, 3.3 and 3.4",
            ));
        }
        let counts = std::array::from_fn(|index| header[4 + index] as i8);
        let reserved_09 = header[9..12].try_into().unwrap();
        let point_lights = cursor.records(nonnegative(counts[0]), 26)?;
        // >=2.3: both native DirectionalLight groups consume 18 words each.
        let directional_lights = [cursor.records(3, 6)?, cursor.records(3, 6)?];
        // >=2.7: embedded cubemap filenames were replaced by ten-word records.
        let cube_map_lights = cursor.records(nonnegative(counts[1]), 10)?;
        let light_groups = cursor.records(nonnegative(counts[2]), 7)?;
        let mut light_collisions = Vec::new();
        cursor.check_records(nonnegative(counts[3]), 17)?;
        for _ in 0..nonnegative(counts[3]) {
            let header = cursor.words(17)?;
            let members = cursor.words(nonnegative_word(header.words[16]))?;
            light_collisions.push(LightCollision { header, members });
        }
        let mut light_animations = Vec::new();
        cursor.check_records(nonnegative(counts[4]), 7)?;
        for _ in 0..nonnegative(counts[4]) {
            let header = cursor.words(7)?;
            // The decompiler types this count as float, but native decrements
            // its stored DWORD, not its floating-point value.
            let channel_count = header.words[5] as usize;
            let channel_words = if version_bits == V32 { 3 } else { 2 };
            cursor.check_records(channel_count, channel_words)?;
            let mut channels = Vec::with_capacity(channel_count);
            for _ in 0..channel_count {
                let header = cursor.words(channel_words)?;
                let key_count = nonnegative_word(header.words[channel_words - 1]);
                let keys = cursor.records(key_count, 4)?;
                channels.push(LightAnimationChannel { header, keys });
            }
            light_animations.push(LightAnimation { header, channels });
        }
        let god_rays = cursor.words(18)?;
        let height_fog = cursor.words(9)?;
        let depth_fog = cursor.words(9)?;
        let depth_of_field = cursor.words(if version_bits == V34 { 13 } else { 11 })?;
        let bloom = cursor.words(7)?;
        let shadows = cursor.words(13)?;
        let ssao = cursor.words(6)?;
        let gaussian_blur = cursor.words(7)?;
        let offset = cursor.offset;
        let count = cursor.take(1)?[0] as i8;
        let points = cursor.records(nonnegative(count), 2)?;
        let flags = cursor.words(1)?;
        let post_process = PostProcessing {
            god_rays,
            height_fog,
            depth_fog,
            depth_of_field,
            bloom,
            shadows,
            ssao,
            gaussian_blur,
            tone_mapping: ToneMapping {
                offset,
                count,
                points,
                flags,
            },
        };
        Ok(Self {
            version_bits,
            counts,
            reserved_09,
            point_lights,
            directional_lights,
            cube_map_lights,
            light_groups,
            light_collisions,
            light_animations,
            post_process,
            trailing: &source[cursor.offset..],
            source,
        })
    }

    /// Content-based identification must account for the entire known layout.
    pub fn probe(source: &'a [u8]) -> Result<Self> {
        let file = Self::parse(source)?;
        if !file.trailing.is_empty() {
            return Err(Error::new(
                source.len() - file.trailing.len(),
                "bytes remain after the HD lighting layout",
            ));
        }
        Ok(file)
    }

    pub fn version(&self) -> f32 {
        f32::from_bits(self.version_bits)
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

fn nonnegative(count: i8) -> usize {
    count.max(0) as usize
}

fn nonnegative_word(count: u32) -> usize {
    (count as i32).max(0) as usize
}

struct Cursor<'a> {
    source: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(size)
            .ok_or_else(|| Error::new(self.offset, "lighting record length overflow"))?;
        let bytes = self
            .source
            .get(self.offset..end)
            .ok_or_else(|| Error::new(self.offset, "truncated HD lighting record"))?;
        self.offset = end;
        Ok(bytes)
    }

    fn words(&mut self, count: usize) -> Result<Record> {
        let offset = self.offset;
        let size = count
            .checked_mul(4)
            .ok_or_else(|| Error::new(offset, "lighting word count overflow"))?;
        let words = self
            .take(size)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect();
        Ok(Record { offset, words })
    }

    fn check_records(&self, count: usize, words: usize) -> Result<()> {
        let size = count
            .checked_mul(words)
            .and_then(|size| size.checked_mul(4))
            .ok_or_else(|| Error::new(self.offset, "lighting record count overflow"))?;
        if size > self.source.len() - self.offset {
            return Err(Error::new(
                self.offset,
                "lighting record count exceeds remaining bytes",
            ));
        }
        Ok(())
    }

    fn records(&mut self, count: usize, words: usize) -> Result<Vec<Record>> {
        self.check_records(count, words)?;
        (0..count).map(|_| self.words(words)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_tail(version: u32) -> Vec<u8> {
        let mut bytes = vec![0; 72 + 72 + if version == V34 { 52 } else { 44 } + 28 + 52 + 24 + 28];
        bytes.push(2);
        // Tone mapping's unaligned points must not be rounded to a DWORD.
        for value in [0x8000_0000u32, 0x7fc0_1234, 2, 3, 0x8765_4321] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn fixture(version: u32) -> Vec<u8> {
        let mut bytes = version.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[1, 1, 1, 1, 1, 0xfe, 0xfe, 0xfe]);
        bytes.extend_from_slice(&[0; 104 + 72 + 72 + 40 + 28]);
        let mut collision = [0; 68];
        collision[64..68].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&collision);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut animation = [0; 28];
        animation[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&animation);
        if version == V32 {
            bytes.extend_from_slice(&19u32.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&fixed_tail(version));
        bytes
    }

    #[test]
    fn versions_preserve_variable_records_and_unaligned_tone_mapping() {
        for version in [V32, V33, V34] {
            let bytes = fixture(version);
            let file = Lighting::probe(&bytes).unwrap();
            assert_eq!(file.counts, [1; 5]);
            assert_eq!(file.reserved_09, [0xfe; 3]);
            assert_eq!(file.light_collisions[0].members.words, [7, u32::MAX]);
            assert_eq!(
                file.light_animations[0].channels[0].header.words.len(),
                if version == V32 { 3 } else { 2 }
            );
            assert_eq!(file.light_animations[0].channels[0].keys.len(), 1);
            let tone = &file.post_process.tone_mapping;
            assert_eq!(tone.points[0].offset, tone.offset + 1);
            assert_eq!(tone.points[0].words, [0x8000_0000, 0x7fc0_1234]);
            assert_eq!(tone.flags.words, [0x8765_4321]);
            assert_eq!(file.as_bytes(), bytes);
        }
    }

    #[test]
    fn rejects_truncation_and_oversized_variable_counts_without_repair() {
        let bytes = fixture(V34);
        for length in 0..bytes.len() {
            assert!(
                Lighting::parse(&bytes[..length]).is_err(),
                "length={length}"
            );
        }
        let mut oversized = bytes.clone();
        let channel_count = 12 + 104 + 144 + 40 + 28 + 68 + 8 + 20;
        oversized[channel_count..channel_count + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Lighting::parse(&oversized).is_err());
        assert_eq!(Lighting::probe(&bytes).unwrap().as_bytes(), bytes);
    }

    #[test]
    fn separates_explicit_parsing_from_strict_identification() {
        let mut bytes = fixture(V33);
        let source_len = bytes.len();
        bytes.extend_from_slice(b"tail");
        let file = Lighting::parse(&bytes).unwrap();
        assert_eq!(file.trailing, b"tail");
        assert_eq!(Lighting::probe(&bytes).unwrap_err().offset, source_len);
        bytes[..4].copy_from_slice(&3.5f32.to_bits().to_le_bytes());
        assert!(!Lighting::has_known_version(&bytes));
        assert!(Lighting::parse(&bytes).is_err());
    }
}
