//! Minimal caller-supplied data for parser and session tests, not a runtime preset.
use crate::api::MonsterSpawn;

pub(super) fn monster_spawn(species: u8, variant: u8) -> MonsterSpawn {
    MonsterSpawn {
        species,
        variant,
        area: 461,
        position: [0.0; 3],
        yaw: 0,
    }
}

pub(super) fn quest_bytes() -> Vec<u8> {
    fn put(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut bytes = vec![0; 0x400];
    put(&mut bytes, 0, 0x80);
    put(&mut bytes, 4, 0x200);
    put(&mut bytes, 24, 0x240);
    bytes[0x80 + 46..0x80 + 48].copy_from_slice(&40001u16.to_le_bytes());
    put(&mut bytes, 0x80 + 0x28, 0x300);
    put(&mut bytes, 0x240 + 8, 0x280);
    put(&mut bytes, 0x280, 1);
    put(&mut bytes, 0x284, u32::MAX);
    for index in 0..8 {
        put(&mut bytes, 0x300 + index * 4, (0x340 + index * 4) as u32);
        bytes[0x340 + index * 4..0x342 + index * 4].copy_from_slice(&[0x82, 0xa0]);
    }
    bytes
}
