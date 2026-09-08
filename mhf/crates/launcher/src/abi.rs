use windows::Win32::Foundation::{HANDLE, HGLOBAL, HINSTANCE, HMODULE};

const MHF_LAUNCH_PARAMS_SIZE: usize = 0x2010;
const MHF_GLOBAL_DATA_SIZE: usize = 0x8ae0;
const LOGIN_NOTICE_SLOTS: usize = 4;
const LOGIN_NOTICE_BYTES: usize = 0x1000;
const FESTA_STALL_SLOTS: usize = 8;

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

// This 0x2010-byte block is passed to mhfo[-hd].dll. Comments retain mhf-iel's
// DataZZ names where they differ from the domain terminology used by Rust.
#[repr(C, align(8))]
pub struct MhfLaunchParams32 {
    pub module_instance: HINSTANCE,                     // 0x0000
    pub unknown_0004: [u8; 8],                          // 0x0004
    pub launch_mode: u32,                               // 0x000c
    pub launch_flags: u32,                              // 0x0010
    pub game_dir: [u8; 0x400],                          // 0x0014
    pub launcher_dir: [u8; 0x400],                      // 0x0414
    pub username: [u8; 0x800],                          // 0x0814, user_name
    pub password: [u8; 0x800],                          // 0x1014, user_password
    pub command_number: u32,                            // 0x1814
    pub command_netcf_update: u32,                      // 0x1818
    pub command_dmm: u32,                               // 0x181c
    pub mhf_mutex_number: u32,                          // 0x1820
    pub instance_mutex: HANDLE,                         // 0x1824
    pub master_ready_mutex: HANDLE,                     // 0x1828
    pub mutex_name: [u8; 0x40],                         // 0x182c
    pub ini_name: [u8; 0x40],                           // 0x186c
    pub host_callback_release: u32,                     // 0x18ac
    pub host_callback_state: u32,                       // 0x18b0
    pub host_callback_query: u32,                       // 0x18b4
    pub host_callback_result: u32,                      // 0x18b8
    pub status_code: u32,                               // 0x18bc
    pub error_code: u32,                                // 0x18c0
    pub selected_character_id_1: u32,                   // 0x18c4, selected_char_id_1
    pub selected_character_id_2: u32,                   // 0x18c8, selected_char_id_2
    pub sign_session_id: u32,                           // 0x18cc, user_token_id
    pub sign_session_token: [u8; 16],                   // 0x18d0, user_token
    pub reserved_18e0: [u8; 8],                         // 0x18e0
    pub sign_session_issued_at: u32,                    // 0x18e8, server_current_ts
    pub fixed_18ec_zero: u32,                           // 0x18ec, fixed_server_zero
    pub reserved_18f0: [u8; 0x200],                     // 0x18f0
    pub entrance_server_address: [u8; 0x100],           // 0x1af0, remote_addr
    pub entrance_server_host: [u8; 0x100],              // 0x1bf0, remote_host
    pub patch_server_count: u32,                        // 0x1cf0, remote_patch_count
    pub entrance_server_count: u32,                     // 0x1cf4, server_entrance_count
    pub selected_character_status: u32,                 // 0x1cf8, selected_char_status
    pub course_rights: u32,                             // 0x1cfc, user_rights
    pub selected_character_hr: u32,                     // 0x1d00, selected_char_hr
    pub selected_character_name: [u8; 16],              // 0x1d04, selected_char_name
    pub character_ids: [u32; 16],                       // 0x1d14, char_ids
    pub global_alloc: HGLOBAL,                          // 0x1d54
    pub fixed_1d58_one: u32,                            // 0x1d58, fixed_global_alloc_one
    pub unknown_1d5c: u32,                              // 0x1d5c
    pub selected_character_gr: u32,                     // 0x1d60, selected_char_gr
    pub reserved_1d64: [u8; 8],                         // 0x1d64
    pub preset_level: u32,                              // 0x1d6c
    pub custom: u32,                                    // 0x1d70
    pub screen_mode: u32,                               // 0x1d74, fullscreen_mode
    pub window_width: u32,                              // 0x1d78, window_resolution_w
    pub window_height: u32,                             // 0x1d7c, window_resolution_h
    pub fullscreen_width: u32,                          // 0x1d80, fullscreen_resolution_w
    pub fullscreen_height: u32,                         // 0x1d84, fullscreen_resolution_h
    pub display_character_limit: u32,                   // 0x1d88, disp_max_char
    pub use_dxt_textures: u32,                          // 0x1d8c, texture_dxt_use
    pub now_monitor_wh: u32,                            // 0x1d90
    pub graphics_version: u32,                          // 0x1d94
    pub sound_disabled: u32,                            // 0x1d98, sound_notuse
    pub sound_volume: u32,                              // 0x1d9c
    pub inactive_sound_volume: u32,                     // 0x1da0, sound_volume_inactivity
    pub minimized_sound_volume: u32,                    // 0x1da4, sound_volume_minimize
    pub sound_sample_rate: u32,                         // 0x1da8, sound_frequency
    pub sound_buffer_size: u32,                         // 0x1dac, sound_buffer_num
    pub language: u32,                                  // 0x1db0
    pub font_quality: u32,                              // 0x1db4
    pub font_weight: u32,                               // 0x1db8
    pub font_name: [u8; 0x68],                          // 0x1dbc
    pub draw_skip: u32,                                 // 0x1e24, drawskip
    pub clog_disabled: u32,                             // 0x1e28, clogdis
    pub use_proxy: u32,                                 // 0x1e2c, proxy_use
    pub use_ie_proxy: u32,                              // 0x1e30, proxy_ie
    pub proxy_configured: u32,                          // 0x1e34, proxy_set
    pub proxy_address: [u8; 0x40],                      // 0x1e38, proxy_addr
    pub proxy_port: u32,                                // 0x1e78
    pub server_selection: u32,                          // 0x1e7c, server_sel
    pub host_services: u32,                             // 0x1e80
    pub reserved_1e84: [u8; 0x40],                      // 0x1e84
    pub reserved_1ec4: [u8; 0x40],                      // 0x1ec4
    pub alternate_entrance_server_address: [u8; 0x100], // 0x1f04, alt_ip_address
    pub return_expires_at: u32,                         // 0x2004, server_expiry_ts
    pub unknown_2008: u32,                              // 0x2008, remote_16e
    pub fixed_200c_one: u32,                            // 0x200c, fixed_server_one
}

