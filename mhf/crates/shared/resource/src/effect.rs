//! The four distinct equipment-effect tables used by the ZZ client.
//!
//! These are file records, not relocated DAT pointers or running effect objects.
//! Unknown bytes and float bit patterns survive parsing and serialization.

use crate::{Error, Result};

fn record<const N: usize>(bytes: &[u8]) -> Result<&[u8; N]> {
    if bytes.len() != N {
        return Err(Error::new(
            bytes.len().min(N),
            format!("expected a {N}-byte effect record"),
        ));
    }
    Ok(bytes.try_into().expect("checked record size"))
}

fn table<const N: usize, T>(bytes: &[u8], parse: fn(&[u8]) -> Result<T>) -> Result<Vec<T>> {
    if !bytes.len().is_multiple_of(N) {
        return Err(Error::new(
            bytes.len() / N * N,
            format!("partial {N}-byte effect table record"),
        ));
    }
    bytes
        .as_chunks::<N>()
        .0
        .iter()
        .map(|record| parse(record))
        .collect()
}

fn ids(bytes: &[u8]) -> [u16; 8] {
    std::array::from_fn(|index| {
        u16::from_le_bytes(
            bytes[index * 2..index * 2 + 2]
                .try_into()
                .expect("fixed record"),
        )
    })
}

fn write_ids(bytes: &mut [u8], offset: usize, ids: &[u16; 8]) {
    for (index, id) in ids.iter().enumerate() {
        bytes[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&id.to_le_bytes());
    }
}

fn active_ids(ids: &[u16; 8]) -> &[u16] {
    &ids[..ids.iter().position(|&id| id == 0).unwrap_or(ids.len())]
}

/// DAT pointer-table entry 160. Its part code uses this table's native namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentGroup {
    pub part_code: u16,
    /// Zero terminates the list; later slots are still preserved in the file.
    pub definition_ids: [u16; 8],
}

impl AttachmentGroup {
    pub const DAT_INDEX: usize = 160;
    pub const SIZE: usize = 18;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bytes = record::<18>(bytes)?;
        Ok(Self {
            part_code: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
            definition_ids: ids(&bytes[2..]),
        })
    }

    pub fn parse_table(bytes: &[u8]) -> Result<Vec<Self>> {
        table::<18, _>(bytes, Self::parse)
    }

    pub fn active_definition_ids(&self) -> &[u16] {
        active_ids(&self.definition_ids)
    }

    pub fn to_bytes(&self) -> [u8; 18] {
        let mut bytes = [0; 18];
        bytes[..2].copy_from_slice(&self.part_code.to_le_bytes());
        write_ids(&mut bytes, 2, &self.definition_ids);
        bytes
    }
}

/// DAT entry 161. `10BB2AB0` applies this local position to the selected node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentDefinition {
    /// IEEE-754 storage, including NaNs and signed zero, in native XYZ order.
    pub local_position_bits: [u32; 3],
    pub unknown_0c: u8,
    pub node_index: u8,
    /// The original mode byte; no guessed mode enumeration is imposed.
    pub attachment_mode: u8,
    pub unknown_0f: [u8; 113],
}

impl AttachmentDefinition {
    pub const DAT_INDEX: usize = 161;
    pub const SIZE: usize = 128;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bytes = record::<128>(bytes)?;
        Ok(Self {
            local_position_bits: std::array::from_fn(|axis| {
                u32::from_le_bytes(bytes[axis * 4..axis * 4 + 4].try_into().unwrap())
            }),
            unknown_0c: bytes[12],
            node_index: bytes[13],
            attachment_mode: bytes[14],
            unknown_0f: bytes[15..].try_into().unwrap(),
        })
    }

    pub fn parse_table(bytes: &[u8]) -> Result<Vec<Self>> {
        table::<128, _>(bytes, Self::parse)
    }

    pub fn local_position(&self) -> [f32; 3] {
        self.local_position_bits.map(f32::from_bits)
    }

    pub fn set_local_position(&mut self, position: [f32; 3]) {
        self.local_position_bits = position.map(f32::to_bits);
    }

    pub fn to_bytes(&self) -> [u8; 128] {
        let mut bytes = [0; 128];
        for (axis, bits) in self.local_position_bits.iter().enumerate() {
            bytes[axis * 4..axis * 4 + 4].copy_from_slice(&bits.to_le_bytes());
        }
        bytes[12] = self.unknown_0c;
        bytes[13] = self.node_index;
        bytes[14] = self.attachment_mode;
        bytes[15..].copy_from_slice(&self.unknown_0f);
        bytes
    }
}

