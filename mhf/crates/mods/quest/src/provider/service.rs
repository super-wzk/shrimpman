//! Base owns an idle or selected local session. safer-ffi generates the
//! virtual tables; consumers borrow them through the host's dependency lifetime.

use super::Session;
use crate::api::{
    MonsterSpawn, QuestApi, QuestControlApi, QuestControlTable, QuestLaunchApi, QuestLaunchTable,
    QuestTable, Snapshot, SpawnOffset,
};
use mhf_mod_sdk::{GameModule, abi as api};
use safer_ffi::slice;
use std::sync::{Arc, Mutex, PoisonError};

/// Owns portable quest data and the generated tables. Native validation and
/// restart report an error on platforms without the game provider.
pub struct QuestService {
    api: QuestTable,
    control_api: QuestControlTable,
    launch_api: QuestLaunchTable,
    #[cfg(any(test, all(windows, target_arch = "x86")))]
    state: Arc<QuestState>,
}

struct QuestState {
    selection: Mutex<Selection>,
}

#[derive(Default)]
struct Selection {
    session: Option<Session>,
    sealed: bool,
}

impl Default for QuestService {
    fn default() -> Self {
        Self::with_session(None)
    }
}

impl QuestService {
    pub fn api(&self) -> &QuestTable {
        &self.api
    }

    pub fn control_api(&self) -> &QuestControlTable {
        &self.control_api
    }

    pub fn launch_api(&self) -> &QuestLaunchTable {
        &self.launch_api
    }

    pub fn new(session: Session) -> Self {
        Self::with_session(Some(session))
    }

    fn with_session(session: Option<Session>) -> Self {
        let state = Arc::new(QuestState {
            selection: Mutex::new(Selection {
                session,
                sealed: false,
            }),
        });
        Self {
            api: state.clone().into(),
            control_api: state.clone().into(),
            launch_api: state.clone().into(),
            #[cfg(any(test, all(windows, target_arch = "x86")))]
            state,
        }
    }

    #[cfg(any(test, all(windows, target_arch = "x86")))]
    pub(crate) fn session_for_attach(&self) -> Option<Session> {
        let mut selection = self
            .state
            .selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        selection.sealed = true;
        selection.session.clone()
    }

    #[cfg(any(test, all(windows, target_arch = "x86")))]
    pub(crate) fn seal(&self) {
        self.state
            .selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .sealed = true;
    }
}

impl QuestState {
    fn session(&self) -> Result<Session, String> {
        self.selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session
            .clone()
            .ok_or_else(|| "no local quest was selected".into())
    }
}

impl QuestApi for QuestState {
    fn snapshot(&self) -> Snapshot {
        let Ok(session) = self.session() else {
            return Snapshot::default();
        };
        Snapshot {
            quest_id: session.quest_id(),
            hunter_initialized: session.started(),
            quest_size: session.quest_len(),
        }
    }
}

impl QuestLaunchApi for QuestState {
    fn prepare_local(&self, quest: slice::Ref<'_, u8>) -> api::Status {
        call(|| {
            let mut selection = self
                .selection
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if selection.sealed || selection.session.is_some() {
                return Err("local quest selection is already closed".into());
            }
            let session = Session::new(&quest)?;
            selection.session = Some(session);
            Ok(())
        })
    }
}

fn call(operation: impl FnOnce() -> Result<(), String>) -> api::Status {
    use std::{
        io::Write,
        panic::{AssertUnwindSafe, catch_unwind},
    };
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => api::OK,
        Ok(Err(error)) => {
            let _ = writeln!(std::io::stderr(), "mhf.quest: {error}");
            api::ERROR
        }
        Err(_) => api::ERROR,
    }
}

