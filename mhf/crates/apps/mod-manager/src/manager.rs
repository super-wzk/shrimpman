use mhf_mod_package::{BuiltinCatalog, Candidate, Resolved, RuntimeConfig, Selection, VersionReq};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Table, value};

type Result<T> = std::result::Result<T, String>;

pub(crate) const CATALOG: BuiltinCatalog = BuiltinCatalog {
    login: cfg!(feature = "login"),
    debug: cfg!(feature = "debug"),
};

#[derive(Deserialize)]
pub(crate) struct ConfigFile {
    #[serde(default)]
    pub mods: RuntimeConfig,
}

#[derive(Clone)]
pub(crate) struct Manager {
    pub config_path: PathBuf,
    pub mods_dir_override: Option<PathBuf>,
}

pub(crate) struct Snapshot {
    pub config: RuntimeConfig,
    pub mods_dir: PathBuf,
    pub candidates: Vec<Candidate>,
}

impl Manager {
    pub fn new(config_path: PathBuf, mods_dir: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            config_path: std::path::absolute(config_path).map_err(|error| error.to_string())?,
            mods_dir_override: mods_dir
                .map(std::path::absolute)
                .transpose()
                .map_err(|error| error.to_string())?,
        })
    }

    fn source(&self) -> Result<String> {
        fs::read_to_string(&self.config_path).map_err(|error| {
            format!(
                "读取配置 {} 失败：{error}。可在启动时通过 --config 指定配置文件。",
                self.config_path.display()
            )
        })
    }

    pub fn load(&self) -> Result<Snapshot> {
        self.snapshot(&self.source()?)
    }

    fn snapshot(&self, source: &str) -> Result<Snapshot> {
        let file: ConfigFile = toml::from_str(source)
            .map_err(|error| format!("配置 {} 无效：{error}", self.config_path.display()))?;
        let mods_dir = std::path::absolute(
            self.mods_dir_override
                .as_ref()
                .unwrap_or(&file.mods.directory),
        )
        .map_err(|error| error.to_string())?;
        let mut candidates = CATALOG.candidates().map_err(|error| error.to_string())?;
        candidates.extend(mhf_mod_package::discover(&mods_dir).map_err(|error| error.to_string())?);
        Ok(Snapshot {
            config: file.mods,
            mods_dir,
            candidates,
        })
    }

    pub fn preview(
        &self,
        snapshot: &Snapshot,
        edits: &BTreeMap<String, Selection>,
    ) -> Result<Resolved> {
        let mut config = snapshot.config.clone();
        for (id, selection) in edits {
            let settings = config.modules.entry(id.clone()).or_default();
            settings.enabled = selection.enabled;
            settings.version = selection.version.clone();
        }
        mhf_mod_package::resolve(
            &snapshot.candidates,
            &config.selections(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .map_err(|error| error.to_string())
    }

    #[cfg(feature = "gui")]
    pub fn save(
        &self,
        baseline: &RuntimeConfig,
        edits: &BTreeMap<String, Selection>,
    ) -> Result<Snapshot> {
        let source = self.source()?;
        let snapshot = self.snapshot(&source)?;
        let merged: BTreeMap<_, _> = edits
            .iter()
            .map(|(id, edited)| {
                let before = baseline.modules.get(id);
                let current = snapshot.config.modules.get(id);
                let enabled = if edited.enabled != before.and_then(|settings| settings.enabled) {
                    edited.enabled
                } else {
                    current.and_then(|settings| settings.enabled)
                };
                let version = if edited.version.as_ref()
                    != before.and_then(|settings| settings.version.as_ref())
                {
                    edited.version.clone()
                } else {
                    current.and_then(|settings| settings.version.clone())
                };
                (id.clone(), Selection { enabled, version })
            })
            .collect();
        self.preview(&snapshot, &merged)?;
        let mut document: DocumentMut = source
            .parse()
            .map_err(|error: toml_edit::TomlError| error.to_string())?;
        for (id, selection) in &merged {
            edit_module(
                &mut document,
                id,
                selection.enabled,
                selection.version.as_ref(),
                false,
            )?;
        }
        self.write(&document.to_string())?;
        self.load()
    }

    pub fn set_enabled(&self, id: &str, enabled: bool, version: Option<&VersionReq>) -> Result<()> {
        self.write(&edit_selection(&self.source()?, id, enabled, version)?)
    }

    fn write(&self, source: &str) -> Result<()> {
        fs::write(&self.config_path, source)
            .map_err(|error| format!("保存 {} 失败：{error}", self.config_path.display()))
    }

    pub fn import(&self, path: &Path) -> Result<(Snapshot, usize)> {
        let snapshot = self.load()?;
        let path = std::path::absolute(path).map_err(|error| error.to_string())?;
        let count = mhf_mod_package::import_archive(&path, &snapshot.mods_dir)
            .map_err(|error| error.to_string())?
            .len();
        let snapshot = self
            .load()
            .map_err(|error| format!("导入已完成，刷新列表失败：{error}"))?;
        Ok((snapshot, count))
    }

    #[cfg(feature = "gui")]
    pub fn export(&self, path: &Path) -> Result<usize> {
        let snapshot = self.load()?;
        let resolved = self.preview(&snapshot, &BTreeMap::new())?;
        if resolved.mods.is_empty() {
            return Err("请先启用需要导出的 Mod".into());
        }
        let path = std::path::absolute(path).map_err(|error| error.to_string())?;
        mhf_mod_package::export_archive(&path, &resolved.mods)
            .map_err(|error| error.to_string())?;
        Ok(resolved.mods.len())
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    #[test]
    fn saving_deltas_preserves_fresh_configuration_and_rejects_invalid_combinations() {
        let root = std::env::temp_dir().join(format!("mhf-mods-save-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let path = root.join("mhf.toml");
        let initial = "# user configuration\n[video]\nvolume = 12\n[mods.\"mhf.base\"]\nenabled = false\nversion = '^1'\n[mods.\"mhf.base\".settings]\ncustom = 7 # preserve\n";
        fs::write(&path, initial).unwrap();
        let manager = Manager::new(path.clone(), Some(root.join("mods"))).unwrap();
        let baseline = manager.load().unwrap().config;
        // Another writer changes a game setting and the same Mod's untouched version.
        fs::write(
            &path,
            initial
                .replace("volume = 12", "volume = 42")
                .replace("'^1'", "'=1.0.0'"),
        )
        .unwrap();
        let changed = manager
            .save(
                &baseline,
                &BTreeMap::from([(
                    "mhf.base".into(),
                    Selection {
                        enabled: None,
                        version: Some("^1".parse().unwrap()),
                    },
                )]),
            )
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("volume = 42"));
        assert!(text.contains("# user configuration"));
        assert!(text.contains("custom = 7 # preserve"));
        assert_eq!(changed.config.modules.len(), 1);
        let base = &changed.config.modules["mhf.base"];
        assert_eq!(base.enabled, None);
        assert_eq!(base.version, Some("=1.0.0".parse().unwrap()));
        assert!(
            manager
                .save(
                    &changed.config,
                    &BTreeMap::from([(
                        "missing.mod".into(),
                        Selection {
                            enabled: Some(true),
                            version: None,
                        }
                    )])
                )
                .is_err()
        );
        assert_eq!(fs::read_to_string(path).unwrap(), text);
    }
}

pub(crate) fn edit_selection(
    source: &str,
    id: &str,
    enabled: bool,
    version: Option<&VersionReq>,
) -> Result<String> {
    let mut document: DocumentMut = source
        .parse()
        .map_err(|error: toml_edit::TomlError| error.to_string())?;
    edit_module(&mut document, id, Some(enabled), version, true)?;
    Ok(document.to_string())
}

fn edit_module(
    document: &mut DocumentMut,
    id: &str,
    enabled: Option<bool>,
    version: Option<&VersionReq>,
    keep_version: bool,
) -> Result<()> {
    validate_id(id)?;
    let mods = document
        .entry("mods")
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or("mods 必须是 TOML 表")?;
    let module = mods
        .entry(id)
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or("Mod 配置必须是 TOML 表")?;
    if let Some(enabled) = enabled {
        module.insert("enabled", value(enabled));
    } else {
        module.remove("enabled");
    }
    if let Some(version) = version {
        module.insert("version", value(version.to_string()));
    } else if !keep_version {
        module.remove("version");
    }
    Ok(())
}

pub(crate) fn validate_id(id: &str) -> std::result::Result<(), String> {
    if id == "directory" {
        return Err("directory 是 Mod 目录配置项，不能用作此处的 Mod ID".into());
    }
    if !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(format!("无效 Mod ID：{id}"));
    }
    Ok(())
}
