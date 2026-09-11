//! The four distinct equipment-effect tables used by the ZZ client.
//!
//! These are file records, not relocated DAT pointers or running effect objects.
//! Unknown bytes and float bit patterns survive parsing and serialization.

use crate::{Error, Result};

mod attachment;
pub use attachment::{
    AttachmentDefinition, AttachmentRotation, AttachmentScale, AttachmentSequence,
    AttachmentUvAnimation,
};

mod model;
pub use model::{
    ColorAnimation, ModelEffectDefinition, OpacityAnimation, RotationAnimation, ScaleAnimation,
    UvAnimation,
};

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
