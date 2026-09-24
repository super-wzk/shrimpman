//! Native entity-selection strategies and relative target-point directions.

/// Entity-selection strategies. The upper variants are single-byte selector
/// opcodes; the four `0..=3` values are `0x06` mode-13 subtypes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EntityTarget {
    SameArea = 0x53,
    SameAreaGroundGroup = 0x5f,
    SameOrAllowedArea = 0x52,
    TrackedPlayer = 0x12,
    LeaderTarget = 0x58,
    PlayerOrMonster = 0x7e,
    CurrentOrLargeMonster = 0,
    LargeMonster = 1,
    OtherMonster = 2,
    OtherLargeMonster = 3,
}

impl EntityTarget {
    const ALL: [Self; 10] = [
        Self::SameArea,
        Self::SameAreaGroundGroup,
        Self::SameOrAllowedArea,
        Self::TrackedPlayer,
        Self::LeaderTarget,
        Self::PlayerOrMonster,
        Self::CurrentOrLargeMonster,
        Self::LargeMonster,
        Self::OtherMonster,
        Self::OtherLargeMonster,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SameArea => "SameArea",
            Self::SameAreaGroundGroup => "SameAreaGroundGroup",
            Self::SameOrAllowedArea => "SameOrAllowedArea",
            Self::TrackedPlayer => "TrackedPlayer",
            Self::LeaderTarget => "LeaderTarget",
            Self::PlayerOrMonster => "PlayerOrMonster",
            Self::CurrentOrLargeMonster => "CurrentOrLargeMonster",
            Self::LargeMonster => "LargeMonster",
            Self::OtherMonster => "OtherMonster",
            Self::OtherLargeMonster => "OtherLargeMonster",
        }
    }

    pub fn opcode(self) -> u8 {
        match self {
            Self::CurrentOrLargeMonster
            | Self::LargeMonster
            | Self::OtherMonster
            | Self::OtherLargeMonster => 0x06,
            _ => self as u8,
        }
    }

    pub fn encode(self) -> Vec<u8> {
        let opcode = self.opcode();
        if opcode == 0x06 {
            vec![opcode, 13, self as u8, 0]
        } else {
            vec![opcode]
        }
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let value = match bytes {
            [0x06, 13, subtype @ 0..=3, 0] => *subtype,
            [opcode] if *opcode > 3 => *opcode,
            _ => return None,
        };
        Self::ALL
            .into_iter()
            .find(|strategy| *strategy as u8 == value)
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|strategy| strategy.name() == name)
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
