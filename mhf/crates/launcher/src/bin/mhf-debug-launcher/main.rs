use clap::Parser;
use mhf_launcher::{
    offline::Session,
    runtime::{self, PROFILE},
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(about = "Monster Hunter Frontier offline debug launcher", version)]
struct Args {
    /// TOML configuration path; defaults to mhf.toml next to this executable.
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config_path: Option<PathBuf>,

    /// Game directory; defaults to this executable's directory.
    #[arg(short = 'd', long = "game-dir", value_name = "DIRECTORY")]
    game_dir: Option<PathBuf>,

    /// Offline quest BIN; defaults to the embedded Historical Site test quest.
    #[arg(long, value_name = "BIN")]
    quest: Option<PathBuf>,

    /// Run the offline quest without the debug tools or control window.
    #[arg(long)]
    no_debug: bool,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => {
            println!("mhDLL_Main returned {code}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("mhf-debug-launcher: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<i32, String> {
    let _debug_enabled = cfg!(feature = "debug") && !args.no_debug;
    let prepared = runtime::prepare(args.config_path, args.game_dir)?;
    let session = match args.quest {
        Some(path) => {
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            Session::new(&bytes)?
        }
        None => Session::test_map()?,
    };
    #[cfg(feature = "debug")]
    if _debug_enabled {
        return prepared.launch_debug(&PROFILE, session);
    }
    prepared.launch_offline(&PROFILE, session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_launcher_defaults_to_the_embedded_quest() {
        let args = Args::try_parse_from(["mhf-debug-launcher"]).unwrap();
        assert!(args.quest.is_none());
        assert!(!args.no_debug);
    }

    #[test]
    fn debug_launcher_accepts_an_explicit_quest_and_paths() {
        let args = Args::try_parse_from([
            "mhf-debug-launcher",
            "--quest",
            "quest.bin",
            "--config",
            "debug.toml",
            "--no-debug",
            "-d",
            "game",
        ])
        .unwrap();
        assert_eq!(args.quest, Some(PathBuf::from("quest.bin")));
        assert_eq!(args.config_path, Some(PathBuf::from("debug.toml")));
        assert_eq!(args.game_dir, Some(PathBuf::from("game")));
        assert!(args.no_debug);
    }
}