pub(crate) type GameMain = unsafe extern "C" fn(*mut MhfLaunchParams32) -> i32;

impl Default for MhfLaunchParams32 {
    fn default() -> Self {
        // SAFETY: every field accepts an all-zero representation.
        unsafe { std::mem::zeroed() }
    }
}

pub(crate) fn ptr32<T>(pointer: *mut T) -> u32 {
    pointer as usize as u32
}

pub(crate) fn function32(pointer: *const ()) -> u32 {
    pointer as usize as u32
}

#[repr(C)]
pub(crate) struct MhfGlobalData32 {
    reserved_0000: [u8; 0x0a0c],
    pub(crate) notice_lengths: [u32; LOGIN_NOTICE_SLOTS],
    reserved_0a1c: [u8; 8],
    pub(crate) notice_flags: [u16; LOGIN_NOTICE_SLOTS],
    pub(crate) notices: [[u8; LOGIN_NOTICE_BYTES]; LOGIN_NOTICE_SLOTS],
    reserved_4a2c: [u8; 0x4080],
    pub(crate) festa_id: u32,
    pub(crate) festa_starts_at: u32,
    pub(crate) festa_expires_at: u32,
    pub(crate) festa_solo_tickets: u32,
    pub(crate) festa_group_tickets: u32,
    pub(crate) festa_stalls: [u32; FESTA_STALL_SLOTS],
}

impl Default for MhfGlobalData32 {
    fn default() -> Self {
        // SAFETY: every field accepts an all-zero representation.
        unsafe { std::mem::zeroed() }
    }
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
