use mhf_mod_sdk::abi as api;
use std::ffi::c_void;

pub const PROVIDER_ID: &str = "example.hook";
pub const INTERFACE_ID: &str = "example.hook.v1";
pub type Target = unsafe extern "C" fn(u32) -> u32;

#[safer_ffi::derive_ReprC]
#[repr(C)]
pub struct HookSnapshotV1 {
    pub entered: u32,
    pub completed: u32,
    pub active: u32,
    pub drains: u32,
}

#[safer_ffi::derive_ReprC]
#[repr(C)]
pub struct HookProbeV1 {
    pub context: *mut c_void,
    pub invoke: Target,
    pub snapshot: unsafe extern "C" fn(*mut c_void, *mut HookSnapshotV1) -> api::Status,
}
