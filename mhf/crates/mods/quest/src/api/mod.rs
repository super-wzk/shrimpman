//! Quest data and operations. These Rust types and traits also generate the
//! public C interface. Base provides the interfaces; local task data is selected
//! explicitly before game attachment.

use mhf_mod_sdk::{
    Dependencies, GameModule, Result, abi as api,
    error::status_result,
    interface::{Interface, InterfaceRef, bind},
};
use safer_ffi::{
    prelude::{VirtualPtr, derive_ReprC},
    slice,
};

pub const PROVIDER_ID: &str = "mhf.base";
pub const INTERFACE_ID: &str = "mhf.quest.v1";
pub const CONTROL_INTERFACE_ID: &str = "mhf.quest.control.v3";
pub const LAUNCH_INTERFACE_ID: &str = "mhf.quest.launch.v1";

#[derive_ReprC(rename = "QuestSnapshot")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub quest_id: u16,
    /// The temporary hunter has initialized. Restart does not clear this;
    /// it does not imply the current task, map or actor is ready.
    pub hunter_initialized: bool,
    pub quest_size: usize,
}

/// Replacement resource species and variant, spawn record and hunter start area. This
/// prepares quest data; it does not create or control a running monster.
#[derive_ReprC(rename = "QuestMonsterSpawn")]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterSpawn {
    pub species: u8,
    /// Native species variant in 0..=16: 0 = normal, 1 = HC, 16 = Zenith.
    /// Other values depend on the species. The caller verifies species support.
    pub variant: u8,
    pub area: u16,
    pub position: [f32; 3],
    pub yaw: u16,
}

/// Record offset into the current quest replacement, not an actor handle.
/// Reset or another preparation invalidates its association with that record.
#[derive_ReprC]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnOffset(u32);

impl SpawnOffset {
    /// Represents an offset reported by a provider. This grants no pointer or
    /// native-memory borrow; `override_contains` checks the current range.
    #[inline]
    pub const fn from_byte_offset(offset: u32) -> Self {
        Self(offset)
    }

    #[inline]
    pub const fn byte_offset(self) -> usize {
        self.0 as usize
    }
}

/// Synchronized snapshots, copied without allocating. Implementations must not
/// unwind through the generated C entry point. Reads may run concurrently.
#[derive_ReprC(dyn)]
pub trait QuestApi: Send + Sync {
    fn snapshot(&self) -> Snapshot;
}

/// Select caller-supplied task data once, before game attachment. The slice is
/// copied and parsed as a BIN/JKR file; empty or invalid input is rejected.
/// Failed parsing leaves the provider idle and permits a corrected request.
/// Successful selection, attachment or stopping closes the selection window.
/// Implementations synchronize calls and must not unwind across the C boundary.
#[derive_ReprC(dyn)]
pub trait QuestLaunchApi: Send + Sync {
    fn prepare_local(&self, quest: slice::Ref<'_, u8>) -> api::Status;
}

/// Quest control on the game thread. Sharing the interface permits synchronized
/// reads; native mutations still require the individual safety contracts.
/// Implementations must not unwind through generated C entry points.
#[derive_ReprC(dyn)]
pub trait QuestControlApi: Send + Sync {
    fn snapshot(&self) -> Snapshot;

    /// Checks that the provider is installed into the intended loaded game.
    ///
    /// # Safety
    /// The module must be live. Call during installation or on the game thread
    /// while this provider's offline hooks are active. This does not retain the
    /// DLL or extend any native-memory lifetime.
    unsafe fn validate_module(&self, module: GameModule) -> api::Status;

    /// # Safety
    /// Call on the game thread with offline hooks active, after all consumers
    /// have released native actor references into the old quest. No native
    /// reader may concurrently use the state being restarted.
    unsafe fn restart(&self) -> api::Status;

    /// # Safety
    /// Call on the game thread with offline hooks active, after releasing all
    /// native references to the old override and before restarting the quest
    /// loader. No native reader may use the old override; old offsets expire.
    unsafe fn reset_quest(&self) -> api::Status;

    /// Prepares replacement records; success initializes `out_offset`, while
    /// failure preserves the current replacement and grants no output value.
    /// The output reference must not be retained by the implementation.
    ///
    /// # Safety
    /// Call on the game thread with offline hooks active, after releasing old
    /// native references and with no concurrent native reads of the replacement.
    unsafe fn prepare_monster_spawn(
        &self,
        spawn: MonsterSpawn,
        out_offset: &mut SpawnOffset,
    ) -> api::Status;

