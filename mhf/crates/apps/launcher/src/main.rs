use clap::Parser;
use mhf_game::runtime::PROFILE;
use mhf_mod_package::Source;
use std::{path::PathBuf, process::ExitCode};

mod builtins;
mod runtime;

#[derive(Parser)]
#[command(about = "Monster Hunter Frontier launcher", version)]
struct Args {
    /// TOML configuration path; defaults to mhf.toml in the current directory.
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config_path: Option<PathBuf>,
    /// Game directory; defaults to the launcher directory.
    #[arg(short = 'd', long = "game-dir", value_name = "DIRECTORY")]
    game_dir: Option<PathBuf>,
    /// Show the selected built-in and external Mods without starting the game.
    #[arg(long)]
    list_mods: bool,
    /// Export the resolved Mod selection without starting the game.
    #[arg(long, value_name = "ZIP", conflicts_with = "list_mods")]
    export_modpack: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(Some(code)) => {
            println!("mhDLL_Main returned {code}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mhf-launcher: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<Option<i32>, String> {
    let prepared = runtime::prepare(args.config_path, args.game_dir)?;
    let catalog = builtins::catalog();
    let mut candidates = catalog.candidates().map_err(|error| error.to_string())?;
    candidates
        .extend(mhf_mod_package::discover(&prepared.mods_dir).map_err(|error| error.to_string())?);
    let resolved = catalog
        .resolve(&prepared.game.mods, &candidates)
        .map_err(|error| error.to_string())?;
    if args.list_mods {
        println!("Mod 目录：{}", prepared.mods_dir.display());
        for candidate in candidates {
            let selected = resolved.mods.iter().any(|chosen| {
                chosen.manifest.id == candidate.manifest.id
                    && chosen.manifest.version == candidate.manifest.version
                    && chosen.source == candidate.source
            });
            println!(
                "{} {}  {}  {}  {}",
                candidate.manifest.id,
                candidate.manifest.version,
                if candidate.source == Source::Builtin {
                    "内置"
                } else {
                    "外部"
                },
                if selected { "已选择" } else { "未选择" },
                candidate.manifest.name
            );
        }
        return Ok(None);
    }
    if let Some(path) = args.export_modpack {
        mhf_mod_package::export_archive(&path, &resolved.mods)
            .map_err(|error| error.to_string())?;
        println!("已导出整合包：{}", path.display());
        return Ok(None);
    }
    let factory = builtins::factory(&prepared);
    mhf_game::run(&prepared.game, &PROFILE, resolved, factory).map(|exit| exit.code)
}
