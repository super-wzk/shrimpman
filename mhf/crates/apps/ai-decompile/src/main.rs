mod pe;
mod profile;

use clap::Parser;
use mhf_monster::ai::{Error, Result, decompile, dsl::Project};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug, Default, Parser)]
#[command(
    version,
    about = "Export a monster AI project without starting the game",
    after_help = "Reads mhfo-hd.dll from the current directory, or MHF_GAME_DIR.\nWrites ai-export/monster-ai/maps/<map>/<species>/ in the current directory; refuses existing projects unless --overwrite is set.\nExports reachable scripts with base native, not the entire native AI implementation."
)]
struct Args {
    /// Game directory; overrides MHF_GAME_DIR (default: current directory)
    #[arg(short = 'd', long = "game-dir", value_name = "DIRECTORY")]
    game_dir: Option<PathBuf>,
    /// Accepted for launcher-wrapper compatibility; offline export does not read runtime configuration
    #[arg(short = 'c', long = "config", value_name = "FILE")]
    config_path: Option<PathBuf>,
    /// Overwrite generated files in an existing project; preserve other files
    #[arg(long)]
    overwrite: bool,
    /// Monster species ID
    species: u8,
    /// Map ID (required; AI selection depends on the map)
    map: u32,
}

struct Paths {
    dll: PathBuf,
    out: PathBuf,
}

fn paths(args: &Args, cwd: &Path, game_dir: Option<OsString>) -> Result<Paths> {
    let game_dir = match args
        .game_dir
        .as_ref()
        .map(|p| p.as_os_str().to_owned())
        .or(game_dir)
    {
        Some(value) if value.is_empty() => {
            return Err(Error::new("game directory must not be empty"));
        }
        Some(value) => cwd.join(value),
        None => cwd.to_path_buf(),
    };
    Ok(Paths {
        dll: game_dir.join("mhfo-hd.dll"),
        out: cwd.join("ai-export/monster-ai"),
    })
}

fn extract(image: &pe::Image, root: u32, args: &Args) -> Result<(Project, Vec<String>)> {
    let output = decompile::decompile(image, root, args.species, 0, Some(args.map))?;
    let project = Project::single(Some(args.map), args.species, output.source);
    // Never publish source that the same toolchain cannot compile.
    project
        .compile()
        .map_err(|e| Error::new(format!("exported project failed validation: {e}")))?;
    Ok((project, output.warnings))
}

fn write_project(out: &Path, project: &Project, report: &str, overwrite: bool) -> Result<PathBuf> {
    let io = |e: std::io::Error| Error::new(e.to_string());
    let entry = out.join(&project.entry);
    let project_dir = entry.parent().unwrap();
    if let Some(parent) = project_dir.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    // Only reserve this species/map directory; sibling projects may already exist.
    let created = match fs::create_dir(project_dir) {
        Ok(()) => true,
        Err(error)
            if overwrite
                && error.kind() == std::io::ErrorKind::AlreadyExists
                && fs::symlink_metadata(project_dir).is_ok_and(|m| m.file_type().is_dir()) =>
        {
            false
        }
        Err(error) => {
            return Err(Error::new(format!(
                "cannot create output directory {}: {error}",
                project_dir.display()
            )));
        }
    };
    let write = || -> Result<()> {
        for file in &project.files {
            let path = out.join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
            fs::write(path, &file.source).map_err(io)?;
        }
        fs::write(project_dir.join("export.txt"), report).map_err(io)?;
        Ok(())
    };
    if let Err(error) = write() {
        // Only this invocation's newly created directory is removed.
        if created {
            let _ = fs::remove_dir_all(project_dir);
        }
        return Err(error);
    }
    Ok(entry)
}

fn export(args: &Args, paths: &Paths) -> Result<PathBuf> {
    let bytes =
        fs::read(&paths.dll).map_err(|e| Error::new(format!("{}: {e}", paths.dll.display())))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let image = pe::Image::parse(bytes)?;
    let root = profile::descriptor(&image, args.species, args.map)?;
    let (project, warnings) = extract(&image, root, args)?;
    let mut report = format!(
        "DLL: {}\nSHA-256: {hash}\nProfile: {}\nSpecies: {}\nMap: {}\nDescriptor VA: 0x{root:08X}\nEntry: {}\nScope: state 0 and reachable states, event entries and explicit subscript references only; base native remains required.\nMemory: PE image initial contents, including zero-filled section tails; runtime writes are not reproduced.\nExtraction warnings: {}\n",
        paths.dll.display(),
        profile::NAME,
        args.species,
        args.map,
        project.entry,
        warnings.len(),
    );
    for warning in &warnings {
        report.push_str(warning);
        report.push('\n');
    }
    let entry = write_project(&paths.out, &project, &report, args.overwrite)?;
    for warning in warnings {
        eprintln!("mhf-ai-decompile: warning: {warning}");
    }
    Ok(entry)
}