impl QuestControlApi for QuestState {
    fn snapshot(&self) -> Snapshot {
        QuestApi::snapshot(self)
    }
    unsafe fn validate_module(&self, module: GameModule) -> api::Status {
        call(|| {
            let session = self.session()?;
            #[cfg(all(windows, target_arch = "x86"))]
            {
                session.validate_module(windows::Win32::Foundation::HMODULE(
                    mhf_mod_sdk::host::game_module_ptr(module),
                ))
            }
            #[cfg(not(all(windows, target_arch = "x86")))]
            {
                let _ = (session, module);
                Err("native quest validation requires i686 Windows".into())
            }
        })
    }
    unsafe fn restart(&self) -> api::Status {
        call(|| {
            // Clone while locked, then release selection before calling native
            // code, which can synchronously enter Quest hooks again.
            let session = self.session()?;
            #[cfg(all(windows, target_arch = "x86"))]
            {
                unsafe { session.restart() }
            }
            #[cfg(not(all(windows, target_arch = "x86")))]
            {
                let _ = session;
                Err("native quest restart requires i686 Windows".into())
            }
        })
    }
    unsafe fn reset_quest(&self) -> api::Status {
        call(|| {
            self.session()?.reset_quest();
            Ok(())
        })
    }
    unsafe fn prepare_monster_spawn(
        &self,
        spawn: MonsterSpawn,
        out: &mut SpawnOffset,
    ) -> api::Status {
        call(|| {
            let offset = self.session()?.prepare_monster_spawn(spawn)?;
            *out = SpawnOffset::from_byte_offset(offset as u32);
            Ok(())
        })
    }
    fn override_contains(&self, offset: SpawnOffset, length: usize) -> bool {
        self.session()
            .is_ok_and(|session| session.override_contains(offset.byte_offset(), length))
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::monster_spawn;
    use super::*;

    #[test]
    fn idle_service_has_no_quest_and_control_operations_preserve_outputs() {
        let service = QuestService::default();
        let control = service.control_api();
        assert_eq!(service.api().snapshot(), Snapshot::default());
        assert_eq!(control.snapshot(), Snapshot::default());
        assert!(!control.override_contains(SpawnOffset::default(), 1));
        // An idle provider has no native state to mutate.
        assert_eq!(unsafe { control.restart() }, api::ERROR);
        assert_eq!(unsafe { control.reset_quest() }, api::ERROR);
        let mut offset = SpawnOffset::from_byte_offset(77);
        let status = unsafe { control.prepare_monster_spawn(monster_spawn(1, 0), &mut offset) };
        assert_eq!(status, api::ERROR);
        assert_eq!(offset.byte_offset(), 77);
        assert!(service.session_for_attach().is_none());
    }

    #[test]
    fn local_selection_copies_valid_data_and_failed_parsing_does_not_consume_it() {
        let service = QuestService::default();
        let launch = unsafe { crate::api::bind_launch(service.launch_api()) };
        assert!(launch.prepare_local(b"invalid quest").is_err());
        assert_eq!(service.api().snapshot(), Snapshot::default());

        let mut bytes = super::super::fixtures::quest_bytes();
        let expected = Session::new(&bytes).unwrap();
        launch.prepare_local(&bytes).unwrap();
        bytes.fill(0);
        let snapshot = service.api().snapshot();
        assert_eq!(snapshot.quest_id, expected.quest_id());
        assert_eq!(snapshot.quest_size, expected.quest_len());
        assert!(!snapshot.hunter_initialized);
        assert!(launch.prepare_local(&[]).is_err());
        assert_eq!(service.api().snapshot(), snapshot);
        assert_eq!(
            service.session_for_attach().unwrap().quest_id(),
            expected.quest_id()
        );
    }

    #[test]
    fn empty_data_is_rejected_and_attachment_or_stop_closes_selection() {
        let service = QuestService::default();
        let launch = unsafe { crate::api::bind_launch(service.launch_api()) };
        assert!(launch.prepare_local(&[]).is_err());
        assert_eq!(service.api().snapshot(), Snapshot::default());
        launch
            .prepare_local(&super::super::fixtures::quest_bytes())
            .unwrap();
        assert_eq!(
            service.api().snapshot().quest_id,
            Session::new(&super::super::fixtures::quest_bytes())
                .unwrap()
                .quest_id()
        );
        assert!(service.session_for_attach().is_some());
        assert!(launch.prepare_local(&[]).is_err());

        let idle = QuestService::default();
        assert!(idle.session_for_attach().is_none());
        assert_eq!(
            idle.launch_api().prepare_local((&[][..]).into()),
            api::ERROR
        );
        let stopped = QuestService::default();
        stopped.seal();
        assert_eq!(
            stopped.launch_api().prepare_local((&[][..]).into()),
            api::ERROR
        );
        assert_eq!(stopped.api().snapshot(), Snapshot::default());
    }

    #[test]
    fn control_table_mutates_its_selected_provider_and_preserves_failed_overrides() {
        let first_session = Session::new(&super::super::fixtures::quest_bytes()).unwrap();
        let _first = QuestService::new(first_session.clone());
        let mut original = super::super::fixtures::quest_bytes();
        original[0x284..0x288].copy_from_slice(&15u32.to_le_bytes());
        original[0x288..0x28c].copy_from_slice(&u32::MAX.to_le_bytes());
        let second_session = Session::new(&original).unwrap();
        let second = QuestService::new(second_session.clone());
        // No native readers exist in this isolated test; each binding is dropped
        // before the service and never invokes native restart operations.
        let control = unsafe { crate::api::bind_control(&second.control_api) };
        let original_size = control.snapshot().quest_size;
        let spawn = monster_spawn(1, 11);
        let offset = unsafe { control.prepare_monster_spawn(spawn) }.unwrap();
        assert!(control.override_contains(offset, 60));
        assert!(!first_session.override_contains(offset.byte_offset(), 60));
        let changed_size = control.snapshot().quest_size;
        assert!(changed_size > original_size);
        let prepared = second_session.inner.quest_override.lock().unwrap().clone();
        assert_eq!(prepared.as_ref().unwrap()[0x80 + 0x91], 11);
        // Invalid values and an unreadable third variant slot must preserve
        // every published byte as well as the caller's output value.
        for rejected in [
            MonsterSpawn {
                species: 0,
                ..spawn
            },
            MonsterSpawn {
                variant: 17,
                ..spawn
            },
            MonsterSpawn {
                species: 17,
                ..spawn
            },
        ] {
            let mut ignored = SpawnOffset::from_byte_offset(77);
            let status = unsafe {
                second
                    .control_api
                    .prepare_monster_spawn(rejected, &mut ignored)
            };
            assert_eq!(status, api::ERROR);
            assert_eq!(ignored.byte_offset(), 77);
            assert_eq!(control.snapshot().quest_size, changed_size);
            assert_eq!(
                *second_session.inner.quest_override.lock().unwrap(),
                prepared
            );
        }
        unsafe { control.reset_quest() }.unwrap();
        assert_eq!(control.snapshot().quest_size, original_size);
        assert!(!control.override_contains(offset, 60));
    }
}
