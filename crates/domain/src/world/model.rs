use std::net::Ipv4Addr;

use derive_more::{From, Into};
use serde::{Deserialize, Serialize};

/// Stable identifier of a World, independent of its client-visible index.
#[repr(transparent)]
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, From, Into, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct WorldKey(String);

/// Stable identifier of a Land, independent of its client-visible index.
#[repr(transparent)]
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, From, Into, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct LandKey(String);

/// A client-visible group of Lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    pub key: WorldKey,
    pub address: Ipv4Addr,
    pub name: String,
    pub description: String,
    pub world_type: WorldType,
    pub season: WorldSeason,
    pub content: WorldContent,
    pub client_compatibility: ClientCompatibility,
    pub lands: Vec<Land>,
}

/// A client-visible gameplay destination within a World.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Land {
    pub key: LandKey,
    pub port: u16,
    pub max_players: u16,
    pub current_players: u16,
}

/// The purpose of a World presented by the Entrance Service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WorldSeason {
    Breeding = 0,
    Warm = 1,
    Cold = 2,
}

/// The content offered to characters entering a World.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
#[allow(clippy::upper_case_acronyms)]
pub enum ClientCompatibility {
    AllPlatforms = 0x0000,
    Xbox360 = 0x0002,
    PC = 0x0200,
    PCPS3PS4 = 0x1000,
    PS3PS4 = 0x1002,
    PSV = 0x2042,
    /// Displayed by the client as `PC/PS3(R)`.
    PCPS3R = 0x3000,
}
