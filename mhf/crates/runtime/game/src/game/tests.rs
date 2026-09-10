use super::*;
use mhf_mod_host::{Context, LaunchProvider};
use mhf_mod_package::{Kind, Manifest, RuntimeConfig, Selection, Version, resolve};
use std::{collections::BTreeSet, path::PathBuf};

struct TestStartup(LaunchProvider);

impl Module for TestStartup {
    fn prepare(&mut self, context: &Context) -> Result<(), String> {
        self.0.register(context)
    }

    fn check(&mut self, context: &Context) -> Result<(), String> {
        if context.game().module_base.is_null() {
            return Err("the kernel must publish the loaded game before check".into());
        }
        Ok(())
    }
}

fn startup(ready: bool) -> Box<dyn Module> {
    Box::new(TestStartup(LaunchProvider::new(move |params, _global| {
        assert_eq!(params.server_selection, 55);
        params.selected_character_id_1 = 7;
        params.selected_character_id_2 = 7;
        params.character_ids[0] = 7;
        params.fixed_1d58_one = 1;
        params.fixed_200c_one = 1;
        params.selected_character_name[..5].copy_from_slice(b"Test\0");
        Ok(ready)
    })))
}

fn plan() -> Resolved {
    let candidate = Candidate::builtin(Manifest {
        schema: 1,
        id: "test.startup".into(),
        name: "Kernel startup fixture".into(),
        version: Version::new(1, 0, 0),
        kind: Kind::Native,
        entry: None,
        dependencies: BTreeMap::new(),
    })
    .unwrap();
    resolve(
        &[candidate],
        &BTreeMap::from([(
            "test.startup".into(),
            Selection {
                enabled: Some(true),
                ..Default::default()
            },
        )]),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
    .unwrap()
}

fn launch_config(game_dir: PathBuf) -> LaunchConfig {
    LaunchConfig {
        game_dir,
        params: crate::MhfLaunchParams32 {
            server_selection: 55,
            ..Default::default()
        },
        mods: RuntimeConfig::default(),
    }
}

#[test]
fn cancelled_startup_skips_loading_the_client_and_running_the_entry() {
    let invocation_dir = env::current_dir().unwrap();
    let config = launch_config(invocation_dir.clone());
    let profile = MhfLaunchProfile {
        game_dll: "missing-mhf-kernel-test.dll",
        ..crate::runtime::PROFILE
    };
    let exit = run_session(
        &config,
        &profile,
        plan(),
        |_| Ok(startup(false)),
        |_| panic!("cancelled startup must never reach the game entry"),
    )
    .unwrap();
    assert_eq!(exit.code, None);
    assert!(exit.mods.iter().all(|status| status.error.is_none()));
    assert_eq!(env::current_dir().unwrap(), invocation_dir);
}

#[test]
#[ignore = "requires MHF_TEST_CLIENT pointing to the supported HD game DLL"]
fn supported_client_receives_provider_parameters_and_unloads() {
    use windows::{Win32::System::LibraryLoader::GetModuleHandleA, core::PCSTR};
    let client = PathBuf::from(env::var_os("MHF_TEST_CLIENT").expect("set MHF_TEST_CLIENT"));
    let game_dll = client.file_name().unwrap().to_str().unwrap();
    let dll_name = std::ffi::CString::new(game_dll).unwrap();
    let config = launch_config(client.parent().unwrap().to_owned());
    let profile = MhfLaunchProfile {
        game_dll,
        ..crate::runtime::PROFILE
    };
    let invocation_dir = env::current_dir().unwrap();
    for _ in 0..2 {
        let exit = run_session(
            &config,
            &profile,
            plan(),
            |_| Ok(startup(true)),
            |native| {
                let data = &native.data;
                assert!(!data.mhfo_module.0.is_null());
                assert!(data.mhfo_main.is_some());
                assert_eq!(data.params.server_selection, 55);
                assert_eq!(data.params.selected_character_id_1, 7);
                assert_eq!(data.params.selected_character_id_2, 7);
                assert_eq!(data.params.character_ids[0], 7);
                assert_eq!(data.params.fixed_1d58_one, 1);
                assert_eq!(data.params.fixed_200c_one, 1);
                assert_eq!(&data.params.selected_character_name[..5], b"Test\0");
                // The real DLL is loaded; this fixture substitutes only its game entry.
                Ok(42)
            },
        )
        .unwrap();
        assert_eq!(exit.code, Some(42));
        assert!(exit.mods.iter().all(|status| status.error.is_none()));
        assert!(
            unsafe { GetModuleHandleA(PCSTR(dll_name.as_ptr().cast())) }.is_err(),
            "all game DLL references should have been released"
        );
        assert_eq!(env::current_dir().unwrap(), invocation_dir);
    }
}
