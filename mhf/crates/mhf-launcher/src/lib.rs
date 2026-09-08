#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
compile_error!("mhf-launcher only supports i686 Windows");

mod abi;
#[cfg(feature = "debug")]
pub mod debug;
pub mod font;
mod launcher;
mod localization;
mod model;
mod overlay;
pub mod runtime;
#[cfg(feature = "login")]
mod sign;
mod text;

pub use abi::MhfLaunchParams32;
pub use model::{
    FontQuality, GraphicsVersion, Language, MhfConfig, MhfFontConfig, MhfLaunchConfig,
    MhfLaunchProfile, MhfLocalizationConfig, MhfOptionConfig, MhfScreenConfig, MhfSetConfig,
    MhfSoundConfig, MhfVideoConfig, MissingTranslation, Resolution, ScreenMode, TranslationConfig,
};
#[cfg(feature = "login")]
pub use sign::{IssuedSignSession, PasswordCredentials, SignCharacter, SignInSuccess};
