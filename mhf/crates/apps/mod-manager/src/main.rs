#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::Parser;
use manager::Manager;
use std::{path::PathBuf, process::ExitCode};

mod cli;
#[cfg(feature = "gui")]
mod dialogs;
#[cfg(feature = "gui")]
mod gui;
mod manager;

#[derive(Parser)]
#[command(
    name = "mhf-mods",
    about = "MHF Mod 管理器；不指定子命令时打开图形界面"
)]
struct Args {
    /// 配置路径，默认读取当前工作目录中的 mhf.toml
    #[arg(long, global = true, default_value = "mhf.toml")]
    config: PathBuf,
    /// Mod 根目录，相对当前工作目录解析，覆盖配置中的 directory
    #[arg(long, global = true)]
    mods_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<cli::Command>,
}

fn main() -> ExitCode {
    // GUI launches do not create a console; CLI invocations reuse the caller's.
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    let args = Args::parse();
    let result = Manager::new(args.config, args.mods_dir).and_then(|manager| {
        if let Some(command) = args.command {
            cli::run(manager, command).map_err(|error| error.to_string())
        } else {
            #[cfg(feature = "gui")]
            return gui::run(manager);
            #[cfg(not(feature = "gui"))]
            Err("当前构建未包含图形界面，请指定命令行子命令".into())
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("失败：{error}");
            ExitCode::FAILURE
        }
    }
}
