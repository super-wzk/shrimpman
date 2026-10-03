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
    let baseline = manager.load().unwrap().config.selections();
    // 另一个写入者更新了游戏设置，以及本次编辑未修改的版本字段。
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
                &changed.config.selections(),
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
