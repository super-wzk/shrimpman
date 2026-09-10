use crate::{Error, Result};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Native,
    Data,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub version: Version,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, VersionReq>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let manifest: Self = toml::from_str(text)
            .map_err(|error| Error::new(format!("解析 mod.toml 失败：{error}")))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 {
            return Err(Error::new(format!(
                "{}：不支持 mod.toml 的 schema 版本 {}",
                self.id, self.schema
            )));
        }
        validate_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err(Error::new(format!("{}：缺少显示名称", self.id)));
        }
        for dependency in self.dependencies.keys() {
            validate_id(dependency)?;
        }
        if let Some(entry) = &self.entry {
            relative_path(entry)?;
            if self.kind == Kind::Data {
                return Err(Error::new(format!(
                    "{}：数据 Mod 不能设置 DLL 入口（entry）",
                    self.id
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    Builtin,
    /// Directory containing this version's mod.toml.
    Directory(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub manifest: Manifest,
    pub source: Source,
}

impl Candidate {
    pub fn builtin(manifest: Manifest) -> Result<Self> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            source: Source::Builtin,
        })
    }

    pub fn from_directory(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().canonicalize()?;
        let manifest = Manifest::parse(&fs::read_to_string(directory.join("mod.toml"))?)?;
        if manifest.kind == Kind::Native && manifest.entry.is_none() {
            return Err(Error::new(format!(
                "{}：原生 Mod 包必须设置入口字段 entry",
                manifest.id
            )));
        }
        if let Some(entry) = &manifest.entry {
            let path = directory.join(entry);
            if !path.is_file() || !path.canonicalize()?.starts_with(&directory) {
                return Err(Error::new(format!(
                    "{}：DLL 入口文件不存在或位于 Mod 包目录之外",
                    manifest.id
                )));
            }
        }
        Ok(Self {
            manifest,
            source: Source::Directory(directory),
        })
    }
}

/// Read packages from `<mods_dir>/<id>/<version>/mod.toml` without loading DLLs.
pub fn discover(mods_dir: impl AsRef<Path>) -> Result<Vec<Candidate>> {
    let mods_dir = mods_dir.as_ref();
    if !mods_dir.exists() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for id in fs::read_dir(mods_dir)? {
        let id = id?;
        if !id.file_type()?.is_dir() || id.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        for version in fs::read_dir(id.path())? {
            let version = version?;
            if !version.file_type()?.is_dir() {
                continue;
            }
            let candidate = Candidate::from_directory(version.path())?;
            if id.file_name().to_str() != Some(candidate.manifest.id.as_str())
                || version.file_name().to_str()
                    != Some(candidate.manifest.version.to_string().as_str())
            {
                return Err(Error::new(format!(
                    "{}：Mod 包目录必须与 mod.toml 中的 id/version 一致",
                    version.path().display()
                )));
            }
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|a, b| {
        a.manifest
            .id
            .cmp(&b.manifest.id)
            .then(a.manifest.version.cmp(&b.manifest.version))
    });
    Ok(candidates)
}

pub(crate) fn validate_id(id: &str) -> Result<()> {
    if !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(Error::new(format!("Mod ID 无效：{id}")));
    }
    Ok(())
}

/// Restrict stored paths using portable rules, including when read on Unix.
pub(crate) fn relative_path(path: &Path) -> Result<&str> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::new("Mod 包路径不是有效的 UTF-8"))?;
    if text.is_empty()
        || text.contains(['\\', ':'])
        || text
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(Error::new(format!("Mod 包的相对路径无效：{text}")));
    }
    Ok(text)
}
