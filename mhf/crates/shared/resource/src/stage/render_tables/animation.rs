//! HD render animation commands and their original, contiguous record groups.
//! 11395430 dispatches a command's +12 ID to the per-channel lookup functions.
//! 11395B30..11395EF0 select the first consecutive run with a matching u16 ID.

use crate::{Error, Result, binary::Reader};

use super::{RenderTable, RenderTables};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AnimationChannel {
    DirectionalLight = 1,
    CubeMapLight = 2,
    GodRay = 3,
    HeightFog = 4,
    DistanceFog = 5,
    DepthOfField = 6,
    Bloom = 7,
    ShadowMap = 8,
    ToneMapping = 9,
    GaussianBlur = 10,
    PointLight = 11,
}

impl AnimationChannel {
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::DirectionalLight),
            2 => Some(Self::CubeMapLight),
            3 => Some(Self::GodRay),
            4 => Some(Self::HeightFog),
            5 => Some(Self::DistanceFog),
            6 => Some(Self::DepthOfField),
            7 => Some(Self::Bloom),
            8 => Some(Self::ShadowMap),
            9 => Some(Self::ToneMapping),
            10 => Some(Self::GaussianBlur),
            11 => Some(Self::PointLight),
            _ => None,
        }
    }

    /// The point-light table appears physically second, despite channel ID 11.
    pub const fn count_offset(self) -> usize {
        match self {
            Self::DirectionalLight => 4,
            Self::CubeMapLight => 6,
            Self::GodRay => 8,
            Self::HeightFog => 10,
            Self::DistanceFog => 12,
            Self::DepthOfField => 14,
            Self::Bloom => 16,
            Self::ShadowMap => 18,
            Self::ToneMapping => 20,
            Self::GaussianBlur => 22,
            Self::PointLight => 28,
        }
    }
}

impl RenderTable<'_> {
    pub fn animation_channel(&self) -> Option<AnimationChannel> {
        (1..=11)
            .filter_map(AnimationChannel::from_id)
            .find(|channel| channel.count_offset() == self.count_offset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationCommand {
    /// 11395300 finds the first record whose ID equals the requested sequence.
    pub sequence_id: u16,
    /// 0 terminates traversal; 1..11 select the native animation channel.
    /// Unrecognized values remain raw instead of becoming another channel.
    pub channel: u8,
    pub unknown_03: u8,
    /// At the selected sequence start, bit 0 restarts the completed sequence.
    /// Other bits are retained, without assigning behavior.
    pub sequence_flags: u32,
    /// Native 11395160 waits while this signed counter is nonnegative.
    /// This is an update-step value, not a duration in seconds.
    pub sequence_delay: i32,
    /// Passed as a full DWORD and compared with each target record's u16 ID.
    /// Values above u16::MAX cannot match and must not be truncated.
    pub animation_id: u32,
}

impl AnimationCommand {
    pub const SIZE: usize = 16;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::SIZE {
            return Err(Error::new(
                0,
                "render animation command must contain 16 bytes",
            ));
        }
        let mut reader = Reader::new(bytes);
        Ok(Self {
            sequence_id: reader.read::<u16>()?.value,
            channel: reader.read::<u8>()?.value,
            unknown_03: reader.read::<u8>()?.value,
            sequence_flags: reader.read::<u32>()?.value,
            sequence_delay: reader.read::<i32>()?.value,
            animation_id: reader.read::<u32>()?.value,
        })
    }
}

/// Common prefix of channels 1..10. Channel 11 has a different selection layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyframeHeader {
    pub animation_id: u16,
    pub frame: u16,
    /// Native animation setup consumes bit 0 as the loop flag.
    pub flags: u8,
    pub unknown_05: [u8; 3],
}

impl KeyframeHeader {
    pub const SIZE: usize = 8;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        Ok(Self {
            animation_id: reader.read::<u16>()?.value,
            frame: reader.read::<u16>()?.value,
            flags: reader.read::<u8>()?.value,
            unknown_05: reader.read::<[u8; 3]>()?.value,
        })
    }
}

