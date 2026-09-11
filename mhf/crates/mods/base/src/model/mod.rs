//! Shared equipment and appearance data for in-process model tools.

#[cfg(all(windows, target_arch = "x86"))]
pub mod native;

// Native DAT class IDs (melee record +3, ranged record +4) and player +3.
// These differ from the server's character::WeaponType discriminants.
pub const NATIVE_WEAPON_NAMES: [&str; 14] = [
    "大剑",
    "重弩",
    "大锤",
    "长枪",
    "片手剑",
    "轻弩",
    "双剑",
    "太刀",
    "狩猎笛",
    "铳枪",
    "弓",
    "穿龙棍",
    "斩斧 F",
    "磁斩锤",
];

#[derive(Clone, Debug)]
pub struct Equipment {
    pub kind: u8,
    pub id: u16,
    pub model_ids: [u16; 2],
    pub weapon: Option<u8>,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct EquipmentCatalog {
    pub equipment: Vec<Equipment>,
    pub appearances: [AppearanceOptions; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transmogs {
    // Native armor kinds; kind 1 is the face, which has no armor override.
    pub armor: [u16; 6],
}

impl Transmogs {
    pub fn changed(
        mut self,
        kind: u8,
        id: Option<u16>,
        equipment: &[Equipment],
    ) -> Result<Self, &'static str> {
        if !matches!(kind, 0 | 2..=5) {
            return Err("此部位不支持防具幻化");
        }
        if let Some(id) = id
            && (id == 0
                || !equipment
                    .iter()
                    .any(|item| item.kind == kind && item.id == id && item.weapon.is_none()))
        {
            return Err("幻化防具编号无效");
        }
        self.armor[usize::from(kind)] = id.unwrap_or(0);
        Ok(self)
    }

    pub fn selected(&self, kind: u8) -> Option<u16> {
        self.armor
            .get(usize::from(kind))
            .copied()
            .filter(|&id| id != 0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    pub female: bool,
    pub face: u8,
    pub hair: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceChange {
    Gender(bool),
    Face(u8),
    Hair(u8),
}

#[derive(Clone, Debug, Default)]
pub struct AppearanceOptions {
    pub faces: Vec<Face>,
    pub hair: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub id: u8,
    pub model_id: u16,
}

impl Appearance {
    pub fn changed(
        mut self,
        change: AppearanceChange,
        options: &[AppearanceOptions; 2],
    ) -> Result<Self, &'static str> {
        match change {
            AppearanceChange::Gender(female) => {
                let options = &options[usize::from(female)];
                let face = options.faces.first().ok_or("此性别没有可用脸型")?.id;
                let hair = *options.hair.first().ok_or("此性别没有可用发型")?;
                self.female = female;
                if !options.faces.iter().any(|face| face.id == self.face) {
                    self.face = face;
                }
                if !options.hair.contains(&self.hair) {
                    self.hair = hair;
                }
            }
            AppearanceChange::Face(face) => {
                if !options[usize::from(self.female)]
                    .faces
                    .iter()
                    .any(|option| option.id == face)
                {
                    return Err("脸型编号无效");
                }
                self.face = face;
            }
            AppearanceChange::Hair(hair) => {
                if !options[usize::from(self.female)].hair.contains(&hair) {
                    return Err("发型编号无效");
                }
                self.hair = hair;
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
