use shrimpman_domain::character::CharacterId;
use shrimpman_mhf_launcher::{
    Config, MhfConfig, MhfLaunchProfile, PasswordCredentials, SignInSuccess, TranslationConfig,
    launch_mhfo,
};
use std::{env, path::PathBuf};

pub(crate) struct PreparedLaunch {
    game_dir: PathBuf,
    sign: super::config::sign::Settings,
    translation: Option<TranslationConfig>,
    mhf: MhfConfig,
    store: super::config::Store,
}

pub(crate) fn prepare(
    config_path: Option<PathBuf>,
    game_dir: Option<PathBuf>,
) -> Result<PreparedLaunch, String> {
    let invocation_dir = env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let launcher_dir = env::current_exe()
        .map_err(|error| format!("failed to determine executable path: {error}"))?
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_owned())?
        .to_owned();
    let game_dir = match game_dir {
        Some(path) => invocation_dir.join(path),
        None => launcher_dir.clone(),
    };
    let config_path = match config_path {
        Some(path) => invocation_dir.join(path),
        None => launcher_dir.join("mhf.toml"),
    };

    let (settings, store) = super::config::load(config_path)?;
    if !game_dir.is_dir() {
        return Err(format!(
            "game directory does not exist: {}",
            game_dir.display()
        ));
    }

    Ok(PreparedLaunch {
        game_dir,
        sign: settings.sign,
        translation: settings.translation,
        mhf: settings.mhf,
        store,
    })
}

impl PreparedLaunch {
    pub(crate) fn sign_http_base_url(&self) -> &str {
        &self.sign.http.base_url
    }

    pub(crate) fn launch(
        self,
        profile: &MhfLaunchProfile<'_>,
        credentials: PasswordCredentials,
        sign_in: SignInSuccess,
        selected_character_id: CharacterId,
    ) -> Result<i32, String> {
        let game_dir = enter_game_directory(self.game_dir)?;
        let configured_font = &self.mhf.font.name;
        let _font_registration = if configured_font.eq_ignore_ascii_case(super::font::FAMILY_NAME) {
            Some(super::font::register()?)
        } else {
            None
        };
        let mut ini_hooks = super::ini_hook::install(profile.ini_name, self.store)?;
        let config = Config {
            credentials,
            sign_in,
            selected_character_id,
            translation: self.translation,
            mhf: self.mhf,
        };
        let result = launch_mhfo(profile, &game_dir, &config);
        let cleanup = ini_hooks.uninstall();
        match (result, cleanup) {
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
        .ok_or_else(|| "game directory is not valid Unicode".to_owned())?
        .to_owned();
    if !game_dir.ends_with(['/', '\\']) {
        game_dir.push('\\');
    }
    Ok(game_dir)
}
