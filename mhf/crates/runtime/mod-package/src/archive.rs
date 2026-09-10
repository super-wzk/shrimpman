use crate::{
    Candidate, Error, Result, Source, Version, discover,
    manifest::{relative_path, validate_id},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

/// An export receipt, not another source of runtime configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pack {
    pub schema: u32,
    pub mods: Vec<PackEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackEntry {
    pub id: String,
    pub version: Version,
    #[serde(default, skip_serializing_if = "is_false")]
    pub builtin: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl Pack {
    /// Read the exact versions selected when this archive was exported. Import
    /// never applies these selections to mhf.toml automatically.
    pub fn read_archive(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let mut archive = ZipArchive::new(File::open(path)?)?;
        read_pack(&mut archive)
    }
}

/// Export precisely the supplied candidates, including all selected versions'
/// assets. Builtins are recorded in pack.toml and remain supplied by the host.
pub fn export_archive(path: impl AsRef<Path>, selected: &[Candidate]) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut files = Vec::new();
    let mut mods = Vec::new();
    for candidate in selected {
        candidate.manifest.validate()?;
        let manifest = &candidate.manifest;
        if !ids.insert(&manifest.id) {
            return Err(Error::new(format!(
                "{}：每个 Mod 只能选择一个版本导出",
                manifest.id
            )));
        }
        if let Source::Directory(directory) = &candidate.source {
            let actual = Candidate::from_directory(directory)?;
            if actual.manifest != *manifest {
                return Err(Error::new(format!(
                    "{}：Mod 包在选定后发生了变化",
                    manifest.id
                )));
            }
            collect_files(
                directory,
                directory,
                &format!("mods/{}/{}", manifest.id, manifest.version),
                &mut files,
            )?;
        }
        mods.push(PackEntry {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            builtin: candidate.source == Source::Builtin,
        });
    }
    let pack = toml::to_string_pretty(&Pack { schema: 1, mods })
        .map_err(|error| Error::new(format!("生成 pack.toml 失败：{error}")))?;
    // Refuse an existing output, avoiding accidental replacement of a package
    // being exported or another user archive.
    let output = File::options()
        .write(true)
        .create_new(true)
        .open(path.as_ref())?;
    let result = (|| {
        let mut archive = ZipWriter::new(output);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("pack.toml", options)?;
        archive.write_all(pack.as_bytes())?;
        files.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in files {
            archive.start_file(name, options)?;
            io::copy(&mut File::open(path)?, &mut archive)?;
        }
        archive.finish()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn collect_files(
    directory: &Path,
    root: &Path,
    prefix: &str,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(Error::new(format!(
                "{}：Mod 包不支持符号链接",
                entry.path().display()
            )));
        }
        if kind.is_dir() {
            collect_files(&entry.path(), root, prefix, files)?;
        } else if kind.is_file() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|error| Error::new(format!("无法取得包内相对路径：{error}")))?;
            let portable = relative
                .components()
                .map(|part| {
                    part.as_os_str()
                        .to_str()
                        .ok_or_else(|| Error::new("Mod 包路径不是有效的 UTF-8"))
                })
                .collect::<Result<Vec<_>>>()?
                .join("/");
            relative_path(Path::new(&portable))?;
            files.push((format!("{prefix}/{portable}"), path));
        } else {
            return Err(Error::new(format!(
                "{}：Mod 包包含不支持的文件类型",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

/// Import a single package or a collection. Existing versions are never
/// replaced. All paths and manifests are checked in staging before publishing.
pub fn import_archive(
    path: impl AsRef<Path>,
    mods_dir: impl AsRef<Path>,
) -> Result<Vec<Candidate>> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    let pack = read_pack(&mut archive)?;
    fs::create_dir_all(mods_dir.as_ref())?;
    let root = mods_dir.as_ref().canonicalize()?;
    let staging = Staging::create(&root)?;
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().trim_end_matches('/');
        relative_path(Path::new(name))?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(Error::new(format!("ZIP 包内路径重复：{name}")));
        }
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(Error::new(format!("ZIP 包不支持符号链接：{name}")));
        }
        if name == "pack.toml" && !file.is_dir() {
            continue;
        }
        let components = name.split('/').collect::<Vec<_>>();
        if components[0] != "mods" || (!file.is_dir() && components.len() < 4) {
            return Err(Error::new(format!(
                "ZIP 包内路径必须采用 mods/id/version/file 格式：{name}"
            )));
        }
        if let Some(id) = components.get(1) {
            validate_id(id)?;
        }
        if let Some(version) = components.get(2) {
            Version::parse(version)
                .map_err(|error| Error::new(format!("Mod 包目录中的版本号无效：{error}")))?;
        }
        let target = staging.0.join(name);
        if file.is_dir() {
            fs::create_dir_all(target)?;
        } else {
            fs::create_dir_all(target.parent().expect("archive path has a parent"))?;
            let mut output = File::options().write(true).create_new(true).open(target)?;
            io::copy(&mut file, &mut output)?;
        }
    }
    let candidates = discover(staging.0.join("mods"))?;
    if candidates.is_empty()
        && pack
            .as_ref()
            .is_none_or(|pack| pack.mods.is_empty() || pack.mods.iter().any(|item| !item.builtin))
    {
        return Err(Error::new("ZIP 包中没有 Mod 包"));
    }
    if let Some(pack) = &pack {
        validate_pack(pack, &candidates)?;
    }
    // Check the complete destination set before moving any package. In
    // particular, do not follow a pre-existing ID directory symlink.
    for candidate in &candidates {
        let parent = root.join(&candidate.manifest.id);
        match fs::symlink_metadata(&parent) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(Error::new(format!(
                    "{}：应为普通的 Mod 目录，不允许符号链接",
                    parent.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let target = parent.join(candidate.manifest.version.to_string());
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(Error::new(format!(
                    "{} {} 已安装",
                    candidate.manifest.id, candidate.manifest.version
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut published = Vec::new();
    let result = (|| {
        let mut imported = Vec::new();
        for candidate in candidates {
            let Source::Directory(source) = &candidate.source else {
                unreachable!()
            };
            let target = root
                .join(&candidate.manifest.id)
                .join(candidate.manifest.version.to_string());
            fs::create_dir_all(target.parent().expect("package has a parent"))?;
            fs::rename(source, &target)?;
            published.push(target.clone());
            imported.push(Candidate {
                source: Source::Directory(target),
                ..candidate
            });
        }
        Ok(imported)
    })();
    if result.is_err() {
        for path in published {
            let _ = fs::remove_dir_all(path);
        }
    }
    result
}

fn read_pack(archive: &mut ZipArchive<File>) -> Result<Option<Pack>> {
    let mut file = match archive.by_name("pack.toml") {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let pack: Pack = toml::from_str(&text)
        .map_err(|error| Error::new(format!("解析 pack.toml 失败：{error}")))?;
    if pack.schema != 1 {
        return Err(Error::new(format!(
            "不支持 pack.toml 的 schema 版本 {}",
            pack.schema
        )));
    }
    let mut ids = BTreeSet::new();
    for entry in &pack.mods {
        validate_id(&entry.id)?;
        if !ids.insert(&entry.id) {
            return Err(Error::new(format!(
                "{}：pack.toml 中重复选择了同一 Mod",
                entry.id
            )));
        }
    }
    Ok(Some(pack))
}

fn validate_pack(pack: &Pack, candidates: &[Candidate]) -> Result<()> {
    let expected: BTreeSet<_> = pack
        .mods
        .iter()
        .filter(|entry| !entry.builtin)
        .map(|entry| (&entry.id, &entry.version))
        .collect();
    let actual: BTreeSet<_> = candidates
        .iter()
        .map(|candidate| (&candidate.manifest.id, &candidate.manifest.version))
        .collect();
    if expected != actual {
        return Err(Error::new(
            "pack.toml 中选择的 Mod 及版本与 ZIP 包内的 Mod 清单不一致",
        ));
    }
    Ok(())
}

struct Staging(PathBuf);

impl Staging {
    fn create(root: &Path) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = root.join(format!(
                ".import-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
