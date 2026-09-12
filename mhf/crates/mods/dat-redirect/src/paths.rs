use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

pub(crate) struct Paths {
    dat: PathBuf,
    root: PathBuf,
}

impl Paths {
    pub(crate) fn new(game_dir: &Path, root: &Path) -> Result<Self, String> {
        if root.as_os_str().is_empty() {
            return Err("dat-redirect root must not be empty".into());
        }
        if !root.is_absolute()
            && (root.has_root() || matches!(root.components().next(), Some(Component::Prefix(_))))
        {
            return Err("dat-redirect root must be absolute or relative to the game directory without a drive or root prefix".into());
        }
        let dat = normalized(&game_dir.join("dat")).ok_or("invalid game dat path")?;
        let root = normalized(&game_dir.join(root)).ok_or("invalid dat-redirect root")?;
        Ok(Self { dat, root })
    }

    pub(crate) fn replacement(&self, path: &Path) -> Option<PathBuf> {
        // Resolve against the actual current directory, just like CreateFile.
        // No filesystem lookup: an override may supply a missing original file.
        let path = normalized(path)?;
        let relative = relative_to(&path, &self.dat)?;
        if relative.as_os_str().is_empty() {
            return None;
        }
        if verbatim(&path) {
            let ordinary_root = !verbatim(&self.root);
            for component in path.components().skip(self.dat.components().count()) {
                match component {
                    Component::CurDir | Component::ParentDir => return None,
                    // Joining an ordinary path would reinterpret these
                    // literal verbatim names instead of opening the same file.
                    Component::Normal(name)
                        if name.encode_wide().any(|unit| unit == u16::from(b'/'))
                            || ordinary_root
                                && matches!(name.encode_wide().last(), Some(0x20 | 0x2e)) =>
                    {
                        return None;
                    }
                    _ => {}
                }
            }
        }
        Some(self.root.join(relative))
    }
}

fn normalized(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    if matches!(absolute.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::DeviceNS(_) | Prefix::Verbatim(_)))
    {
        return None;
    }
    // Windows absolute() already normalizes ordinary paths and intentionally
    // preserves verbatim paths, whose dot components are literal.
    Some(absolute)
}

fn verbatim(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim())
}

fn relative_to<'a>(path: &'a Path, base: &Path) -> Option<&'a Path> {
    if let Ok(relative) = path.strip_prefix(base) {
        return Some(relative);
    }
    // Rust Path equality is case-sensitive even on Windows. Compare native
    // components with Windows ordinal casing, never a textual path prefix.
    let mut components = path.components();
    for expected in base.components() {
        let actual = components.next()?;
        let matches = match (actual, expected) {
            (Component::Normal(a), Component::Normal(b)) => same_name(a, b),
            (Component::Prefix(a), Component::Prefix(b)) => match (a.kind(), b.kind()) {
                (
                    Prefix::Disk(a) | Prefix::VerbatimDisk(a),
                    Prefix::Disk(b) | Prefix::VerbatimDisk(b),
                ) => a.eq_ignore_ascii_case(&b),
                (
                    Prefix::UNC(a, b) | Prefix::VerbatimUNC(a, b),
                    Prefix::UNC(c, d) | Prefix::VerbatimUNC(c, d),
                ) => same_name(a, c) && same_name(b, d),
                _ => a == b,
            },
            _ => actual == expected,
        };
        if !matches {
            return None;
        }
    }
    Some(components.as_path())
}

fn same_name(a: &OsStr, b: &OsStr) -> bool {
    let a: Vec<_> = a.encode_wide().collect();
    let b: Vec<_> = b.encode_wide().collect();
    unsafe {
        CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
            == CSTR_EQUAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_components_and_normalizes_windows_paths() {
        let paths = Paths::new(Path::new(r"C:\game"), Path::new(r"D:\替换资源")).unwrap();
        for path in [
            r"C:\game\dat\em\model.bin",
            r"C:/game/dat/./em/model.bin",
            r"c:\GAME\DAT\other\..\em\model.bin",
            r"\\?\C:\game\dat\em\model.bin",
        ] {
            assert_eq!(
                paths.replacement(Path::new(path)),
                Some(PathBuf::from(r"D:\替换资源\em\model.bin")),
                "{path}"
            );
        }
        for path in [
            r"C:\game\database\model.bin",
            r"C:\other\dat\model.bin",
            r"C:\game\dat\..\model.bin",
            r"C:\game\dat",
            r"\\.\pipe\dat\model.bin",
            r"\\?\C:\game\dat\em\..\model.bin",
            r"\\?\C:\game\dat\model.bin.",
            r"\\?\C:\game\dat\em.\model.bin",
            r"\\?\C:\game\dat\em/model.bin",
        ] {
            assert_eq!(paths.replacement(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn roots_have_an_unambiguous_base_and_verbatim_names_keep_their_meaning() {
        let game = Path::new(r"C:\game");
        for root in ["", r"C:overrides", r"\overrides"] {
            assert!(Paths::new(game, Path::new(root)).is_err(), "{root}");
        }
        let paths = Paths::new(game, Path::new(r"\\?\D:\overrides")).unwrap();
        assert_eq!(
            paths.replacement(Path::new(r"\\?\C:\game\dat\model.bin.")),
            Some(PathBuf::from(r"\\?\D:\overrides\model.bin."))
        );
    }

    #[test]
    fn resolves_relative_roots_and_requests_without_requiring_files() {
        let cwd = std::env::current_dir().unwrap();
        let paths = Paths::new(&cwd, Path::new("overrides")).unwrap();
        assert_eq!(
            paths.replacement(Path::new(r"dat\missing.bin")),
            Some(cwd.join(r"overrides\missing.bin"))
        );
        let paths = Paths::new(
            Path::new(r"\\server\share\game"),
            Path::new(r"D:\overrides"),
        )
        .unwrap();
        assert_eq!(
            paths.replacement(Path::new(r"\\?\UNC\SERVER\SHARE\game\dat\a.bin")),
            Some(PathBuf::from(r"D:\overrides\a.bin"))
        );
    }
}
