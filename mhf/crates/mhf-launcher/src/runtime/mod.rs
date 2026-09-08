//! Shared configuration, font registration and INI lifetime for both binaries.

mod config;
mod ini_hook;

use crate::{
    MhfConfig, MhfLaunchProfile, TranslationConfig,
    launcher::{self, LaunchMode},
};
use std::{env, path::PathBuf};

pub const PROFILE: MhfLaunchProfile<'static> = MhfLaunchProfile {
    mhfo_dll: "mhfo.dll",
    mhfo_hd_dll: "mhfo-hd.dll",
    ini_name: "mhf.ini",
    instance_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER",
    ready_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER_READY",
    host_message: "Host protection service is unavailable",
};

pub struct PreparedLaunch {
    game_dir: PathBuf,
    translation: Option<TranslationConfig>,
    mhf: MhfConfig,
    store: config::Store,
}

pub fn prepare(
    config_path: Option<PathBuf>,
    game_dir: Option<PathBuf>,
) -> Result<PreparedLaunch, String> {
    let invocation_dir = env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let launcher_dir = env::current_exe()
        .map_err(|error| format!("failed to determine executable path: {error}"))?
        .parent()
        .ok_or("executable has no parent directory")?
        .to_owned();
    let game_dir = game_dir.map_or_else(|| launcher_dir.clone(), |path| invocation_dir.join(path));
    let config_path = config_path.map_or_else(
        || launcher_dir.join("mhf.toml"),
        |path| invocation_dir.join(path),
    );
    let (settings, store) = config::load(config_path)?;
    if !game_dir.is_dir() {
        return Err(format!(
            "game directory does not exist: {}",
            game_dir.display()
        ));
    }
    Ok(PreparedLaunch {
        game_dir,
        translation: settings.translation,
        mhf: settings.mhf,
        store,
    })
}

impl PreparedLaunch {
    #[cfg(feature = "login")]
    pub fn sign_http_base_url(&self) -> Result<String, String> {
        self.store.sign_http_base_url()
    }

    #[cfg(feature = "login")]
    pub fn launch(
        self,
        profile: &MhfLaunchProfile<'_>,
        credentials: crate::PasswordCredentials,
        sign_in: crate::SignInSuccess,
        selected_character_id: shrimpman_domain::character::CharacterId,
    ) -> Result<i32, String> {
        self.with_game(profile, |game_dir, mhf, translation| {
            let config = crate::sign::Config {
                credentials,
                sign_in,
                selected_character_id,
                translation,
                mhf,
            };
            launcher::launch(profile, game_dir, LaunchMode::Online(&config))
        })
    }

    #[cfg(feature = "debug")]
    pub fn launch_debug(
        self,
        profile: &MhfLaunchProfile<'_>,
        session: crate::debug::DebugSession,
    ) -> Result<i32, String> {
        self.with_game(profile, |game_dir, mhf, translation| {
            launcher::launch(
                profile,
                game_dir,
                LaunchMode::Debug {
                    settings: &mhf,
                    translation: translation.as_ref(),
                    session,
                },
            )
        })
    }

    fn with_game(
        self,
        profile: &MhfLaunchProfile<'_>,
        launch: impl FnOnce(&str, MhfConfig, Option<TranslationConfig>) -> Result<i32, String>,
    ) -> Result<i32, String> {
        let game_dir = enter_game_directory(self.game_dir)?;
        let _font = if self
            .mhf
            .font
            .name
            .eq_ignore_ascii_case(crate::font::FAMILY_NAME)
        {
            Some(crate::font::register()?)
        } else {
            None
        };
        let mut ini = ini_hook::install(profile.ini_name, self.store)?;
        let result = launch(&game_dir, self.mhf, self.translation);
        match (result, ini.uninstall()) {
            (Err(error), Err(cleanup)) => {
                Err(format!("{error}; INI hook cleanup also failed: {cleanup}"))
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(code), Ok(())) => Ok(code),
        }
    }
}

fn enter_game_directory(game_dir: PathBuf) -> Result<String, String> {
    env::set_current_dir(&game_dir)
        .map_err(|error| format!("failed to enter {}: {error}", game_dir.display()))?;
    let game_dir = env::current_dir()
        .map_err(|error| format!("failed to resolve the game directory: {error}"))?;
    let mut game_dir = game_dir
        .to_str()
        .ok_or("game directory is not valid Unicode")?
        .to_owned();
    if !game_dir.ends_with(['/', '\\']) {
        game_dir.push('\\');
    }
    Ok(game_dir)
}