/// Channel 11 selects point-light animation groups and target light IDs.
/// 11398B80/11398CF0 use +8/+10/+12; +2 is not a keyframe time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointLightSelection {
    pub animation_id: u16,
    pub unknown_02: u16,
    pub unknown_04: u32,
    pub animation_group_id: u16,
    pub first_light_id: u16,
    /// Inclusive bound; 0xffff means to use first_light_id as the bound.
    pub last_light_id: u16,
    pub unknown_0e: u16,
}

impl PointLightSelection {
    pub const SIZE: usize = 16;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::SIZE {
            return Err(Error::new(
                0,
                "point-light animation selection must contain 16 bytes",
            ));
        }
        let mut reader = Reader::new(bytes);
        Ok(Self {
            animation_id: reader.read::<u16>()?.value,
            unknown_02: reader.read::<u16>()?.value,
            unknown_04: reader.read::<u32>()?.value,
            animation_group_id: reader.read::<u16>()?.value,
            first_light_id: reader.read::<u16>()?.value,
            last_light_id: reader.read::<u16>()?.value,
            unknown_0e: reader.read::<u16>()?.value,
        })
    }
}

#[derive(Clone, Debug)]
pub struct AnimationRecords<'a> {
    pub channel: AnimationChannel,
    pub first_record: usize,
    /// Relative to the enclosing RenderTables, independent of the command.
    pub offset: usize,
    pub record_size: usize,
    /// A borrowed range from the original physical table, never a copied group.
    pub records: &'a [u8],
}

impl AnimationRecords<'_> {
    pub fn count(&self) -> usize {
        self.records.len() / self.record_size
    }
}

