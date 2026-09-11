use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// The physical file remains the source of truth; extension labels are hints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub size: u64,
    pub header: Vec<u8>,
}

impl Entry {
    pub fn matches(&self, query: &str) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        let path = self.relative_path.to_string_lossy().to_lowercase();
        let query = query.to_lowercase();
        query.split_whitespace().all(|word| path.contains(word))
    }

    pub fn encoding(&self) -> &'static str {
        if self.header.starts_with(b"ecd\x1a") {
            "ECD"
        } else if self.header.starts_with(b"exf\x1a") {
            "EXF"
        } else if self.header.starts_with(b"JKR\x1a") {
            "JKR"
        } else {
            "原始资源"
        }
    }
}

#[derive(Default, Debug)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub errors: Vec<(PathBuf, String)>,
}

impl Catalog {
    /// Called by the workbench's I/O worker. Symlinks are not followed, so an
    /// asset directory cannot introduce a recursive scan or leave its root.
    pub fn scan(root: &Path, cancelled: &AtomicBool) -> io::Result<Self> {
        let mut result = Self::default();
        let mut directories = vec![root.to_owned()];
        while let Some(directory) = directories.pop() {
            if cancelled.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "资源扫描已取消"));
            }
            let children = match fs::read_dir(&directory) {
                Ok(children) => children,
                Err(error) if directory == root => return Err(error),
                Err(error) => {
                    result.errors.push((directory, error.to_string()));
                    continue;
                }
            };
            for child in children {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "资源扫描已取消"));
                }
                let child = match child {
                    Ok(child) => child,
                    Err(error) => {
                        result.errors.push((directory.clone(), error.to_string()));
                        continue;
                    }
                };
                let path = child.path();
                let file_type = match child.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        result.errors.push((path, error.to_string()));
                        continue;
                    }
                };
                if file_type.is_symlink() {
                    continue;
                }
                let metadata = match child.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        result.errors.push((path, error.to_string()));
                        continue;
                    }
                };
                if metadata.is_dir() {
                    directories.push(path);
                } else if metadata.is_file() {
                    let mut header = Vec::with_capacity(16);
                    match fs::File::open(&path)
                        .and_then(|file| file.take(16).read_to_end(&mut header))
                    {
                        Ok(_) => result.entries.push(Entry {
                            relative_path: path.strip_prefix(root).unwrap_or(&path).to_owned(),
                            path,
                            size: metadata.len(),
                            header,
                        }),
                        Err(error) => result.errors.push((path, error.to_string())),
                    }
                }
            }
        }
        result
            .entries
            .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_every_term_without_changing_asset_paths() {
        let entry = Entry {
            path: "root/dat/emmodel-hd/em094_b-hd.pac".into(),
            relative_path: "emmodel-hd/em094_b-hd.pac".into(),
            size: 0,
            header: Vec::new(),
        };
        assert!(entry.matches("EM094 HD"));
        assert!(!entry.matches("em094 motion"));
        assert!(entry.matches("  "));
        assert_eq!(
            entry.relative_path,
            PathBuf::from("emmodel-hd/em094_b-hd.pac")
        );
    }
}
