#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
compile_error!("mhf-launcher only supports i686 Windows");

mod abi;
#[cfg(feature = "debug")]
pub mod debug;
pub mod font;
mod launcher;
mod model;
#[cfg(feature = "offline")]
pub mod offline;
mod overlay;
pub mod runtime;
#[cfg(feature = "login")]
mod sign;
#[cfg(feature = "unicode")]
mod text;
mod translation;

pub use abi::MhfLaunchParams32;
pub use model::{
    FontQuality, GraphicsVersion, Language, MhfConfig, MhfFontConfig, MhfLaunchConfig,
    MhfLaunchProfile, MhfLocalizationConfig, MhfOptionConfig, MhfScreenConfig, MhfSetConfig,
    MhfSoundConfig, MhfVideoConfig, Resolution, ScreenMode,
};
#[cfg(feature = "login")]
pub use sign::{IssuedSignSession, PasswordCredentials, SignCharacter, SignInSuccess};
pub use translation::{MissingTranslation, TranslationConfig};
