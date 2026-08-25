/// The purpose of a World presented by the Entrance Service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WorldType {
    Free = 1,
    DundormaTown = 2,
    Beginner = 3,
    PublicTavern = 4,
    ReturningHunter = 5,
    MezeportaFesta = 6,
}

/// The current in-game season for a World.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WorldSeason {
    Breeding = 0,
    Warm = 1,
    Cold = 2,
}

/// The content offered to characters entering a World.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WorldContent {
    AllQuests = 0,
    UpToTwoStarQuests = 1,
    UpToFourStarQuests = 2,
    HunterRankQuests = 4,
    GRankQuests = 5,
    Minigames = 6,
}

/// The client platforms compatible with a World.
///
/// These are historical compatibility codes rather than independent bit flags.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientCompatibility(u32);

impl ClientCompatibility {
    pub const ALL_PLATFORMS: Self = Self(0x0000);
    pub const XBOX_360: Self = Self(0x0002);
    pub const PC: Self = Self(0x0200);
    pub const PC_PS3_PS4: Self = Self(0x1000);
    pub const PS3_PS4: Self = Self(0x1002);
    pub const PSV: Self = Self(0x2042);
    /// Displayed by the client as `PC/PS3(R)`.
    pub const PC_PS3_R: Self = Self(0x3000);
}

impl From<ClientCompatibility> for u32 {
    fn from(compatibility: ClientCompatibility) -> Self {
        compatibility.0
    }
}
