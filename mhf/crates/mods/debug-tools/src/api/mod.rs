//! Synchronized debug snapshots and game-thread command submission.

use mhf_mod_sdk::{
    Dependencies, Result, abi as api,
    error::status_result,
    interface::{Interface, InterfaceRef, bind},
};
use safer_ffi::{
    option::TaggedOption,
    prelude::{VirtualPtr, derive_ReprC},
};

pub const PROVIDER_ID: &str = "mhf.debug";
pub const INTERFACE_ID: &str = "mhf.debug-tools.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Transform(u8),
    RestoreHunter,
    ChangeArea(u16),
    Restart,
    Exit,
}

#[derive_ReprC(rename = "DebugSnapshot")]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub ready: bool,
    pub quest_id: u16,
    pub area: u16,
    pub monster: TaggedOption<u8>,
    pub position: [f32; 3],
    pub hit_checks: u32,
    pub hits: u32,
}

impl Snapshot {
    #[inline]
    pub fn monster(&self) -> Option<u8> {
        self.monster.into_rust()
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            ready: false,
            quest_id: 0,
            area: 0,
            monster: TaggedOption::None,
            position: [0.0; 3],
            hit_checks: 0,
            hits: 0,
        }
    }
}

/// Implementations synchronize concurrent calls and never unwind through a
/// generated C entry point. Command success confirms queue acceptance; execution
/// and its result occur later on the game thread.
#[derive_ReprC(dyn)]
pub trait DebugApi: Send + Sync {
    fn snapshot(&self) -> Snapshot;
    fn transform(&self, species: u8) -> api::Status;
    fn restore_hunter(&self) -> api::Status;
    fn change_area(&self, area: u16) -> api::Status;
    fn restart(&self) -> api::Status;
    fn exit(&self) -> api::Status;
}

/// The provider owns this virtual object. Host lookup lends its immutable table
/// through consumer destruction; consumers must not move, free, overwrite,
/// byte-copy as an owner, or call its `vtable.release_vptr`. A pointer copy does
/// not keep the provider DLL loaded. All access must end before provider release.
pub type DebugTable = VirtualPtr<dyn DebugApi + Send + Sync>;

pub enum DebugInterface {}

// SAFETY: The provider publishes an immutable table, synchronizes snapshots,
// and queues commands without retaining caller-owned pointers.
unsafe impl Interface for DebugInterface {
    type Table = DebugTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}

#[repr(transparent)]
pub struct DebugTools<'host> {
    binding: InterfaceRef<'host, DebugInterface>,
}

impl<'host> DebugTools<'host> {
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

    /// Success confirms queue acceptance. Execution occurs on the game thread;
    /// inspect snapshots or game messages for the eventual outcome.
    #[inline]
    pub fn command(&self, command: Command) -> Result<()> {
        let table = self.binding.table();
        let status = match command {
            Command::Transform(species) => table.transform(species),
            Command::RestoreHunter => table.restore_hunter(),
            Command::ChangeArea(area) => table.change_area(area),
            Command::Restart => table.restart(),
            Command::Exit => table.exit(),
        };
        status_result(status, "debug command")
    }
}

#[cfg(feature = "headers")]
pub fn define_header(definer: &mut dyn mhf_mod_sdk::abi::headers::Definer) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::alias::<Snapshot>(definer, "DebugSnapshot")?;
    h::alias::<DebugTable>(definer, "DebugTable")?;
    h::string(definer, "MHF_DEBUG_PROVIDER", PROVIDER_ID)?;
    h::string(definer, "MHF_DEBUG_INTERFACE", INTERFACE_ID)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn generated_snapshot_layout_keeps_narrow_fields_and_optional_monster() {
        assert_eq!(size_of::<Snapshot>(), 28);
        assert_eq!(offset_of!(Snapshot, quest_id), 2);
        assert_eq!(offset_of!(Snapshot, area), 4);
        assert_eq!(offset_of!(Snapshot, monster), 6);
        assert_eq!(offset_of!(Snapshot, position), 8);
        assert_eq!(size_of::<DebugTable>(), size_of::<[usize; 8]>());
        assert_eq!(
            offset_of!(safer_ffi::layout::CLayoutOf<DebugTable>, vtable.transform),
            3 * size_of::<usize>()
        );
        assert_eq!(Snapshot::default().monster(), None);
        assert_eq!(
            Snapshot {
                monster: Some(u8::MAX).into(),
                ..Default::default()
            }
            .monster(),
            Some(u8::MAX)
        );
    }
}
