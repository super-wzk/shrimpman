#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
compile_error!("mhf-game only supports i686 Windows");

mod abi;
mod game;
mod profile;
pub mod runtime;

pub use abi::MhfLaunchParams32;
pub use game::{GameExit, run};
pub use profile::MhfLaunchProfile;
pub use runtime::LaunchConfig;
