//! Mod lifecycle and host C ABI. safer-ffi derives the layouts and generates
//! `include/mhf_mod.h` from these definitions.
//!
//! Every function uses the C calling convention (`__cdecl` on Windows x86).
//! The manifest is the only source of package versions. Interface IDs identify
//! fixed layouts; this ABI does not negotiate structure sizes.

#![no_std]

#[cfg(feature = "headers")]
extern crate std;
#[cfg(feature = "headers")]
pub mod headers;

pub mod game;

use core::ffi::c_void;
use safer_ffi::derive_ReprC;

pub type Status = i32;
pub const OK: Status = 0;
pub const ERROR: Status = 1;
pub const BUFFER_TOO_SMALL: Status = 2;
pub const NOT_FOUND: Status = 3;
pub const INVALID_STATE: Status = 4;
pub const CONFLICT: Status = 5;
pub const CANCELLED: Status = 6;

pub const LOG_ERROR: u32 = 0;
pub const LOG_WARN: u32 = 1;
pub const LOG_INFO: u32 = 2;
pub const LOG_DEBUG: u32 = 3;
pub const LOG_TRACE: u32 = 4;

pub const PHASE_PREPARE: u32 = 0;
pub const PHASE_CHECK: u32 = 1;
pub const PHASE_ATTACH: u32 = 2;
pub const PHASE_RUNNING: u32 = 3;
pub const PHASE_STOP: u32 = 4;
pub const PHASE_DETACH: u32 = 5;
pub const PHASE_DESTROY: u32 = 6;

pub const MOD_QUERY_SYMBOL: &[u8] = b"mhf_mod_query_v2\0";
pub const LAUNCH_INTERFACE_ID: &str = "mhf.launch.v1";
pub const FALLBACK_LAUNCH_INTERFACE_ID: &str = "mhf.launch.fallback.v1";

/// UTF-8 borrowed for one call. No terminating NUL; a zero length permits NULL.
#[derive_ReprC]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Str {
    pub ptr: *const u8,
    pub len: u32,
}

impl Str {
    pub fn new(value: &str) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: u32::try_from(value.len()).expect("ABI strings must fit in u32"),
        }
    }

    /// # Safety
    /// `ptr` must address `len` readable bytes of UTF-8 for the returned lifetime.
    pub unsafe fn as_str<'a>(self) -> &'a str {
        if self.len == 0 {
            return "";
        }
        let bytes = unsafe { core::slice::from_raw_parts(self.ptr, self.len as usize) };
        unsafe { core::str::from_utf8_unchecked(bytes) }
    }
}

/// `module_base` is borrowed from the host and NULL before the game DLL loads.
/// A mod must not keep an independent DLL reference beyond detach; the host
/// coordinates the final release of game and mod libraries.
#[derive_ReprC]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameInfoV2 {
    pub module_base: *mut c_void,
    pub phase: u32,
}

/// Host-owned, initialized launch storage. Both pointers are valid and exclusive
/// for one launch callback. A provider must not retain either pointer.
#[derive_ReprC]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LaunchTargetV1 {
    pub params: *mut game::LaunchParams32,
    pub global: *mut game::GlobalData32,
}

/// Mods publish this table during prepare under the launch or fallback launch
/// interface ID. The host calls one provider from the launch tier, or from the
/// fallback tier when no launch provider exists. The chosen tier must have one
/// provider. Calls run once before game loading, on the lifecycle thread. Return OK
/// when launch data is ready, CANCELLED for user cancellation, or an error after
/// logging details through the host. The callback must not unwind or retain the
/// target pointers. The provider owns this table and its state through destroy.
#[derive_ReprC]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LaunchApiV1 {
    pub context: *mut c_void,
    pub run: unsafe extern "C" fn(context: *mut c_void, target: *mut LaunchTargetV1) -> Status,
}

pub type HookGroup = *mut c_void;
pub type DrainFn = unsafe extern "C" fn(state: *mut c_void) -> Status;
pub type LifecycleFn = unsafe extern "C" fn(instance: *mut c_void) -> Status;
pub type ReadTextFn = unsafe extern "C" fn(
    context: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> Status;

/// The host owns all groups. The mod owns callback state until group cleanup
/// succeeds. `drain` runs after hooks are disabled and before trampolines vanish.
#[derive_ReprC]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HookApiV1 {
    pub prepare_group: unsafe extern "C" fn(
        context: *mut c_void,
        name: Str,
        state: *mut c_void,
        drain: Option<DrainFn>,
        out_group: *mut HookGroup,
    ) -> Status,
    pub create_hook: unsafe extern "C" fn(
        context: *mut c_void,
        group: HookGroup,
        target: *mut c_void,
        detour: *mut c_void,
        out_trampoline: *mut *mut c_void,
    ) -> Status,
    pub enable_group: unsafe extern "C" fn(context: *mut c_void, group: HookGroup) -> Status,
    pub discard_group: unsafe extern "C" fn(context: *mut c_void, group: HookGroup) -> Status,
}

/// A stable per-mod host table, borrowed until that mod's destroy returns.
/// Host operations run on the lifecycle thread. Provider interfaces can define
/// their own threading rules. No host lock is held while invoking mod code.
#[derive_ReprC]
#[repr(C)]
pub struct HostV2 {
    pub context: *mut c_void,
    pub log: unsafe extern "C" fn(context: *mut c_void, level: u32, message: Str),
    /// UTF-8 TOML for this mod's settings. Output excludes a terminating NUL.
    pub config: ReadTextFn,
    /// Absolute UTF-8 package resource directory.
    pub resource_root: ReadTextFn,
    /// Last host error for this mod. Reading it must not clear or replace it.
    pub last_error: ReadTextFn,
    /// The table and anything it references stay alive until destroy. Interfaces
    /// registered during prepare or attach become available once that phase
    /// succeeds for this provider.
    pub register_interface: unsafe extern "C" fn(
        context: *mut c_void,
        interface_id: Str,
        table: *const c_void,
    ) -> Status,
    /// This Mod's published interfaces and declared dependencies are visible.
    /// A successful table remains valid
    /// through this consumer's destroy, including stop and detach. It is borrowed:
    /// consumers must not free it or consume/copy an owning virtual object from it.
    /// In particular, never call a safer-ffi object's vtable.release_vptr.
    pub dependency: unsafe extern "C" fn(
        context: *mut c_void,
        provider_id: Str,
        interface_id: Str,
        out_table: *mut *const c_void,
    ) -> Status,
    pub game_info: unsafe extern "C" fn(context: *mut c_void, out: *mut GameInfoV2) -> Status,
    pub hooks: HookApiV1,
}

/// Lifecycle calls are serialized. All callbacks must contain panics/exceptions.
/// `create` owns allocation and `destroy` frees it in the same DLL. Failed
/// stop/detach/drain leaves the instance, DLL, and its dependencies resident.
#[derive_ReprC]
#[repr(C)]
pub struct ModV2 {
    pub create: unsafe extern "C" fn(host: *const HostV2, out_instance: *mut *mut c_void) -> Status,
    pub prepare: Option<LifecycleFn>,
    pub check: Option<LifecycleFn>,
    pub attach: Option<LifecycleFn>,
    pub stop: Option<LifecycleFn>,
    pub detach: Option<LifecycleFn>,
    pub destroy: unsafe extern "C" fn(instance: *mut c_void),
}

pub type ModQueryV2 = unsafe extern "C" fn() -> *const ModV2;
