use jiff::Timestamp;
use shrimpman_mhf_launcher::{MhfLaunchProfile, launch_mhfo};
use std::{env, path::PathBuf};

pub(crate) fn run(
    profile: &MhfLaunchProfile<'_>,
    config_path: Option<PathBuf>,
    game_dir: Option<PathBuf>,
) -> Result<i32, String> {
    let invocation_dir = env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let game_dir = match game_dir {
        Some(path) => invocation_dir.join(path),
        None => env::current_exe()
            .map_err(|error| format!("failed to determine executable path: {error}"))?
            .parent()
            .ok_or_else(|| "executable has no parent directory".to_owned())?
            .to_owned(),
    };
    let config_path = match config_path {
        Some(path) if path.is_absolute() => path,
        Some(path) => invocation_dir.join(path),
        None => invocation_dir.join("mhf.toml"),
    };

    let (config, store) = super::config::load(config_path, Timestamp::now())?;
    let game_dir = enter_game_directory(game_dir)?;
    super::ini_hook::install(profile.ini_name, store)?;

    launch_mhfo(profile, &game_dir, &config)
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
