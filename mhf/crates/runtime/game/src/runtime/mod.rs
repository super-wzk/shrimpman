//! Game client profile and the neutral inputs owned by the host.

use crate::{MhfLaunchParams32, MhfLaunchProfile};
use std::path::PathBuf;

pub const PROFILE: MhfLaunchProfile<'static> = MhfLaunchProfile {
    game_dll: "mhfo-hd.dll",
    ini_name: "mhf.ini",
    instance_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER",
    ready_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER_READY",
    host_message: "Host protection service is unavailable",
};

pub struct LaunchConfig {
    pub game_dir: PathBuf,
    pub params: MhfLaunchParams32,
    pub mods: mhf_mod_package::RuntimeConfig,
}
