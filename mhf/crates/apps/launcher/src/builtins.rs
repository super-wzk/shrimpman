use crate::runtime::PreparedLaunch;
use mhf_mod_host::Module;
use mhf_mod_package::{BuiltinCatalog, Candidate};

pub(super) fn catalog() -> BuiltinCatalog {
    BuiltinCatalog {
        login: cfg!(feature = "login"),
        debug: cfg!(feature = "debug"),
    }
}

/// The application assembles its compiled packages; the game host only sees Mods.
pub(super) fn factory(
    prepared: &PreparedLaunch,
) -> impl FnMut(&Candidate) -> Result<Box<dyn Module>, String> + '_ {
    #[cfg(feature = "debug")]
    let mut registry = None;
    move |candidate| match candidate.manifest.id.as_str() {
        "mhf.config" => Ok(Box::new(mhf_config::ConfigMod::new(
            prepared.store.clone(),
            mhf_game::runtime::PROFILE.ini_name.to_owned(),
        ))),
        "mhf.base" => {
            let base = mhf_base::BaseMod::new(&prepared.mhf)?;
            #[cfg(feature = "debug")]
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
        id => Err(format!("unknown builtin Mod {id}")),
    }
}

#[cfg(all(test, feature = "login", feature = "debug"))]
mod tests {
    use super::*;
    use mhf_mod_host::{ModHost, api};
    use std::{collections::BTreeMap, fs};

    #[test]
    fn enabling_debug_replaces_login_without_requiring_sign_configuration() {
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
