use crate::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn candidate(id: &str, version: &str, dependencies: &[(&str, &str)]) -> Candidate {
    Candidate::builtin(Manifest {
        schema: 1,
        id: id.into(),
        name: id.into(),
        version: version.parse().unwrap(),
        kind: Kind::Native,
        entry: None,
        dependencies: dependencies
            .iter()
            .map(|(id, version)| ((*id).into(), version.parse().unwrap()))
            .collect(),
    })
    .unwrap()
}

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).into()).collect()
}

#[test]
fn runtime_config_separates_selection_from_provider_settings() {
    let config: RuntimeConfig = toml::from_str(
        r#"
directory = "packages"
["example.counter"]
enabled = true
version = "^1.0"
["example.counter".settings]
step = 3
"#,
    )
    .unwrap();
    let settings = &config.modules["example.counter"];
    assert_eq!(settings.enabled, Some(true));
    assert_eq!(settings.settings["step"].as_integer(), Some(3));
    assert_eq!(
        config.selections()["example.counter"]
            .version
            .as_ref()
            .unwrap()
            .to_string(),
        "^1.0"
    );
    let empty: RuntimeConfig = toml::from_str("").unwrap();
    assert_eq!(empty.directory, PathBuf::from("mods"));
    assert!(empty.modules.is_empty());
}

#[test]
fn chooses_older_provider_when_a_later_consumer_requires_it() {
    // a is chosen first at 2.0, then z constrains it to 1.x. Resolution must
    // backtrack to a's choice rather than report the initial conflict.
    let candidates = [
        candidate("a", "2.0.0", &[]),
        candidate("a", "1.4.0", &[]),
        candidate("z", "1.0.0", &[("a", "^1")]),
    ];
    let resolved = resolve(
        &candidates,
        &BTreeMap::new(),
        &ids(&["a", "z"]),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        resolved
            .mods
            .iter()
            .map(|m| (m.manifest.id.as_str(), m.manifest.version.to_string()))
            .collect::<Vec<_>>(),
        [("a", "1.4.0".into()), ("z", "1.0.0".into())]
    );
}

#[test]
fn backtracks_consumer_version_for_transitive_dependencies() {
    let candidates = [
        candidate("app", "2.0.0", &[("middle", "^2")]),
        candidate("app", "1.0.0", &[("middle", "^1")]),
        candidate("middle", "2.0.0", &[("base", "^2")]),
        candidate("middle", "1.0.0", &[("base", "^1")]),
        candidate("base", "1.0.0", &[]),
    ];
    let resolved = resolve(
        &candidates,
        &BTreeMap::new(),
        &ids(&["app"]),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        resolved
            .mods
            .iter()
            .map(|m| m.manifest.id.as_str())
            .collect::<Vec<_>>(),
        ["base", "middle", "app"]
    );
    assert!(
        resolved
            .mods
            .iter()
            .all(|m| m.manifest.version == Version::new(1, 0, 0))
    );
}

#[test]
fn rejects_incompatible_consumers_and_explicitly_disabled_dependencies() {
    let candidates = [
        candidate("a", "1.0.0", &[]),
        candidate("a", "2.0.0", &[]),
        candidate("b", "1.0.0", &[("a", "^1")]),
        candidate("c", "1.0.0", &[("a", "^2")]),
    ];
    assert!(
        resolve(
            &candidates,
            &BTreeMap::new(),
            &ids(&["b", "c"]),
            &BTreeSet::new(),
        )
        .is_err()
    );
    let selections = BTreeMap::from([(
        "a".into(),
        Selection {
            enabled: Some(false),
            version: None,
        },
    )]);
    assert!(resolve(&candidates, &selections, &ids(&["b"]), &BTreeSet::new()).is_err());
    assert!(resolve(&candidates, &selections, &BTreeSet::new(), &ids(&["a"])).is_err());
    assert!(
        resolve(&candidates, &selections, &ids(&["a"]), &BTreeSet::new())
            .unwrap()
            .mods
            .is_empty()
    );
}

#[test]
fn cycles_are_rejected_and_can_cause_version_backtracking() {
    let mut candidates = vec![
        candidate("a", "2.0.0", &[("b", "^1")]),
        candidate("b", "1.0.0", &[("a", ">=1")]),
    ];
    assert!(
        resolve(
            &candidates,
            &BTreeMap::new(),
            &ids(&["a"]),
            &BTreeSet::new(),
        )
        .is_err()
    );
    candidates.push(candidate("a", "1.0.0", &[]));
    assert_eq!(
        resolve(
            &candidates,
            &BTreeMap::new(),
            &ids(&["a"]),
            &BTreeSet::new()
        )
        .unwrap()
        .mods[0]
            .manifest
            .version,
        Version::new(1, 0, 0)
    );
}

