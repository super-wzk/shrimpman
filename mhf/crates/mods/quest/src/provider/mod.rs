//! A temporary local hunter and quest session, independent of the debug tools.

mod binary;
#[cfg(test)]
mod fixtures;
#[cfg(all(windows, target_arch = "x86"))]
mod module;
#[cfg(all(windows, target_arch = "x86"))]
mod native;
mod service;

use crate::api::MonsterSpawn;
#[cfg(all(windows, target_arch = "x86"))]
pub use module::QuestMod;
#[cfg(all(windows, target_arch = "x86"))]
pub use native::{State, install};
pub use service::QuestService;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};

/// Prepared quest data shared by the offline runtime and optional debug tools.
/// Original files are decompressed once; the runtime owns every replacement image.
#[derive(Clone)]
pub struct Session {
    inner: Arc<SessionData>,
}

struct SessionData {
    quest: binary::Quest,
    quest_override: Mutex<Option<Vec<u8>>>,
    started: AtomicBool,
}

impl Session {
    /// Read an original Japanese BIN/JKR quest file, retaining its CP932 text.
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self::from_quest(binary::Quest::parse(bytes)?))
    }

    fn from_quest(quest: binary::Quest) -> Self {
        Self {
            inner: Arc::new(SessionData {
                quest,
                quest_override: Mutex::new(None),
                started: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn quest_id(&self) -> u16 {
        self.inner.quest.id
    }

    #[cfg(all(windows, target_arch = "x86"))]
    pub(crate) fn validate_module(
        &self,
        module: windows::Win32::Foundation::HMODULE,
    ) -> Result<(), String> {
        native::validate_session(self, module)
    }

    pub(crate) fn started(&self) -> bool {
        self.inner.started.load(Ordering::Acquire)
    }

    pub(crate) fn quest_len(&self) -> usize {
        self.inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or(self.inner.quest.bytes.len(), Vec::len)
    }

    /// Build from the original quest and publish only a complete replacement.
    /// Call on the game thread before restarting the native quest loader.
    pub(crate) fn prepare_monster_spawn(&self, spawn: MonsterSpawn) -> Result<usize, String> {
        let quest = self.inner.quest.with_monster(spawn)?;
        *self
            .inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(quest.bytes);
        Ok(quest.spawn_offset)
    }

    pub(crate) fn reset_quest(&self) {
        *self
            .inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }

    pub(crate) fn override_contains(&self, offset: usize, length: usize) -> bool {
        self.inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|bytes| {
                offset
                    .checked_add(length)
                    .is_some_and(|end| end <= bytes.len())
            })
    }

    /// All optional tools must release their native actor references first.
    /// Only call on the game thread while this session's offline hooks are active.
    #[cfg(all(windows, target_arch = "x86"))]
    pub(crate) unsafe fn restart(&self) -> Result<(), String> {
        unsafe { native::restart(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::{Session, fixtures::monster_spawn};
    use crate::api::MonsterSpawn;

    #[test]
    fn session_clones_share_complete_overrides_without_accumulating_quest_data() {
        let session = Session::new(&super::fixtures::quest_bytes()).unwrap();
        let observer = session.clone();
        let original_len = session.quest_len();
        let original_id = session.quest_id();
        assert!(!observer.override_contains(0, 1));

        let first_spawn = session.prepare_monster_spawn(monster_spawn(1, 1)).unwrap();
        let first_len = observer.quest_len();
        assert!(first_spawn >= original_len);
        assert!(observer.override_contains(first_spawn, 60));
        assert!(!observer.override_contains(first_spawn, 61));
        assert!(!observer.override_contains(usize::MAX, 60));

        let next_spawn = observer
            .prepare_monster_spawn(MonsterSpawn {
                position: [1.0; 3],
                yaw: 1,
                ..monster_spawn(2, 0)
            })
            .unwrap();
        assert_eq!(next_spawn, first_spawn);
        assert_eq!(session.quest_len(), first_len);
        let before = session.inner.quest_override.lock().unwrap().clone();
        assert!(session.prepare_monster_spawn(monster_spawn(0, 0)).is_err());
        assert_eq!(*session.inner.quest_override.lock().unwrap(), before);

        observer.reset_quest();
        assert_eq!(session.quest_len(), original_len);
        assert_eq!(session.quest_id(), original_id);
        assert!(!session.override_contains(first_spawn, 60));
    }
}
