use derive_more::{From, Into};

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, From, Into)]
pub struct CharacterId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Gender {
    Male = 0,
    Female = 1,
}

/// Weapon class used by a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum WeaponType {
    SwordAndShield = 0,
    HeavyBowgun = 1,
    Hammer = 2,
    GreatSword = 3,
    Lance = 4,
    LightBowgun = 5,
    LongSword = 6,
    DualBlades = 7,
    HuntingHorn = 8,
    Gunlance = 9,
    Bow = 10,
    Tonfa = 11,
    SwitchAxe = 12,
    MagnetSpike = 13,
}

/// Active character state.
pub struct Character {
    pub id: CharacterId,
    pub gender: Gender,
    pub savedata: Option<Vec<u8>>,
    pub name: String,
    pub description: String,
    pub gr: u16,
    pub hr: u16,
    pub weapon_type: WeaponType,
}

impl Character {
    pub fn is_new(&self) -> bool {
        self.savedata.is_none()
    }
}