fn run(args: &Args) -> Result<PathBuf> {
    let cwd = env::current_dir().map_err(|e| Error::new(e.to_string()))?;
    export(args, &paths(args, &cwd, env::var_os("MHF_GAME_DIR"))?)
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(path) => {
            println!(
                "Exported {} (base native; see export.txt for scope)",
                path.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("mhf-ai-decompile: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_exactly_species_and_map() {
        let args = Args::try_parse_from(["mhf-ai-decompile", "1", "31"]).unwrap();
        assert_eq!((args.species, args.map), (1, 31));
        assert!(!args.overwrite);
        assert!(
            Args::try_parse_from(["mhf-ai-decompile", "1", "31", "--overwrite"])
                .unwrap()
                .overwrite
        );
        for values in [
            vec![],
            vec!["1"],
            vec!["256", "31"],
            vec!["1", "-1"],
            vec!["1", "31", "extra"],
            vec!["--dll", "client.dll"],
        ] {
            assert!(
                Args::try_parse_from(std::iter::once("mhf-ai-decompile").chain(values)).is_err()
            );
        }
    }

    #[test]
    fn game_directory_override_does_not_move_output() {
        let args = Args {
            species: 1,
            map: 31,
            ..Args::default()
        };
        let cwd = Path::new("workspace");
        let default = paths(&args, cwd, None).unwrap();
        assert_eq!(default.dll, cwd.join("mhfo-hd.dll"));
        let overridden = paths(&args, cwd, Some("game".into())).unwrap();
        assert_eq!(overridden.dll, cwd.join("game/mhfo-hd.dll"));
        assert_eq!(default.out, overridden.out);
        assert!(paths(&args, cwd, Some(OsString::new())).is_err());
    }

    #[test]
    fn accepts_windows_wrapper_arguments() {
        let args = Args::try_parse_from([
            "mhf-ai-decompile",
            "--config",
            "missing.toml",
            "--game-dir",
            "configured-game",
            "1",
            "31",
        ])
        .unwrap();
        assert_eq!(args.config_path, Some(PathBuf::from("missing.toml")));
        let cwd = Path::new("workspace");
        let selected = paths(&args, cwd, Some("environment-game".into())).unwrap();
        assert_eq!(selected.dll, cwd.join("configured-game/mhfo-hd.dll"));
        assert_eq!(selected.out, cwd.join("ai-export/monster-ai"));
    }

    #[test]
    fn synthetic_pe_exports_without_zero_filled_events() {
        let mut data = pe::tests::fixture();
        data[0x200..0x204].copy_from_slice(&0x10001060u32.to_le_bytes());
        data[0x260..0x264].copy_from_slice(&0x10001080u32.to_le_bytes());
        data[0x280..0x282].copy_from_slice(&[0xff, 0]);
        let args = Args {
            species: 1,
            map: 31,
            ..Args::default()
        };
        let (project, warnings) =
            extract(&pe::Image::parse(data.clone()).unwrap(), 0x10001000, &args).unwrap();
        assert_eq!(project.entry, "maps/31/1/main.mhai");
        assert!(warnings.is_empty());
        // A nonzero bait-event cell address can still contain a null script pointer.
        data[0x220..0x224].copy_from_slice(&0x10001100u32.to_le_bytes());
        let (without_event, warnings) =
            extract(&pe::Image::parse(data.clone()).unwrap(), 0x10001000, &args).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(without_event.files, project.files);

        // An address beyond the section remains a warning, never an invented event.
        data[0x220..0x224].copy_from_slice(&0x10001200u32.to_le_bytes());
        let (partial, warnings) =
            extract(&pe::Image::parse(data.clone()).unwrap(), 0x10001000, &args).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("事件 6 沿用原生："));
        assert!(!partial.files[0].source.contains("bait_detected =>"));
        assert!(partial.files[0].source.contains(&warnings[0]));

        // With no recoverable main script or event, extraction still fails.
        data[0x260..0x264].fill(0);
        assert!(extract(&pe::Image::parse(data).unwrap(), 0x10001000, &args).is_err());
    }

    #[test]
    fn null_subscript_keeps_native_call_and_warning() {
        let mut data = pe::tests::fixture();
        data[0x200..0x204].copy_from_slice(&0x10001060u32.to_le_bytes());
        data[0x204..0x208].copy_from_slice(&0x10001070u32.to_le_bytes());
        data[0x260..0x264].copy_from_slice(&0x10001080u32.to_le_bytes());
        data[0x280..0x284].copy_from_slice(&[0x81, 1, 0xff, 0]);
        let args = Args {
            species: 146,
            map: 31,
            ..Args::default()
        };
        let (project, warnings) =
            extract(&pe::Image::parse(data).unwrap(), 0x10001000, &args).unwrap();
        assert_eq!(warnings, ["表 1 项 1 沿用原生：null script"]);
        let source = &project.files[0].source;
        assert!(source.contains("base native;"));
        assert!(source.contains("native(0x81, 0x01);"));
        assert!(source.contains(&warnings[0]));
        assert!(!source.contains("fn sub_1_1"));
    }

    #[test]
    fn overwrite_requires_opt_in_and_preserves_other_files() {
        let root = env::temp_dir().join(format!("mhf-ai-decompile-write-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let out = root.join("ai-export/monster-ai");
        let project = Project::single(Some(31), 1, "original".into());
        let entry = write_project(&out, &project, "report", false).unwrap();
        assert!(
            write_project(
                &out,
                &Project::single(Some(31), 1, "replacement".into()),
                "other",
                false
            )
            .is_err()
        );
        assert_eq!(entry, out.join("maps/31/1/main.mhai"));
        assert_eq!(fs::read_to_string(&entry).unwrap(), "original");
        assert_eq!(
            fs::read_to_string(out.join("maps/31/1/export.txt")).unwrap(),
            "report"
        );
        for (map, species) in [(31, 11), (32, 1)] {
            let sibling = Project::single(Some(map), species, "sibling".into());
            let path = write_project(&out, &sibling, "sibling report", false).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), "sibling");
            assert_eq!(
                fs::read_to_string(path.parent().unwrap().join("export.txt")).unwrap(),
                "sibling report"
            );
        }
        assert_eq!(fs::read_to_string(&entry).unwrap(), "original");
        let custom = entry.parent().unwrap().join("custom.mhai");
        fs::write(&custom, "keep").unwrap();
        write_project(
            &out,
            &Project::single(Some(31), 1, "replacement".into()),
            "updated report",
            true,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&entry).unwrap(), "replacement");
        assert_eq!(
            fs::read_to_string(entry.parent().unwrap().join("export.txt")).unwrap(),
            "updated report"
        );
        assert_eq!(fs::read_to_string(custom).unwrap(), "keep");
        assert_eq!(
            fs::read_to_string(out.join("maps/31/11/main.mhai")).unwrap(),
            "sibling"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires MHF_AI_DLL pointing at the verified real DLL"]
    fn real_dll_projects_compile() {
        let bytes = fs::read(env::var_os("MHF_AI_DLL").expect("set MHF_AI_DLL")).unwrap();
        let image = pe::Image::parse(bytes).unwrap();
        for (species, map) in [
            (1, 31),
            (11, 31),
            (100, 31),
            (141, 31),
            (141, 50),
            (146, 31),
            (146, 55),
        ] {
            let root = profile::descriptor(&image, species, map).unwrap();
            let (project, warnings) = extract(
                &image,
                root,
                &Args {
                    species,
                    map,
                    ..Args::default()
                },
            )
            .unwrap_or_else(|e| panic!("species {species}, map {map}: {e}"));
            if species == 146 {
                assert!(!project.files[0].source.contains("bait_detected =>"));
                assert_eq!(warnings, ["表 1 项 1 沿用原生：null script"]);
            } else if matches!(species, 1 | 11 | 100) {
                assert!(
                    warnings.is_empty(),
                    "species {species}, map {map}: {warnings:?}"
                );
            }
        }
    }
}