impl<'a> RenderTables<'a> {
    /// Match the native first contiguous run. A later run with the same ID is
    /// not merged into it, and no sorting or deduplication changes the source.
    pub fn animation_records(
        &self,
        channel: AnimationChannel,
        animation_id: u32,
    ) -> Result<Option<AnimationRecords<'a>>> {
        if animation_id > u16::MAX as u32 {
            return Ok(None);
        }
        let Some(table) = self
            .tables
            .iter()
            .find(|table| table.count_offset == channel.count_offset())
        else {
            return Ok(None);
        };
        let mut first = None;
        let mut count = 0;
        for (index, record) in table.records().enumerate() {
            let id = Reader::with_base(record, table.offset + index * table.record_size)
                .read::<u16>()?
                .value;
            if u32::from(id) == animation_id {
                first.get_or_insert(index);
                count += 1;
            } else if first.is_some() {
                break;
            }
        }
        Ok(first.map(|first_record| {
            let start = first_record * table.record_size;
            let end = start + count * table.record_size;
            AnimationRecords {
                channel,
                first_record,
                offset: table.offset + start,
                record_size: table.record_size,
                records: &table.records[start..end],
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(groups: &[(usize, usize, &[u16])]) -> Vec<u8> {
        let mut source = vec![0; 32];
        source[..2].copy_from_slice(&2u16.to_le_bytes());
        for &(count_offset, stride, ids) in groups {
            source[count_offset..count_offset + 2]
                .copy_from_slice(&(ids.len() as u16).to_le_bytes());
            for &id in ids {
                let mut record = vec![0xcc; stride];
                record[..2].copy_from_slice(&id.to_le_bytes());
                source.extend(record);
            }
        }
        source
    }

    #[test]
    fn lookup_uses_each_native_channel_table_and_only_the_first_contiguous_id_run() {
        let source = tables(&[
            (4, 28, &[7, 7, 8, 7]),
            (28, 16, &[11]),
            (6, 40, &[2]),
            (8, 60, &[3]),
            (10, 36, &[4]),
            (12, 32, &[5]),
            (14, 44, &[6]),
            (16, 28, &[7]),
            (18, 52, &[8]),
            (20, 20, &[9]),
            (22, 28, &[10]),
        ]);
        let file = RenderTables::parse(&source).unwrap();
        let expected = [
            (1, 7, 32, 28, 2),
            (11, 11, 144, 16, 1),
            (2, 2, 160, 40, 1),
            (3, 3, 200, 60, 1),
            (4, 4, 260, 36, 1),
            (5, 5, 296, 32, 1),
            (6, 6, 328, 44, 1),
            (7, 7, 372, 28, 1),
            (8, 8, 400, 52, 1),
            (9, 9, 452, 20, 1),
            (10, 10, 472, 28, 1),
        ];
        for (channel, id, offset, stride, count) in expected {
            let channel = AnimationChannel::from_id(channel).unwrap();
            let group = file.animation_records(channel, id).unwrap().unwrap();
            assert_eq!(
                (group.offset, group.record_size, group.count()),
                (offset, stride, count)
            );
            assert_eq!(group.records, &source[offset..offset + stride * count]);
            assert_eq!(group.records.as_ptr(), source[offset..].as_ptr());
        }
        let group = file
            .animation_records(AnimationChannel::DirectionalLight, 8)
            .unwrap()
            .unwrap();
        assert_eq!(
            (group.first_record, group.offset, group.count()),
            (2, 88, 1)
        );
        for id in [99, 0x1_0007, u32::MAX] {
            assert!(
                file.animation_records(AnimationChannel::DirectionalLight, id)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(AnimationChannel::from_id(0), None);
        assert_eq!(AnimationChannel::from_id(12), None);
    }

    #[test]
    fn command_fields_preserve_signed_delay_full_ids_and_unknown_channel_values() {
        let mut bytes = vec![0x34, 0x12, 0xff, 0xab];
        bytes.extend_from_slice(&0x8765_4321u32.to_le_bytes());
        bytes.extend_from_slice(&(-7i32).to_le_bytes());
        bytes.extend_from_slice(&0x1234_0007u32.to_le_bytes());
        let command = AnimationCommand::parse(&bytes).unwrap();
        assert_eq!(command.sequence_id, 0x1234);
        assert_eq!((command.channel, command.unknown_03), (0xff, 0xab));
        assert_eq!(command.sequence_flags, 0x8765_4321);
        assert_eq!(command.sequence_delay, -7);
        assert_eq!(command.animation_id, 0x1234_0007);
        for length in 0..AnimationCommand::SIZE {
            assert!(AnimationCommand::parse(&bytes[..length]).is_err());
        }
        bytes.push(0);
        assert!(AnimationCommand::parse(&bytes).is_err());
    }

    #[test]
    fn point_light_selection_does_not_reinterpret_unknown_header_as_frame_or_flags() {
        let bytes = [0x0009u16, 0xff80, 0xbeef, 0x7fc0, 81, 7, 0xffff, 0xabcd]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let selection = PointLightSelection::parse(&bytes).unwrap();
        assert_eq!(selection.animation_id, 9);
        assert_eq!(selection.unknown_02, 0xff80);
        assert_eq!(selection.unknown_04, 0x7fc0_beef);
        assert_eq!(selection.animation_group_id, 81);
        assert_eq!(
            (selection.first_light_id, selection.last_light_id),
            (7, 0xffff)
        );
        assert_eq!(selection.unknown_0e, 0xabcd);
        for length in 0..PointLightSelection::SIZE {
            assert!(PointLightSelection::parse(&bytes[..length]).is_err());
        }
        let header = KeyframeHeader::parse(&[7, 0, 0xff, 0xff, 0x81, 0xa5, 0xfe, 0xcc]).unwrap();
        assert_eq!(
            (header.animation_id, header.frame, header.flags),
            (7, 65535, 0x81)
        );
        assert_eq!(header.unknown_05, [0xa5, 0xfe, 0xcc]);
        for length in 0..KeyframeHeader::SIZE {
            assert!(KeyframeHeader::parse(&bytes[..length]).is_err());
        }
    }
}