    /// A synchronized range query. It grants no pointer or lasting borrow, and
    /// the replacement may change after this call returns.
    fn override_contains(&self, offset: SpawnOffset, length: usize) -> bool;

    /// Replace one primary quest spawn; preserves the replacement on failure.
    /// # Safety
    /// Call on the game thread before restarting the quest loader, with no
    /// concurrent readers of replacement data. Offsets refer to the current quest.
    unsafe fn replace_monster(&self, offset: SpawnOffset, expected: u8, species: u8)
    -> api::Status;
}

/// Host lookups borrow this generated object through consumer destruction.
/// The provider owns it: never consume or free it, copy it into another owner,
/// or call its C `vtable.release_vptr`. All uses end before provider unloading.
pub type QuestTable = VirtualPtr<dyn QuestApi + Send + Sync>;
/// Borrowed under the same ownership rules as `QuestTable`.
pub type QuestControlTable = VirtualPtr<dyn QuestControlApi + Send + Sync>;
/// Borrowed under the same ownership rules as `QuestTable`.
pub type QuestLaunchTable = VirtualPtr<dyn QuestLaunchApi + Send + Sync>;

pub enum QuestInterface {}
pub enum QuestControlInterface {}
pub enum QuestLaunchInterface {}

// SAFETY: Each ID identifies its generated immutable trait table. Providers
// retain it through consumer destruction and honor the method contracts.
unsafe impl Interface for QuestInterface {
    type Table = QuestTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}
unsafe impl Interface for QuestControlInterface {
    type Table = QuestControlTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = CONTROL_INTERFACE_ID;
}
unsafe impl Interface for QuestLaunchInterface {
    type Table = QuestLaunchTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = LAUNCH_INTERFACE_ID;
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct QuestLaunch<'host> {
    table: &'host QuestLaunchTable,
}

impl<'host> QuestLaunch<'host> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'host>) -> Result<Self> {
        Ok(Self {
            table: bind::<QuestLaunchInterface>(dependencies)?.table(),
        })
    }

    #[inline]
    pub fn prepare_local(&self, quest: &[u8]) -> Result<()> {
        status_result(
            self.table.prepare_local(quest.into()),
            "prepare local quest",
        )
    }
}

#[repr(transparent)]
pub struct Quest<'host> {
    binding: InterfaceRef<'host, QuestInterface>,
}

impl<'host> Quest<'host> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'host>) -> Result<Self> {
        Ok(Self {
            binding: bind(dependencies)?,
        })
    }

    #[inline]
    pub fn snapshot(&self) -> Snapshot {
        self.binding.table().snapshot()
    }
}

/// A borrowed provider handle. The only adapters are host binding and error
/// conversion for fallible operations; no extra virtual dispatch is added.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct QuestControl<'host> {
    table: &'host QuestControlTable,
}

impl<'host> QuestControl<'host> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'host>) -> Result<Self> {
        Ok(Self {
            table: bind::<QuestControlInterface>(dependencies)?.table(),
        })
    }

    #[inline]
    pub fn snapshot(&self) -> Snapshot {
        self.table.snapshot()
    }

    /// # Safety
    /// See `QuestControlApi::validate_module`.
    #[inline]
    pub unsafe fn validate_module(&self, module: GameModule) -> Result<()> {
        status_result(
            unsafe { self.table.validate_module(module) },
            "quest module validation",
        )
    }

    /// # Safety
    /// See `QuestControlApi::restart`.
    #[inline]
    pub unsafe fn restart(&self) -> Result<()> {
        status_result(unsafe { self.table.restart() }, "quest restart")
    }

    /// # Safety
    /// See `QuestControlApi::reset_quest`.
    #[inline]
    pub unsafe fn reset_quest(&self) -> Result<()> {
        status_result(unsafe { self.table.reset_quest() }, "quest reset")
    }

    /// # Safety
    /// See `QuestControlApi::prepare_monster_spawn`. The returned offset expires
    /// at reset or the next preparation; this does not create a live actor.
    #[inline]
    pub unsafe fn prepare_monster_spawn(&self, spawn: MonsterSpawn) -> Result<SpawnOffset> {
        let mut offset = SpawnOffset::default();
        status_result(
            unsafe { self.table.prepare_monster_spawn(spawn, &mut offset) },
            "quest monster spawn preparation",
        )?;
        Ok(offset)
    }

    #[inline]
    pub fn override_contains(&self, offset: SpawnOffset, length: usize) -> bool {
        self.table.override_contains(offset, length)
    }

    /// # Safety
    /// See `QuestControlApi::replace_monster`.
    pub unsafe fn replace_monster(
        &self,
        offset: SpawnOffset,
        expected: u8,
        species: u8,
    ) -> Result<()> {
        status_result(
            unsafe { self.table.replace_monster(offset, expected, species) },
            "quest monster replacement",
        )
    }
}

