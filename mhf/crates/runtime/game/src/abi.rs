pub(crate) use mhf_mod_api::game::GlobalData32 as MhfGlobalData32;
pub use mhf_mod_api::game::LaunchParams32 as MhfLaunchParams32;
use windows::Win32::Foundation::HMODULE;

const MHF_LAUNCH_PARAMS_SIZE: usize = 0x2010;
const MHF_GLOBAL_DATA_SIZE: usize = 0x8ae0;

pub(crate) fn copy_c_string(
    field: &str,
    destination: &mut [u8],
    value: &[u8],
) -> Result<(), String> {
    if value.len() >= destination.len() {
        return Err(format!(
            "{field} is {} bytes; at most {} bytes are supported",
            value.len(),
            destination.len().saturating_sub(1)
        ));
    }
    destination.fill(0);
    destination[..value.len()].copy_from_slice(value);
    Ok(())
}

pub(crate) type GameMain = unsafe extern "C" fn(*mut MhfLaunchParams32) -> i32;

pub(crate) fn ptr32<T>(pointer: *mut T) -> u32 {
    pointer as usize as u32
}

pub(crate) fn function32(pointer: *const ()) -> u32 {
    pointer as usize as u32
}

#[repr(C)]
pub(crate) struct HostServices32 {
    reserved_00: u32,
    request_size: u32,
    request: u32,
    dynamic_callback: u32,
    response_size: u32,
    response: u32,
    validate: u32,
    installed_callback: u32,
    message: u32,
}

impl HostServices32 {
    pub(crate) fn new(request: *mut u8, response: *mut u8, validate: u32, message: u32) -> Self {
        Self {
            reserved_00: 0,
            request_size: 16,
            request: ptr32(request),
            dynamic_callback: 0,
            response_size: 16,
            response: ptr32(response),
            validate,
            installed_callback: 0,
            message,
        }
    }
}

#[repr(C, align(8))]
pub(crate) struct MhfHostData32 {
    pub(crate) params: MhfLaunchParams32,     // 0x0000
    reserved_2010: [u8; 8],                   // 0x2010
    pub(crate) data_ptr: u32,                 // 0x2018
    pub(crate) keyboard_layout: u32,          // 0x201c
    pub(crate) host_response: [u8; 16],       // 0x2020
    pub(crate) host_services: HostServices32, // 0x2030
    reserved_2054: [u8; 8],                   // 0x2054
    pub(crate) host_request: [u8; 0x14],      // 0x205c
    reserved_2070: u32,                       // 0x2070
    pub(crate) mhfo_module: HMODULE,          // 0x2074
    reserved_2078: [u8; 0x520],               // 0x2078
    pub(crate) ready_mutex_name: [u8; 0x100], // 0x2598
    reserved_2698: [u8; 0x414],               // 0x2698
    pub(crate) mhfo_main: Option<GameMain>,   // 0x2aac
    reserved_2ab0: u32,                       // 0x2ab0
}

impl Default for MhfHostData32 {
    fn default() -> Self {
        // SAFETY: every field accepts an all-zero representation.
        unsafe { std::mem::zeroed() }
    }
}

const _: () = {
    assert!(std::mem::size_of::<MhfLaunchParams32>() == MHF_LAUNCH_PARAMS_SIZE);
    assert!(std::mem::offset_of!(MhfLaunchParams32, launch_flags) == 0x0010);
    assert!(std::mem::offset_of!(MhfLaunchParams32, sign_session_id) == 0x18cc);
    assert!(std::mem::offset_of!(MhfLaunchParams32, sign_session_issued_at) == 0x18e8);
    assert!(std::mem::offset_of!(MhfLaunchParams32, entrance_server_address) == 0x1af0);
    assert!(std::mem::offset_of!(MhfLaunchParams32, entrance_server_host) == 0x1bf0);
    assert!(std::mem::offset_of!(MhfLaunchParams32, global_alloc) == 0x1d54);
    assert!(std::mem::offset_of!(MhfLaunchParams32, selected_character_gr) == 0x1d60);
    assert!(std::mem::offset_of!(MhfLaunchParams32, preset_level) == 0x1d6c);
    assert!(std::mem::offset_of!(MhfLaunchParams32, graphics_version) == 0x1d94);
    assert!(std::mem::offset_of!(MhfLaunchParams32, font_name) == 0x1dbc);
    assert!(std::mem::offset_of!(MhfLaunchParams32, proxy_address) == 0x1e38);
    assert!(std::mem::offset_of!(MhfLaunchParams32, server_selection) == 0x1e7c);
    assert!(std::mem::offset_of!(MhfLaunchParams32, host_services) == 0x1e80);
    assert!(std::mem::offset_of!(MhfLaunchParams32, return_expires_at) == 0x2004);
    assert!(std::mem::size_of::<MhfGlobalData32>() == MHF_GLOBAL_DATA_SIZE);
    assert!(std::mem::offset_of!(MhfGlobalData32, notice_lengths) == 0x0a0c);
    assert!(std::mem::offset_of!(MhfGlobalData32, notice_flags) == 0x0a24);
    assert!(std::mem::offset_of!(MhfGlobalData32, notices) == 0x0a2c);
    assert!(std::mem::offset_of!(MhfGlobalData32, festa_id) == 0x8aac);
    assert!(std::mem::offset_of!(MhfGlobalData32, festa_stalls) == 0x8ac0);
    assert!(std::mem::size_of::<HostServices32>() == 0x24);
    assert!(std::mem::offset_of!(MhfHostData32, data_ptr) == 0x2018);
    assert!(std::mem::offset_of!(MhfHostData32, host_services) == 0x2030);
    assert!(std::mem::offset_of!(MhfHostData32, host_request) == 0x205c);
    assert!(std::mem::offset_of!(MhfHostData32, ready_mutex_name) == 0x2598);
    assert!(std::mem::offset_of!(MhfHostData32, mhfo_main) == 0x2aac);
    assert!(std::mem::offset_of!(MhfHostData32, reserved_2ab0) == 0x2ab0);
};
