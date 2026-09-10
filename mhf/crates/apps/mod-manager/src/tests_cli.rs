use super::*;

#[test]
fn toggles_preserve_settings_other_sections_and_comments() {
    let source = r#"# launcher comment
[game]
directory = "game" # unchanged

[mods]
directory = "packages"

[mods."example.counter"]
enabled = false
version = "^1.0"

[mods."example.counter".settings]
label = "计数器" # provider setting

[mods."other.mod"]
enabled = true
"#;
    let changed = edit_selection(source, "example.counter", true, None).unwrap();
    assert!(changed.contains("# launcher comment"));
    assert!(changed.contains("directory = \"game\" # unchanged"));
    assert!(changed.contains("label = \"计数器\" # provider setting"));
    let config: ConfigFile = toml::from_str(&changed).unwrap();
    assert_eq!(config.mods.directory, PathBuf::from("packages"));
    assert_eq!(config.mods.modules["example.counter"].enabled, Some(true));
    assert_eq!(
        config.mods.modules["example.counter"]
            .version
            .as_ref()
            .unwrap()
            .to_string(),
        "^1.0"
    );
    assert_eq!(config.mods.modules["other.mod"].enabled, Some(true));
    let changed = edit_selection(
        &changed,
        "example.counter",
        false,
        Some(&"=1.2.3".parse().unwrap()),
    )
    .unwrap();
    let config: ConfigFile = toml::from_str(&changed).unwrap();
    assert_eq!(config.mods.modules["example.counter"].enabled, Some(false));
    assert_eq!(
        config.mods.modules["example.counter"]
            .version
            .as_ref()
            .unwrap()
            .to_string(),
        "=1.2.3"
    );
}

#[test]
fn adding_a_mod_preserves_an_inline_configuration() {
    let source =
        "mods = { directory = 'packages', 'existing.mod' = { enabled = false } }\nother = 42\n";
    let changed = edit_selection(source, "new.mod", true, None).unwrap();
    let document: toml::Table = toml::from_str(&changed).unwrap();
    assert_eq!(document["other"].as_integer(), Some(42));
    let config: ConfigFile = toml::from_str(&changed).unwrap();
    assert_eq!(config.mods.modules["new.mod"].enabled, Some(true));
    assert_eq!(config.mods.modules["existing.mod"].enabled, Some(false));
}

#[test]
fn explicit_export_roots_keep_dependency_pins_and_disables() {
    let config: ConfigFile = toml::from_str(
        r#"
[mods."unrelated"]
enabled = true
[mods."provider"]
version = "^1"
[mods."disabled"]
enabled = false
"#,
    )
    .unwrap();
    let requests = vec!["consumer@~2.1".parse().unwrap()];
    let selections = export_selections(&config.mods, &requests);
    assert_eq!(selections["unrelated"].enabled, None);
    assert_eq!(
        selections["provider"].version.as_ref().unwrap().to_string(),
        "^1"
    );
    assert_eq!(selections["disabled"].enabled, Some(false));
    assert_eq!(selections["consumer"].enabled, Some(true));
    assert_eq!(
        selections["consumer"].version.as_ref().unwrap().to_string(),
        "~2.1"
    );
}
