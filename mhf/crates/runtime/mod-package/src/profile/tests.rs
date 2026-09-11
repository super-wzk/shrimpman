use super::*;
use crate::Source;

const CATALOG: BuiltinCatalog = BuiltinCatalog {
    login: true,
    debug: true,
    workbench: false,
};

fn plan(catalog: BuiltinCatalog, text: &str) -> Result<Resolved> {
    let config = toml::from_str(text).unwrap();
    let candidates = catalog.candidates()?;
    catalog.resolve(&config, &candidates)
}

fn selected(plan: &Resolved, id: &str) -> bool {
    plan.mods
        .iter()
        .any(|candidate| candidate.manifest.id == id)
}

fn position(plan: &Resolved, id: &str) -> usize {
    plan.mods
        .iter()
        .position(|candidate| candidate.manifest.id == id)
        .unwrap()
}

fn add_external(candidates: &mut Vec<Candidate>, id: &str) {
    let mut candidate = candidates
        .iter()
        .find(|candidate| candidate.manifest.id == id)
        .unwrap()
        .clone();
    candidate.manifest.version = Version::new(1, 1, 0);
    candidate.manifest.entry = Some("mod.dll".into());
    candidate.source = Source::Directory(format!("mods/{id}/1.1.0").into());
    candidates.push(candidate);
}

#[test]
fn default_login_pulls_base_through_its_declared_dependencies() {
    let resolved = plan(CATALOG, "").unwrap();
    assert_eq!(
        resolved
            .mods
            .iter()
            .map(|candidate| candidate.manifest.id.as_str())
            .collect::<Vec<_>>(),
        ["mhf.config", "mhf.base", "mhf.login"]
    );
    assert!(plan(CATALOG, "['mhf.base']\nenabled = false").is_err());
    let resolved = plan(CATALOG, "['mhf.login']\nenabled = false").unwrap();
    assert!(resolved.mods.is_empty());
    let resolved = plan(
        CATALOG,
        "['mhf.login']\nenabled = false\n['mhf.base']\nenabled = false",
    )
    .unwrap();
    assert!(resolved.mods.is_empty());
}

#[test]
fn explicit_debug_keeps_default_login_and_adds_its_dependencies() {
    let resolved = plan(CATALOG, "['mhf.debug']\nenabled = true").unwrap();
    for id in ["mhf.base", "mhf.login", "mhf.debug"] {
        assert!(selected(&resolved, id), "{id}");
    }
    assert!(position(&resolved, "mhf.base") < position(&resolved, "mhf.debug"));
    // Choosing the startup callback is a host concern; resolution keeps both.
    let resolved = plan(
        CATALOG,
        "['mhf.debug']\nenabled = true\n['mhf.login']\nenabled = false",
    )
    .unwrap();
    assert!(selected(&resolved, "mhf.base"));
    assert!(selected(&resolved, "mhf.config"));
    assert!(selected(&resolved, "mhf.debug"));
    assert!(!selected(&resolved, "mhf.login"));
    assert!(
        plan(
            CATALOG,
            "['mhf.debug']\nenabled = true\n['mhf.login']\nenabled = false\n['mhf.base']\nenabled = false",
        )
        .is_err()
    );
}

#[test]
fn the_catalog_only_exposes_compiled_packages() {
    let minimal = BuiltinCatalog {
        login: false,
        debug: false,
        workbench: false,
    };
    let candidates = minimal.candidates().unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].manifest.id, "mhf.config");
    assert!(plan(minimal, "").unwrap().mods.is_empty());
    assert_eq!(
        plan(minimal, "['mhf.base']\nenabled = true")
            .unwrap()
            .mods
            .len(),
        2
    );
    for id in ["mhf.login", "mhf.debug", "mhf.quest"] {
        assert!(plan(minimal, &format!("['{id}']\nenabled = true")).is_err());
    }
    let ids: BTreeSet<_> = CATALOG
        .candidates()
        .unwrap()
        .into_iter()
        .map(|candidate| candidate.manifest.id)
        .collect();
    for former_component in ["mhf.font", "mhf.ui", "mhf.geometry", "mhf.quest"] {
        assert!(
            !ids.contains(former_component),
            "{former_component} must remain inside Base"
        );
    }
}

#[test]
fn workbench_is_explicit_and_shares_base_without_pulling_debug() {
    let catalog = BuiltinCatalog {
        workbench: true,
        ..CATALOG
    };
    let ordinary = plan(catalog, "").unwrap();
    assert!(!selected(&ordinary, "mhf.workbench"));
    let workbench = plan(catalog, "['mhf.workbench']\nenabled = true").unwrap();
    for id in ["mhf.config", "mhf.base", "mhf.workbench"] {
        assert!(selected(&workbench, id));
    }
    assert!(!selected(&workbench, "mhf.debug"));
    assert!(position(&workbench, "mhf.base") < position(&workbench, "mhf.workbench"));
    assert!(plan(CATALOG, "['mhf.workbench']\nenabled = true").is_err());
}

#[test]
fn external_base_can_replace_builtin_support() {
    let mut candidates = CATALOG.candidates().unwrap();
    add_external(&mut candidates, "mhf.base");
    for text in ["", "['mhf.debug']\nenabled = true"] {
        let config: RuntimeConfig = toml::from_str(text).unwrap();
        let resolved = CATALOG.resolve(&config, &candidates).unwrap();
        assert!(matches!(
            resolved.mods[position(&resolved, "mhf.base")].source,
            Source::Directory(_)
        ));
        assert!(selected(&resolved, "mhf.login"));
    }
}

#[test]
fn configuration_provider_has_no_consumer_dependencies() {
    let candidates = CATALOG.candidates().unwrap();
    let provider = candidates
        .iter()
        .find(|candidate| candidate.manifest.id == "mhf.config")
        .unwrap();
    assert!(provider.manifest.dependencies.is_empty());
    let login = candidates
        .iter()
        .find(|candidate| candidate.manifest.id == "mhf.login")
        .unwrap();
    assert_eq!(
        login
            .manifest
            .dependencies
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["mhf.base", "mhf.config"]
    );
    let base = candidates
        .iter()
        .find(|candidate| candidate.manifest.id == "mhf.base")
        .unwrap();
    assert!(base.manifest.dependencies.contains_key("mhf.config"));
}
