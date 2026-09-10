#[test]
#[ignore = "set MHF_TEST_CLIENT and MHF_TEST_QUEST; loads the real game DllMain"]
fn supported_client_keeps_quest_and_debug_hook_lifetimes_separate() {
    use crate::provider as debug;
    use mhf_hooks::ModuleReference;
    use mhf_quest::provider::{QuestService, Session, install};
    use windows::{
        Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW},
        core::PCWSTR,
    };

    let path = std::env::var("MHF_TEST_CLIENT").expect("MHF_TEST_CLIENT");
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let module =
        unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
            .expect("load supported game DLL and adjacent dependencies");
    let loaded = unsafe { ModuleReference::from_owned(module) };
    let entry = |rva| unsafe {
        std::slice::from_raw_parts((module.0 as usize + rva) as *const u8, 16).to_vec()
    };
    let offline_rvas = [0x008d25a0, 0x01501c30, 0x00817950];
    let debug_rvas = [
        0x008fcee0, 0x0089e510, 0x008696d0, 0x00a5b800, 0x00baee10, 0x00b7b570, 0x008b6bf0,
        0x008b7b60, 0x00846ca0,
    ];
    let offline_originals = offline_rvas.map(entry);
    let debug_originals = debug_rvas.map(entry);
    let quest = std::env::var("MHF_TEST_QUEST").expect("MHF_TEST_QUEST");
    let session = Session::new(&std::fs::read(quest).unwrap()).unwrap();

    let service = QuestService::new(session.clone());
    let control = unsafe { mhf_quest::bind_control(service.control_api()) };
    for _ in 0..2 {
        // Reusing a prepared Session starts a fresh original quest run.
        let mut offline =
            unsafe { install(module, session.clone()) }.expect("install offline only");
        assert!(!control.snapshot().hunter_initialized);
        assert!(!control.override_contains(mhf_quest::SpawnOffset::default(), 1));
        for (rva, original) in debug_rvas.into_iter().zip(&debug_originals) {
            assert_eq!(
                entry(rva),
                *original,
                "offline changed debug entry {rva:#x}"
            );
        }
        for (rva, original) in offline_rvas.into_iter().zip(&offline_originals) {
            assert_ne!(
                entry(rva),
                *original,
                "offline entry {rva:#x} was not hooked"
            );
        }

        // Keep the selected provider alive until the debug guard and its
        // retired native state have been dropped at the end of this loop.
        let mut tools = unsafe { debug::install(module, control, debug::DebugControl::new()) }
            .expect("add optional debug hooks to the running offline group");
        assert!(
            !control.snapshot().hunter_initialized,
            "install must not bootstrap the hunter or read the catalog"
        );
        for (rva, original) in debug_rvas.into_iter().zip(&debug_originals) {
            assert_ne!(entry(rva), *original, "debug entry {rva:#x} was not hooked");
        }
        tools
            .uninstall()
            .expect("remove optional debug hooks first");
        for (rva, original) in debug_rvas.into_iter().zip(&debug_originals) {
            assert_eq!(
                entry(rva),
                *original,
                "debug entry {rva:#x} was not restored"
            );
        }
        for (rva, original) in offline_rvas.into_iter().zip(&offline_originals) {
            assert_ne!(
                entry(rva),
                *original,
                "removing debug also removed offline entry {rva:#x}"
            );
        }

        unsafe {
            control.prepare_monster_spawn(mhf_quest::MonsterSpawn {
                species: 1,
                area: 461,
                position: [0.0; 3],
                yaw: 0,
            })
        }
        .unwrap();

        offline
            .uninstall()
            .expect("remove offline hooks after debug");
        for (rva, original) in offline_rvas.into_iter().zip(&offline_originals) {
            assert_eq!(
                entry(rva),
                *original,
                "offline entry {rva:#x} was not restored"
            );
        }
    }
    drop(loaded);
}
