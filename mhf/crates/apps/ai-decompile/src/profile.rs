//! Verified ZZ HD selector: 10860360 / 1086E100 and its map-only helpers.
//! Fixed-address layout, not a species registry or an automatic version detector.
use mhf_monster::ai::{Error, Result, decompile::Memory};

pub const NAME: &str = "ZZ HD fixed-address layout";

pub fn descriptor(memory: &impl Memory, species: u8, map: u32) -> Result<u32> {
    // This build has 98 map entries and a __int16[98] map-remapping table.
    if map >= 98 {
        return Err(Error::new("map outside this DLL profile's 0..97 map range"));
    }
    if species <= 131 {
        let row = memory.word(0x11a4bd68 + map * 4)?;
        if row == 0 {
            return Err(Error::new("native AI map table is null"));
        }
        return nonnull(
            memory.word(
                row.checked_add(u32::from(species) * 4)
                    .ok_or_else(|| Error::new("descriptor address overflow"))?,
            )?,
        );
    }
    let normalized = || -> Result<u32> {
        let bytes = memory.bytes(0x118648f0 + map * 2, 2)?;
        let value = i16::from_le_bytes(bytes.try_into().map_err(|_| Error::new("short map read"))?);
        Ok(if value == -1 {
            map
        } else {
            value as u16 as u32
        })
    };
    let root = match species {
        132 => 0x11a23380,
        133..=135 | 137..=138 | 157 | 171 | 173 => 0x11a3edd0,
        139 => 0x11a22ed0,
        140 => 0x11a22948,
        141 => match map {
            31 => 0x11a22450,
            50 => 0x11a22318,
            55 | 56 => 0x11a226a0,
            _ => 0x11a22640,
        },
        142 => {
            if map == 55 {
                0x11a21ec8
            } else {
                0x11a21e68
            }
        }
        143 => 0x11a21c00,
        144 => 0x11a0e508,
        145 => 0x11a21b78,
        146 | 153 => match normalized()? {
            6 | 11 | 26 => 0x11a218e0,
            55 => 0x11a21a28,
            69 => 0x11a219c8,
            _ => 0x11a21968,
        },
        147 => match normalized()? {
            5 => 0x11a21080,
            6 => 0x11a20e20,
            11 => 0x11a20ba0,
            _ => 0x11a214b0,
        },
        148 => match normalized()? {
            5 => 0x11a206f0,
            11 => 0x11a20520,
            57 => 0x11a95b38,
            60 => 0x11a95948,
            79 => 0x11a95750,
            _ => 0x11a208e0,
        },
        149 => 0x11a1fac8,
        150 => match map {
            11 => 0x11a194c8,
            21 => 0x11a19388,
            60 => 0x11a19210,
            61 => 0x11a4efc0,
            _ => 0x11a195f8,
        },
        151 => match map {
            11 | 21 => 0x11a952c0,
            53 => 0x11a95510,
            60 | 61 => 0x11a95098,
            _ => 0x11a95470,
        },
        152 => {
            if normalized()? == 5 {
                0x11a94b18
            } else {
                0x11a94c88
            }
        }
        154 => 0x11a94958,
        155 => 0x11a21510,
        156 => 0x11a12cd8,
        158 => 0x11a94618,
        159 => match normalized()? {
            4 => 0x11a93d18,
            6 => 0x11a93b68,
            26 => 0x11a939c0,
            55 => 0x11a93818,
            95 => 0x11a93670,
            _ => 0x11a93f90,
        },
        160 => 0x11a93448,
        161 => match normalized()? {
            55 => 0x11a93250,
            92 | 93 => 0x11a93148,
            _ => 0x11a931d8,
        },
        162 => match normalized()? {
            55 => 0x11a92a68,
            92 | 93 => 0x11a929a8,
            _ => 0x11a92a08,
        },
        163 => 0x11a94020,
        164 => 0x11a925b8,
        165 => 0x11a92388,
        166 => {
            return Err(Error::new(
                "species 166 AI selection depends on quest/runtime state (1015E2A0); species and map alone are insufficient for offline export",
            ));
        }
        167 => 0x11a8a5c8,
        168 => 0x11a8a480,
        169 => match normalized()? {
            3 => 0x11a8a268,
            55 => 0x11a8a0e8,
            92 | 93 => 0x11a89f90,
            _ => 0x11a8a3b8,
        },
        170 => 0x11a899a0,
        172 => 0x11a89710,
        174 => 0x11a89508,
        175 => 0x11a893c0,
        176 => 0x11a892f0,
        _ => 0,
    };
    nonnull(root)
}

fn nonnull(root: u32) -> Result<u32> {
    if root == 0 {
        Err(Error::new(
            "native selector has a null descriptor for this species/map; no AI project was written",
        ))
    } else {
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Map(u16);
    impl Memory for Map {
        fn bytes(&self, _: u32, length: usize) -> Result<Vec<u8>> {
            assert_eq!(length, 2);
            Ok(self.0.to_le_bytes().to_vec())
        }
    }
    #[test]
    fn selectors_distinguish_raw_and_normalized_maps() {
        assert_eq!(descriptor(&Map(55), 141, 31).unwrap(), 0x11a22450);
        assert_eq!(descriptor(&Map(55), 146, 31).unwrap(), 0x11a21a28);
        assert_eq!(descriptor(&Map(0xffff), 146, 31).unwrap(), 0x11a21968);
        assert_eq!(descriptor(&Map(5), 152, 19).unwrap(), 0x11a94b18);
    }
    #[test]
    fn refuses_invalid_maps_and_ambiguous_or_null_roots() {
        assert!(descriptor(&Map(0), 1, 98).is_err());
        assert!(
            descriptor(&Map(0), 166, 31)
                .unwrap_err()
                .to_string()
                .contains("runtime")
        );
        assert!(
            descriptor(&Map(0), 136, 31)
                .unwrap_err()
                .to_string()
                .contains("null")
        );
        assert!(descriptor(&Map(0), 255, 31).is_err());
    }
}
