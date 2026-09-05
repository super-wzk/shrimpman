#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
compile_error!("mhf-launcher only supports i686 Windows");

mod abi;
mod launcher;
mod localization;
mod model;
mod overlay;

pub use abi::MhfLaunchParams32;
pub use launcher::launch_mhfo;
pub use model::{
    Config, FontQuality, GraphicsVersion, IssuedSignSession, Language, MhfConfig, MhfFontConfig,
    MhfLaunchConfig, MhfLaunchProfile, MhfLocalizationConfig, MhfOptionConfig, MhfScreenConfig,
    MhfSetConfig, MhfSoundConfig, MhfVideoConfig, MissingTranslation, PasswordCredentials,
    Resolution, ScreenMode, SignCharacter, SignInSuccess, TranslationConfig,
};
