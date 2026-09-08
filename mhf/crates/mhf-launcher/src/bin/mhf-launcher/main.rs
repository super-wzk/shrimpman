use clap::Parser;
use shrimpman_mhf_launcher::runtime::{self, PROFILE};
use std::{path::PathBuf, process::ExitCode};

mod credentials;
mod sign;
mod ui;

#[derive(Parser)]
#[command(about = "Monster Hunter Frontier launcher", version)]
struct Args {
    /// TOML configuration path; defaults to mhf.toml next to the launcher.
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config_path: Option<PathBuf>,

    /// Game directory; defaults to the launcher directory.
    #[arg(short = 'd', long = "game-dir", value_name = "DIRECTORY")]
    game_dir: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();

    match run(args.config_path, args.game_dir) {
        Ok(Some(game_result)) => {
            println!("mhDLL_Main returned {game_result}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mhf-launcher: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(config_path: Option<PathBuf>, game_dir: Option<PathBuf>) -> Result<Option<i32>, String> {
    let prepared = runtime::prepare(config_path, game_dir)?;
    let settings = prepared.sign_settings()?;
    let client = sign::Client::new(&settings.endpoint, settings.encoding)
        .map_err(|error| error.to_string())?;
    let credential_store = credentials::CredentialStore::new(&client.credential_target());
    let Some(request) = ui::run(client, credential_store, settings.encoding)? else {
        return Ok(None);
    };
    prepared
        .launch(
            &PROFILE,
            request.credentials,
            request.sign_in,
            request.selected_character_id,
        )
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_launcher_accepts_paths_and_rejects_debug_mode() {
        let args = Args::try_parse_from([
            "mhf-launcher",
            "--config",
            "online.toml",
            "--game-dir",
            "game",
        ])
        .unwrap();
        assert_eq!(args.config_path, Some(PathBuf::from("online.toml")));
        assert_eq!(args.game_dir, Some(PathBuf::from("game")));
        assert!(Args::try_parse_from(["mhf-launcher", "--debug-quest"]).is_err());
        assert!(Args::try_parse_from(["mhf-launcher", "--quest", "quest.bin"]).is_err());
    }
}
