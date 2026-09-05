use shrimpman_domain::character::{Character, CharacterId, Gender, WeaponType};

use super::CharacterRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, toasty::Embed)]
#[column(type = enum("gender"))]
pub(crate) enum StoredGender {
    Male,
    Female,
}

impl From<StoredGender> for Gender {
    fn from(gender: StoredGender) -> Self {
        match gender {
            StoredGender::Male => Self::Male,
            StoredGender::Female => Self::Female,
        }
    }
}

impl From<Gender> for StoredGender {
    fn from(gender: Gender) -> Self {
        match gender {
            Gender::Male => Self::Male,
            Gender::Female => Self::Female,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, toasty::Embed)]
#[column(type = enum("weapon_type"))]
pub(crate) enum StoredWeaponType {
    SwordAndShield,
    HeavyBowgun,
    Hammer,
    GreatSword,
    Lance,
    LightBowgun,
    LongSword,
    DualBlades,
    HuntingHorn,
    Gunlance,
    Bow,
    Tonfa,
    SwitchAxe,
    MagnetSpike,
}

impl From<StoredWeaponType> for WeaponType {
    fn from(weapon_type: StoredWeaponType) -> Self {
        match weapon_type {
            StoredWeaponType::SwordAndShield => Self::SwordAndShield,
            StoredWeaponType::HeavyBowgun => Self::HeavyBowgun,
            StoredWeaponType::Hammer => Self::Hammer,
            StoredWeaponType::GreatSword => Self::GreatSword,
            StoredWeaponType::Lance => Self::Lance,
            StoredWeaponType::LightBowgun => Self::LightBowgun,
            StoredWeaponType::LongSword => Self::LongSword,
            StoredWeaponType::DualBlades => Self::DualBlades,
            StoredWeaponType::HuntingHorn => Self::HuntingHorn,
            StoredWeaponType::Gunlance => Self::Gunlance,
            StoredWeaponType::Bow => Self::Bow,
            StoredWeaponType::Tonfa => Self::Tonfa,
            StoredWeaponType::SwitchAxe => Self::SwitchAxe,
            StoredWeaponType::MagnetSpike => Self::MagnetSpike,
        }
    }
}

impl From<WeaponType> for StoredWeaponType {
    fn from(weapon_type: WeaponType) -> Self {
        match weapon_type {
            WeaponType::SwordAndShield => Self::SwordAndShield,
            WeaponType::HeavyBowgun => Self::HeavyBowgun,
            WeaponType::Hammer => Self::Hammer,
            WeaponType::GreatSword => Self::GreatSword,
            WeaponType::Lance => Self::Lance,
            WeaponType::LightBowgun => Self::LightBowgun,
            WeaponType::LongSword => Self::LongSword,
            WeaponType::DualBlades => Self::DualBlades,
            WeaponType::HuntingHorn => Self::HuntingHorn,
            WeaponType::Gunlance => Self::Gunlance,
            WeaponType::Bow => Self::Bow,
            WeaponType::Tonfa => Self::Tonfa,
            WeaponType::SwitchAxe => Self::SwitchAxe,
            WeaponType::MagnetSpike => Self::MagnetSpike,
        }
    }
}

impl From<CharacterRow> for Character {
    fn from(character: CharacterRow) -> Self {
        Self {
            id: CharacterId::from(character.id),
            gender: character.gender.into(),
            savedata: character.savedata,
            name: character.name,
            description: character.description,
            gr: character.gr,
            hr: character.hr,
            weapon_type: character.weapon_type.into(),
        }
    }
}