/// Binds a provider inside native state without taking ownership.
///
/// # Safety
/// The pointer must be non-null, aligned, and point to an immutable initialized
/// `QuestControlTable`. Its object, vtable and provider code must remain valid
/// for `'provider` and all concurrent uses. The host may erase the lifetime only
/// while retaining the provider until the consuming native state stops and dies.
#[inline]
pub unsafe fn bind_control<'provider>(table: *const QuestControlTable) -> QuestControl<'provider> {
    QuestControl {
        table: unsafe { &*table },
    }
}

/// Borrow the startup interface without taking ownership of the provider.
///
/// # Safety
/// The table, object and provider code must remain valid for `'provider`.
/// The pointer must be non-null, aligned and refer to an initialized immutable
/// `QuestLaunchTable`. A copied binding does not keep its provider loaded.
#[inline]
pub unsafe fn bind_launch<'provider>(table: *const QuestLaunchTable) -> QuestLaunch<'provider> {
    QuestLaunch {
        table: unsafe { &*table },
    }
}

#[cfg(feature = "headers")]
pub fn define_header(definer: &mut dyn mhf_mod_sdk::abi::headers::Definer) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::alias::<Snapshot>(definer, "QuestSnapshot")?;
    h::alias::<MonsterSpawn>(definer, "QuestMonsterSpawn")?;
    h::alias::<SpawnOffset>(definer, "QuestSpawnOffset")?;
    h::alias::<QuestTable>(definer, "QuestTable")?;
    h::alias::<QuestControlTable>(definer, "QuestControlTable")?;
    h::alias::<QuestLaunchTable>(definer, "QuestLaunchTable")?;
    h::string(definer, "MHF_QUEST_PROVIDER", PROVIDER_ID)?;
    h::string(definer, "MHF_QUEST_INTERFACE", INTERFACE_ID)?;
    h::string(definer, "MHF_QUEST_CONTROL_INTERFACE", CONTROL_INTERFACE_ID)?;
    h::string(definer, "MHF_QUEST_LAUNCH_INTERFACE", LAUNCH_INTERFACE_ID)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn generated_layout_matches_typed_values_and_c_declarations() {
        assert_eq!(size_of::<Snapshot>(), 2 * size_of::<usize>());
        assert_eq!(offset_of!(Snapshot, hunter_initialized), 2);
        assert_eq!(offset_of!(Snapshot, quest_size), size_of::<usize>());
        assert_eq!(size_of::<MonsterSpawn>(), 20);
        assert_eq!(offset_of!(MonsterSpawn, variant), 1);
        assert_eq!(offset_of!(MonsterSpawn, area), 2);
        assert_eq!(offset_of!(MonsterSpawn, position), 4);
        assert_eq!(offset_of!(MonsterSpawn, yaw), 16);
        assert_eq!(size_of::<SpawnOffset>(), 4);
        assert_eq!(size_of::<QuestTable>(), 3 * size_of::<*const ()>());
        assert_eq!(size_of::<QuestControlTable>(), 9 * size_of::<*const ()>());
        assert_eq!(size_of::<QuestLaunchTable>(), 3 * size_of::<*const ()>());
        assert_eq!(
            offset_of!(
                safer_ffi::layout::CLayoutOf<QuestControlTable>,
                vtable.prepare_monster_spawn
            ),
            6 * size_of::<usize>()
        );
        assert_eq!(size_of::<QuestControl<'_>>(), size_of::<usize>());
        assert_eq!(size_of::<QuestLaunch<'_>>(), size_of::<usize>());
    }
}
