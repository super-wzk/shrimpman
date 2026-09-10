#[cfg(test)]
use crate::manager::{ConfigFile, edit_selection};
use crate::manager::{Manager, validate_id};
use clap::Subcommand;
use mhf_mod_package::{Kind, RuntimeConfig, Selection, VersionReq, export_archive};
use std::{collections::BTreeSet, error::Error, path::PathBuf, str::FromStr};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Subcommand)]
pub(crate) enum Command {
    /// 列出已安装包及其配置状态
    List,
    /// 导入 ZIP 包，不自动启用
    Import { archive: PathBuf },
    /// 解析依赖并导出精确版本；省略 ID 时导出配置中明确启用的 Mod
    Export {
        archive: PathBuf,
        #[arg(value_name = "ID[@VERSION]")]
        mods: Vec<RequestedMod>,
    },
    /// 启用 Mod，可同时设置 semver 版本要求
    Enable {
        id: String,
        #[arg(long)]
        version: Option<VersionReq>,
    },
    /// 关闭 Mod，保留参数和版本要求
    Disable {
        id: String,
        #[arg(long)]
        version: Option<VersionReq>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct RequestedMod {
    id: String,
    version: Option<VersionReq>,
}

impl FromStr for RequestedMod {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (id, version) = match value.split_once('@') {
            Some((id, version)) => (
                id,
                Some(
                    version
                        .parse()
                        .map_err(|error| format!("无效版本要求：{error}"))?,
                ),
            ),
            None => (value, None),
        };
        validate_id(id)?;
        Ok(Self {
            id: id.to_owned(),
            version,
        })
    }
}

pub(crate) fn run(manager: Manager, command: Command) -> Result<()> {
    match command {
        Command::List => {
            let snapshot = manager.load()?;
            println!("Mod 目录：{}", snapshot.mods_dir.display());
            println!("ID\t已安装版本\t类型\t配置开关\t版本要求\t来源");
            let installed: BTreeSet<_> = snapshot
                .candidates
                .iter()
                .map(|candidate| candidate.manifest.id.as_str())
                .collect();
            for candidate in &snapshot.candidates {
                let manifest = &candidate.manifest;
                let settings = snapshot.config.modules.get(&manifest.id);
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    manifest.id,
                    manifest.version,
                    match manifest.kind {
                        Kind::Native => "DLL",
                        Kind::Data => "数据",
                    },
                    enabled_text(settings.and_then(|item| item.enabled)),
                    requirement_text(settings.and_then(|item| item.version.as_ref())),
                    match candidate.source {
                        mhf_mod_package::Source::Builtin => "内置",
                        _ => "外部",
                    }
                );
            }
            for (id, settings) in &snapshot.config.modules {
                if !installed.contains(id.as_str()) {
                    println!(
                        "{}\t未安装\t—\t{}\t{}\t—",
                        id,
                        enabled_text(settings.enabled),
                        requirement_text(settings.version.as_ref())
                    );
                }
            }
        }
        Command::Import { archive } => {
            let (_, count) = manager.import(&archive)?;
            println!("已导入 {count} 个包；启用设置保持不变。");
        }
        Command::Export { archive, mods } => {
            let snapshot = manager.load()?;
            let selections = export_selections(&snapshot.config, &mods);
            let selected = manager.preview(&snapshot, &selections)?;
            if selected.mods.is_empty() {
                return Err("没有选中 Mod；请提供 ID 或先启用 Mod".into());
            }
            export_archive(&std::path::absolute(&archive)?, &selected.mods)?;
            println!(
                "已导出 {} 个 Mod：{}",
                selected.mods.len(),
                archive.display()
            );
        }
        Command::Enable { id, version } => {
            manager.set_enabled(&id, true, version.as_ref())?;
            println!("已启用 {id}，下次启动生效。");
        }
        Command::Disable { id, version } => {
            manager.set_enabled(&id, false, version.as_ref())?;
            println!("已关闭 {id}，下次启动生效。");
        }
    }
    Ok(())
}

fn export_selections(
    config: &RuntimeConfig,
    requested: &[RequestedMod],
) -> std::collections::BTreeMap<String, Selection> {
    let mut selections = config.selections();
    if !requested.is_empty() {
        // Explicit export roots replace other enabled roots. Pins and explicit
        // dependency disables still constrain the combination being exported.
        for selection in selections.values_mut() {
            if selection.enabled == Some(true) {
                selection.enabled = None;
            }
        }
        for request in requested {
            let selection = selections.entry(request.id.clone()).or_default();
            selection.enabled = Some(true);
            if let Some(version) = &request.version {
                selection.version = Some(version.clone());
            }
        }
    }
    selections
}

fn enabled_text(enabled: Option<bool>) -> &'static str {
    match enabled {
        Some(true) => "启用",
        Some(false) => "关闭",
        None => "未设置",
    }
}

fn requirement_text(version: Option<&VersionReq>) -> String {
    version
        .map(ToString::to_string)
        .unwrap_or_else(|| "*".into())
}

#[cfg(test)]
#[path = "tests_cli.rs"]
mod tests;
