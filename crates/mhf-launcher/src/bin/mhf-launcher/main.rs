use clap::Parser;
use shrimpman_mhf_launcher::MhfLaunchProfile;
use std::{path::PathBuf, process::ExitCode};

mod config;
mod credentials;
mod font;
mod http;
mod ini_hook;
mod runtime;
mod ui;

const PROFILE: MhfLaunchProfile<'static> = MhfLaunchProfile {
    mhfo_dll: "mhfo.dll",
    mhfo_hd_dll: "mhfo-hd.dll",
    ini_name: "mhf.ini",
    instance_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER",
    ready_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER_READY",
    host_message: "Host protection service is unavailable",
};

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
    let credential_store = credentials::CredentialStore::new(prepared.sign_http_base_url());
    let client =
        http::Client::new(prepared.sign_http_base_url()).map_err(|error| error.to_string())?;
    let Some(request) = ui::run(client, credential_store)? else {
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