#[test]
fn selection_supports_explicit_prereleases_and_rejects_duplicate_sources() {
    let candidates = [candidate("a", "1.0.0-beta.1", &[])];
    let selections = BTreeMap::from([(
        "a".into(),
        Selection {
            enabled: Some(true),
            version: Some("=1.0.0-beta.1".parse().unwrap()),
        },
    )]);
    assert_eq!(
        resolve(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new())
            .unwrap()
            .mods
            .len(),
        1
    );
    let duplicate = [candidates[0].clone(), candidates[0].clone()];
    assert!(resolve(&duplicate, &selections, &BTreeSet::new(), &BTreeSet::new()).is_err());
}

fn data_package(root: &Path, id: &str, version: &str) -> Candidate {
    let path = root.join(id).join(version);
    fs::create_dir_all(path.join("assets")).unwrap();
    let mut manifest = candidate(id, version, &[]).manifest;
    manifest.kind = Kind::Data;
    fs::write(path.join("mod.toml"), toml::to_string(&manifest).unwrap()).unwrap();
    fs::write(path.join("assets/text.txt"), "字典内容").unwrap();
    Candidate::from_directory(path).unwrap()
}

#[test]
fn archive_round_trip_preserves_exact_selection_and_builtin_receipt() {
    let temp = Temp::new();
    let first = data_package(&temp.0.join("source"), "example.text", "1.0.0");
    let _second = data_package(&temp.0.join("source"), "example.text", "2.0.0");
    let builtin = candidate("mhf.base", "1.2.0", &[]);
    let archive = temp.0.join("pack.zip");
    export_archive(&archive, &[first, builtin]).unwrap();
    let pack = Pack::read_archive(&archive).unwrap().unwrap();
    assert_eq!(pack.mods.len(), 2);
    assert!(pack.mods[1].builtin);
    let imported = import_archive(&archive, temp.0.join("imported")).unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].manifest.version, Version::new(1, 0, 0));
    assert_eq!(
        fs::read_to_string(temp.0.join("imported/example.text/1.0.0/assets/text.txt")).unwrap(),
        "字典内容"
    );
    assert!(!temp.0.join("imported/example.text/2.0.0").exists());
    assert!(!temp.0.join("imported/pack.toml").exists());
    assert!(import_archive(&archive, temp.0.join("imported")).is_err());
}

fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    for (name, body) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn rejects_archive_traversal_and_portable_absolute_paths_without_publishing() {
    let temp = Temp::new();
    for (index, name) in [
        "../escape",
        "mods/a/1.0.0/../../../../escape",
        "C:/escape",
        "mods/a/1.0.0/..\\escape",
        "/escape",
    ]
    .iter()
    .enumerate()
    {
        let archive = temp.0.join(format!("{index}.zip"));
        write_zip(&archive, &[(name, "bad")]);
        assert!(
            import_archive(&archive, temp.0.join("installed")).is_err(),
            "{name}"
        );
        assert_eq!(fs::read_dir(temp.0.join("installed")).unwrap().count(), 0);
    }
    assert!(!temp.0.join("escape").exists());
}

#[test]
fn validates_manifest_identity_and_entry_before_import() {
    let temp = Temp::new();
    let mut manifest = candidate("a", "1.0.0", &[]).manifest;
    manifest.kind = Kind::Data;
    let archive = temp.0.join("mismatch.zip");
    write_zip(
        &archive,
        &[(
            "mods/b/1.0.0/mod.toml",
            &toml::to_string(&manifest).unwrap(),
        )],
    );
    assert!(import_archive(&archive, temp.0.join("mods")).is_err());
    manifest.kind = Kind::Native;
    manifest.entry = Some(PathBuf::from("../elsewhere.dll"));
    assert!(manifest.validate().is_err());
    assert_eq!(fs::read_dir(temp.0.join("mods")).unwrap().count(), 0);
}

#[test]
fn existing_version_prevents_partial_pack_import() {
    let temp = Temp::new();
    let a = data_package(&temp.0.join("source"), "a", "1.0.0");
    let z = data_package(&temp.0.join("source"), "z", "1.0.0");
    data_package(&temp.0.join("installed"), "z", "1.0.0");
    let archive = temp.0.join("pack.zip");
    export_archive(&archive, &[a, z]).unwrap();
    assert!(import_archive(&archive, temp.0.join("installed")).is_err());
    assert!(!temp.0.join("installed/a").exists());
    assert_eq!(discover(temp.0.join("installed")).unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn rejects_existing_destination_symlink() {
    let temp = Temp::new();
    let candidate = data_package(&temp.0.join("source"), "a", "1.0.0");
    let archive = temp.0.join("pack.zip");
    export_archive(&archive, &[candidate]).unwrap();
    fs::create_dir_all(temp.0.join("installed")).unwrap();
    fs::create_dir_all(temp.0.join("outside")).unwrap();
    std::os::unix::fs::symlink(temp.0.join("outside"), temp.0.join("installed/a")).unwrap();
    assert!(import_archive(&archive, temp.0.join("installed")).is_err());
    assert!(!temp.0.join("outside/1.0.0").exists());
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "mhf-mods-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
