use super::*;
use mhf_config::{ConfigService, Store, bind};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn store(source: &str) -> (Store, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "mhf-base-config-{}-{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, source).unwrap();
    (Store::load(path.clone()).unwrap(), path)
}

#[test]
fn base_defaults_and_fixed_hd_match_the_ini_view_without_rewriting_the_file() {
    let source = "[video]\ngraphics_version = 'standard'\n[screen]\nmode = 'windowed'\nwindow_resolution = { width = 1366, height = 768 }\n[localization]\nlanguage = 'korean'\n[other.owner]\nenabled = true\n";
    let (store, path) = store(source);
    let store = Arc::new(Mutex::new(store));
    let service = ConfigService::new(store.clone());
    let api = unsafe { bind(service.api()) };
    let config = register_config(api).unwrap();
    assert_eq!(register_config(api).unwrap(), config);
    let store = store.lock().unwrap();
    assert_eq!(
        config.video.graphics_version,
        GraphicsVersion::HighDefinition
    );
    assert_eq!(config.screen.mode, ScreenMode::Windowed);
    assert_eq!(
        config.screen.window_resolution,
        Resolution {
            width: 1366,
            height: 768
        }
    );
    assert_eq!(config.screen.fullscreen_resolution, Resolution::default());
    assert_eq!(config.localization.language, Language::Korean);
    assert!(config.option.draw_skip);
    assert_eq!(store.value("VIDEO", "GRAPHICS_VER").as_deref(), Some("1"));
    assert_eq!(
        store.value("SCREEN", "FULLSCREEN_MODE").as_deref(),
        Some("0")
    );
    assert_eq!(
        store.value("SCREEN", "WINDOW_RESOLUTION_W").as_deref(),
        Some("1366")
    );
    assert_eq!(
        store.value("LOCALIZATION", "LANGUAGE").as_deref(),
        Some("6")
    );
    assert_eq!(
        store.value("FONT", "NAME").as_deref(),
        Some(mhf_font::FAMILY_NAME)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    fs::remove_file(path).unwrap();
}

#[test]
fn base_rejects_values_outside_its_own_domain_types() {
    let source = "[font]\nweight = 65536\n";
    let (store, path) = store(source);
    let service = ConfigService::new(Arc::new(Mutex::new(store)));
    let api = unsafe { bind(service.api()) };
    assert!(register_config(api).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    fs::remove_file(path).unwrap();
}
