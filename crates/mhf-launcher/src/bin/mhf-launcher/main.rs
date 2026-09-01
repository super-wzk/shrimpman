use shrimpman_mhf_launcher::MhfLaunchProfile;
use std::process::ExitCode;

mod config;
mod ini_hook;
mod runtime;

const PROFILE: MhfLaunchProfile<'static> = MhfLaunchProfile {
    mhfo_dll: "mhfo.dll",
    mhfo_hd_dll: "mhfo-hd.dll",
    ini_name: "mhf.ini",
    instance_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER",
    ready_mutex_prefix: "Monster Hunter Frontier Z MHF_MASTER_READY",
    host_message: "Host protection service is unavailable",
};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let config_path = args.next().map(std::path::PathBuf::from);
    let game_dir = args.next().map(std::path::PathBuf::from);

    if args.next().is_some() {
        eprintln!("usage: mhf-launcher.exe [mhf.toml] [game-directory]");
        return ExitCode::FAILURE;
    }

    match runtime::run(&PROFILE, config_path, game_dir) {
        Ok(game_result) => {
            println!("mhDLL_Main returned {game_result}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("mhf-launcher: {error}");
            ExitCode::FAILURE
        }
    }
}
