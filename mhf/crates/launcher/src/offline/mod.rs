//! A temporary local hunter and quest session, independent of the debug tools.

mod native;
mod quest;

pub(crate) use native::install;
use std::sync::{Arc, Mutex, atomic::AtomicBool};

#[cfg(feature = "debug")]
use std::sync::{PoisonError, atomic::Ordering};

/// Prepared quest data shared by the offline runtime and optional debug tools.
/// Original files are decoded once; the runtime owns every replacement image.
#[derive(Clone)]
pub struct Session {
    inner: Arc<SessionData>,
}

struct SessionData {
    quest: quest::Quest,
    quest_override: Mutex<Option<Vec<u8>>>,
    started: AtomicBool,
}

impl Session {
    /// The embedded quest, starting at the Historical Site camp.
    /// The `translation` feature applies its Chinese sample text.
    pub fn test_map() -> Result<Self, String> {
        Ok(Self::from_quest(quest::Quest::test_map()?))
    }

    /// Read an original Japanese BIN/JKR quest file. With `unicode`, its
    /// CP932 text is converted once before it reaches the native quest buffer.
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self::from_quest(quest::Quest::parse(bytes)?))
    }

    fn from_quest(quest: quest::Quest) -> Self {
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

    #[cfg(feature = "debug")]
    pub(crate) fn validate_module(
        &self,
        module: windows::Win32::Foundation::HMODULE,
    ) -> Result<(), String> {
        native::validate_session(self, module)
    }

    #[cfg(feature = "debug")]
    pub(crate) fn started(&self) -> bool {
        self.inner.started.load(Ordering::Acquire)
    }

    #[cfg(feature = "debug")]
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
    #[cfg(feature = "debug")]
    pub(crate) fn prepare_monster(
        &self,
        species: u8,
        area: u16,
        position: [f32; 3],
        yaw: u16,
    ) -> Result<usize, String> {
        let quest = self
            .inner
            .quest
            .with_monster(species, area, position, yaw)?;
        *self
            .inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(quest.bytes);
        Ok(quest.spawn_offset)
    }

    #[cfg(feature = "debug")]
    pub(crate) fn reset_quest(&self) {
        *self
            .inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }

    #[cfg(feature = "debug")]
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
    #[cfg(feature = "debug")]
    pub(crate) unsafe fn restart(&self) -> Result<(), String> {
        unsafe { native::restart(self) }
    }
}

#[cfg(all(test, feature = "debug"))]
mod tests {
    use super::Session;

    #[test]
    fn session_clones_share_complete_overrides_without_accumulating_quest_data() {
        let session = Session::test_map().unwrap();
        let observer = session.clone();
        let original_len = session.quest_len();
        let original_id = session.quest_id();
        assert!(!observer.override_contains(0, 1));

        let first_spawn = session.prepare_monster(1, 461, [0.0; 3], 0).unwrap();
        let first_len = observer.quest_len();
        assert!(first_spawn >= original_len);
        assert!(observer.override_contains(first_spawn, 60));
        assert!(!observer.override_contains(first_spawn, 61));
        assert!(!observer.override_contains(usize::MAX, 60));

        let next_spawn = observer.prepare_monster(2, 461, [1.0; 3], 1).unwrap();
        assert_eq!(next_spawn, first_spawn);
        assert_eq!(session.quest_len(), first_len);
        let before = session.inner.quest_override.lock().unwrap().clone();
        assert!(session.prepare_monster(0, 461, [0.0; 3], 0).is_err());
        assert_eq!(*session.inner.quest_override.lock().unwrap(), before);

        observer.reset_quest();
        assert_eq!(session.quest_len(), original_len);
        assert_eq!(session.quest_id(), original_id);
        assert!(!session.override_contains(first_spawn, 60));
    }
}
