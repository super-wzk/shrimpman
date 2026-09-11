use crate::runtime::PreparedLaunch;
use mhf_mod_host::Module;
use mhf_mod_package::{BuiltinCatalog, Candidate};

pub(super) fn catalog() -> BuiltinCatalog {
    BuiltinCatalog {
        login: cfg!(feature = "login"),
        debug: cfg!(feature = "debug"),
        workbench: cfg!(feature = "workbench"),
    }
}

/// The application assembles its compiled packages; the game host only sees Mods.
pub(super) fn factory(
    prepared: &PreparedLaunch,
) -> impl FnMut(&Candidate) -> Result<Box<dyn Module>, String> + '_ {
    #[cfg(any(feature = "debug", feature = "workbench"))]
    let mut registry = None;
    move |candidate| match candidate.manifest.id.as_str() {
        "mhf.config" => Ok(Box::new(mhf_config::ConfigMod::new(
            prepared.store.clone(),
            mhf_game::runtime::PROFILE.ini_name.to_owned(),
        ))),
        "mhf.base" => {
            let base = mhf_base::BaseMod::new(&prepared.mhf)?;
            #[cfg(any(feature = "debug", feature = "workbench"))]
            {
                registry = Some(base.registry());
            }
            Ok(Box::new(base))
        }
        #[cfg(feature = "login")]
        "mhf.login" => Ok(Box::new(mhf_login::LoginModule::new())),
        #[cfg(feature = "debug")]
        "mhf.debug" => Ok(Box::new(mhf_debug::DebugModule::new(
            registry.clone().ok_or("内置调试界面需要内置 mhf.base")?,
        ))),
        #[cfg(feature = "workbench")]
        "mhf.workbench" => Ok(Box::new(mhf_workbench::WorkbenchModule::new(
            registry.clone().ok_or("内置资源工作台需要内置 mhf.base")?,
            prepared.game.game_dir.clone(),
        ))),
        id => Err(format!("unknown builtin Mod {id}")),
    }
}

#[cfg(all(test, feature = "login", feature = "debug"))]
mod tests {
    use super::*;
    use mhf_mod_host::{ModHost, api};
    use std::{collections::BTreeMap, fs, sync::Mutex};

    // Config installs process-wide INI hooks during prepare. Each host owns a
    // complete install/uninstall cycle, so these tests cannot run concurrently.
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
    fn enabling_debug_replaces_login_without_requiring_sign_configuration() {
        let _host = HOST_LOCK.lock().unwrap();
        let directory = std::env::temp_dir().join(format!("mhf-startup-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("mhf.toml");
        for login in ["", "[mods.'mhf.login']\nenabled = false\n"] {
            fs::write(
                &path,
                format!("[mods.'mhf.debug']\nenabled = true\n{login}"),
            )
            .unwrap();
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
            let mut host = ModHost::load(resolved, &BTreeMap::new(), factory(&prepared)).unwrap();
            host.prepare().unwrap();
            let mut params = api::game::LaunchParams32::default();
            let mut global = api::game::GlobalData32::default();
            let mut target = api::LaunchTargetV1 {
                params: &mut params,
                global: &mut global,
            };
            assert!(unsafe { host.launch(&mut target) }.unwrap());
            assert_eq!(params.selected_character_id_1, 1);
            assert_eq!(&params.selected_character_name[..6], b"Debug\0");
            host.stop().unwrap();
            unsafe {
                host.detach(&[]).unwrap();
                host.prepare_release().unwrap();
            }
            drop(host);
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