/// DAT entry 165, matched by `10BBA300` before loading entry-166 definitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelEffectBinding {
    /// A separate namespace from `AttachmentGroup::part_code`.
    pub part_code: u16,
    /// Compared with the native weapon class in the weapon branch only.
    pub weapon_class: u16,
    /// Native selector: male/female/special-mode selection depends on the branch.
    pub variant: u16,
    pub model_id: u16,
    /// Zero terminates loading; retain even the unused tail for serialization.
    pub definition_ids: [u16; 8],
}

impl ModelEffectBinding {
    pub const DAT_INDEX: usize = 165;
    pub const SIZE: usize = 24;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bytes = record::<24>(bytes)?;
        Ok(Self {
            part_code: u16::from_le_bytes(bytes[0..2].try_into().unwrap()),
            weapon_class: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
            variant: u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
            model_id: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            definition_ids: ids(&bytes[8..]),
        })
    }

    pub fn parse_table(bytes: &[u8]) -> Result<Vec<Self>> {
        table::<24, _>(bytes, Self::parse)
    }

    pub fn active_definition_ids(&self) -> &[u16] {
        active_ids(&self.definition_ids)
    }

    pub fn to_bytes(&self) -> [u8; 24] {
        let mut bytes = [0; 24];
        for (index, value) in [
            self.part_code,
            self.weapon_class,
            self.variant,
            self.model_id,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        write_ids(&mut bytes, 8, &self.definition_ids);
        bytes
    }
}

/// DAT entry 166. This modifies an existing model's selected draw entry/node;
/// it is not the same record or binding scheme as entry 161.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelEffectDefinition {
    /// Added to / removed from the selected node's translation by `10BBE150`.
    pub translation_delta_bits: [u32; 3],
    pub unknown_0c: u8,
    pub draw_group: u8,
    pub group_entry: u8,
    pub node_index: u8,
    pub start_delay: u16,
    /// Includes interpolation parameters and flags whose full semantics have
    /// not been established. Preserve them instead of assigning guessed names.
    pub unknown_12: [u8; 162],
}

impl ModelEffectDefinition {
    pub const DAT_INDEX: usize = 166;
    pub const SIZE: usize = 180;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bytes = record::<180>(bytes)?;
        Ok(Self {
            translation_delta_bits: std::array::from_fn(|axis| {
                u32::from_le_bytes(bytes[axis * 4..axis * 4 + 4].try_into().unwrap())
            }),
            unknown_0c: bytes[12],
            draw_group: bytes[13],
            group_entry: bytes[14],
            node_index: bytes[15],
            start_delay: u16::from_le_bytes(bytes[16..18].try_into().unwrap()),
            unknown_12: bytes[18..].try_into().unwrap(),
        })
    }

    pub fn parse_table(bytes: &[u8]) -> Result<Vec<Self>> {
        table::<180, _>(bytes, Self::parse)
    }

    pub fn translation_delta(&self) -> [f32; 3] {
        self.translation_delta_bits.map(f32::from_bits)
    }

    pub fn set_translation_delta(&mut self, translation: [f32; 3]) {
        self.translation_delta_bits = translation.map(f32::to_bits);
    }

    pub fn to_bytes(&self) -> [u8; 180] {
        let mut bytes = [0; 180];
        for (axis, bits) in self.translation_delta_bits.iter().enumerate() {
            bytes[axis * 4..axis * 4 + 4].copy_from_slice(&bits.to_le_bytes());
        }
        bytes[12] = self.unknown_0c;
        bytes[13] = self.draw_group;
        bytes[14] = self.group_entry;
        bytes[15] = self.node_index;
        bytes[16..18].copy_from_slice(&self.start_delay.to_le_bytes());
        bytes[18..].copy_from_slice(&self.unknown_12);
        bytes
    }
}
