use crate::runtime::PreparedLaunch;
use mhf_launcher_catalog::BuiltinCatalog;
use mhf_mod_host::Module;
use mhf_mod_package::Candidate;

pub(super) fn catalog() -> BuiltinCatalog {
    mhf_launcher_catalog::builtin_catalog!()
}

/// The application assembles its compiled packages; the game host only sees Mods.
pub(super) fn factory(
    prepared: &PreparedLaunch,
) -> impl FnMut(&Candidate) -> Result<Box<dyn Module>, String> + '_ {
    #[cfg(feature = "base")]
    let registry = mhf_ui::OverlayRegistry::default();
    move |candidate| match candidate.manifest.id.as_str() {
        "mhf.config" => Ok(Box::new(mhf_config::ConfigMod::new(
            prepared.store.clone(),
            mhf_game::runtime::PROFILE.ini_name.to_owned(),
        ))),
        "mhf.dat-redirect" => Ok(Box::new(mhf_dat_redirect::DatRedirectMod::new(
            prepared.game.game_dir.clone(),
        ))),
        #[cfg(feature = "base")]
        "mhf.base" => Ok(Box::new(mhf_base::BaseMod::new(
            &prepared.mhf,
            registry.clone(),
        )?)),
        #[cfg(feature = "login")]
        "mhf.login" => Ok(Box::new(mhf_login::LoginModule::new())),
        #[cfg(feature = "debug")]
        "mhf.debug" => Ok(Box::new(mhf_debug::DebugModule::new(registry.clone()))),
        #[cfg(feature = "workbench")]
        "mhf.workbench" => Ok(Box::new(mhf_workbench::WorkbenchModule::new(
            registry.clone(),
            prepared.game.game_dir.clone(),
        ))),
        id => Err(format!("unknown builtin Mod {id}")),
    }
}

#[cfg(test)]
#[test]
fn registered_candidates_match_the_compiled_factory() {
    let directory = std::env::temp_dir().join(format!("mhf-factory-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("mhf.toml");
    std::fs::write(&path, "").unwrap();
    let prepared = crate::runtime::prepare(Some(path), Some(directory.clone())).unwrap();
    let mut factory = factory(&prepared);
    for candidate in catalog().candidates().unwrap() {
        factory(&candidate).unwrap_or_else(|error| panic!("{}: {error}", candidate.manifest.id));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(all(test, feature = "login", feature = "debug"))]
mod tests {
    use super::*;
    use mhf_mod_host::{ModHost, api};
    #[cfg(feature = "workbench")]
    use std::collections::BTreeMap;
    use std::{fs, sync::Mutex};

    // Config and DatRedirect install process-wide hooks during prepare. Each
    // host owns a complete cycle, so these tests cannot run concurrently.
    static HOST_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(feature = "workbench")]
    #[test]
    fn workbench_launches_locally_and_rejects_two_ordinary_startup_providers() {
        let _host = HOST_LOCK.lock().unwrap();
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-startup-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("mhf.toml");
        for debug in [false, true] {
            fs::write(&path, format!("[mods.'mhf.workbench']\nenabled = true\n[mods.'mhf.debug']\nenabled = {debug}\n")).unwrap();
            let prepared =
                crate::runtime::prepare(Some(path.clone()), Some(directory.clone())).unwrap();
            let catalog = catalog();
            let resolved = catalog
                .resolve(&prepared.game.mods, &catalog.candidates().unwrap())
                .unwrap();
            let mut host = ModHost::load(resolved, &BTreeMap::new(), factory(&prepared)).unwrap();
            host.prepare().unwrap();
            let mut params = api::game::LaunchParams32::default();
            let mut global = api::game::GlobalData32::default();
            let mut target = api::LaunchTargetV1 {
                params: &mut params,
                global: &mut global,
            };
            let result = unsafe { host.launch(&mut target) };
            if debug {
                assert!(result.is_err());
                assert_eq!(params.selected_character_id_1, 0);
            } else {
                assert!(result.unwrap());
                assert_eq!(&params.selected_character_name[..10], b"Workbench\0");
                assert_eq!(params.selected_character_id_1, 1);
            }
            host.stop().unwrap();
            unsafe {
                host.detach(&[]).unwrap();
                host.prepare_release().unwrap();
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dat_redirect_is_independent_of_startup_mode() {
        let _host = HOST_LOCK.lock().unwrap();
        let directory = std::env::temp_dir().join(format!("mhf-startup-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("mhf.toml");
        for (debug, login, redirect_enabled, root) in [
            (true, "", true, None),
            (
                true,
                "[mods.'mhf.login']\nenabled = false\n",
                true,
                Some("custom-dat"),
            ),
            (false, "", true, None),
            (false, "", true, Some("custom-dat")),
            (true, "", false, None),
            (false, "", false, None),
        ] {
            let redirect = root.map_or_else(String::new, |root| {
                format!("[mods.'mhf.dat-redirect'.settings]\nroot = '{root}'\n")
            });
            fs::write(
                &path,
                format!("[mods.'mhf.debug']\nenabled = {debug}\n{login}[mods.'mhf.dat-redirect']\nenabled = {redirect_enabled}\n{redirect}"),
            )
            .unwrap();
            let dat = directory.join("dat");
            let replacement = directory.join(root.unwrap_or("dat-redirect"));
            fs::create_dir_all(&dat).unwrap();
            fs::create_dir_all(&replacement).unwrap();
            fs::write(dat.join("model.bin"), "original").unwrap();
            fs::write(dat.join("fallback.bin"), "fallback").unwrap();
            let replacement_contents = root.unwrap_or("default-dat");
            fs::write(replacement.join("model.bin"), replacement_contents).unwrap();
            let prepared =
                crate::runtime::prepare(Some(path.clone()), Some(directory.clone())).unwrap();
            let catalog = catalog();
            let candidates = catalog.candidates().unwrap();
            let resolved = catalog.resolve(&prepared.game.mods, &candidates).unwrap();
            assert_eq!(
                resolved
                    .mods
                    .iter()
                    .any(|candidate| candidate.manifest.id == "mhf.login"),
                login.is_empty()
            );
            assert_eq!(
                resolved
                    .mods
                    .iter()
                    .any(|candidate| candidate.manifest.id == "mhf.dat-redirect"),
                redirect_enabled,
            );
            let configuration = prepared
                .game
                .mods
                .modules
                .iter()
                .map(|(id, settings)| (id.clone(), toml::to_string(&settings.settings).unwrap()))
                .collect();
            let mut host = ModHost::load(resolved, &configuration, factory(&prepared)).unwrap();
            host.prepare().unwrap();
            assert_eq!(
                fs::read_to_string(dat.join("model.bin")).unwrap(),
                if redirect_enabled {
                    replacement_contents
                } else {
                    "original"
                }
            );
            assert_eq!(
                fs::read_to_string(dat.join("fallback.bin")).unwrap(),
                "fallback"
            );
            if debug {
                let mut params = api::game::LaunchParams32::default();
                let mut global = api::game::GlobalData32::default();
                let mut target = api::LaunchTargetV1 {
                    params: &mut params,
                    global: &mut global,
                };
                assert!(unsafe { host.launch(&mut target) }.unwrap());
                assert_eq!(params.selected_character_id_1, 1);
                assert_eq!(&params.selected_character_name[..6], b"Debug\0");
            }
            host.stop().unwrap();
            unsafe {
                host.detach(&[]).unwrap();
                host.prepare_release().unwrap();
            }
            drop(host);
            assert_eq!(
                fs::read_to_string(dat.join("model.bin")).unwrap(),
                "original"
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
