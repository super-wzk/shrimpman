//! Native entity-selection strategies and relative target-point directions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TargetStrategy {
    AllowedAreas = 0x52,
    SameArea = 0x53,
    GroundFiltered = 0x5f,
    PlayerOrMonster = 0x7e,
    TrackedBySlot = 0x12,
    LeaderTarget = 0x58,
}

impl TargetStrategy {
    const ALL: [Self; 6] = [
        Self::AllowedAreas,
        Self::SameArea,
        Self::GroundFiltered,
        Self::PlayerOrMonster,
        Self::TrackedBySlot,
        Self::LeaderTarget,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::AllowedAreas => "AllowedAreas",
            Self::SameArea => "SameArea",
            Self::GroundFiltered => "GroundFiltered",
            Self::PlayerOrMonster => "PlayerOrMonster",
            Self::TrackedBySlot => "TrackedBySlot",
            Self::LeaderTarget => "LeaderTarget",
        }
    }

    pub fn opcode(self) -> u8 {
        self as u8
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|strategy| strategy.name() == name)
    }

    pub fn from_opcode(opcode: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|strategy| strategy.opcode() == opcode)
    }
}

/// Direction and fixed distance relative to the monster's position and facing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    Forward500 = 0,
    Left500 = 1,
    Right500 = 2,
    Backward500 = 3,
    Forward1000 = 8,
    Left1000 = 9,
    Right1000 = 10,
    Backward1000 = 11,
}

impl Direction {
    const ALL: [Self; 8] = [
        Self::Forward500,
        Self::Left500,
        Self::Right500,
        Self::Backward500,
        Self::Forward1000,
        Self::Left1000,
        Self::Right1000,
        Self::Backward1000,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Forward500 => "Forward500",
            Self::Left500 => "Left500",
            Self::Right500 => "Right500",
            Self::Backward500 => "Backward500",
            Self::Forward1000 => "Forward1000",
            Self::Left1000 => "Left1000",
            Self::Right1000 => "Right1000",
            Self::Backward1000 => "Backward1000",
        }
    }

    pub fn from_native(value: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|direction| *direction as u8 == value)
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|direction| direction.name() == name)
    }
}
