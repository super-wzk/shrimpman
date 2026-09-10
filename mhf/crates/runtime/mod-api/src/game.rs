//! Fixed 32-bit client startup data. Providers borrow these buffers during
//! their launch callback; the game host owns their allocation and lifetime.
//! Host-populated pointers and handles must not be replaced by a provider.

use core::ops::{Deref, DerefMut};
use safer_ffi::derive_ReprC;

#[safer_ffi::cfg_headers]
extern crate std;

/// Inline byte storage for game structure fields whose lengths are unsupported
/// by safer-ffi's fixed array implementations. These fields cross the ABI only
/// inside the shared launch structures, never as standalone function arguments.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Bytes<const N: usize>([u8; N]);

impl<const N: usize> Default for Bytes<N> {
    fn default() -> Self {
        Self([0; N])
    }
}

impl<const N: usize> Deref for Bytes<N> {
    type Target = [u8; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> DerefMut for Bytes<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// SAFETY: the transparent field has exactly N bytes, alignment one and no
// invalid bit patterns, matching the generated inline C byte array.
unsafe impl<const N: usize> safer_ffi::layout::CType for Bytes<N> {
    type OPAQUE_KIND = safer_ffi::layout::OpaqueKind::Concrete;

    #[safer_ffi::cfg_headers]
    fn short_name() -> std::string::String {
        std::format!("MhfGameBytes{N}")
    }

    #[safer_ffi::cfg_headers]
    fn name(_: &dyn safer_ffi::headers::languages::HeaderLanguage) -> std::string::String {
        Self::short_name()
    }

    #[safer_ffi::cfg_headers]
    fn define_self__impl(
        language: &dyn safer_ffi::headers::languages::HeaderLanguage,
        definer: &mut dyn safer_ffi::headers::Definer,
    ) -> std::io::Result<()> {
        if !language.is::<safer_ffi::headers::languages::C>() || N == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "game byte fields require a nonempty C array",
            ));
        }
        <u8 as safer_ffi::layout::CType>::define_self(language, definer)?;
        std::writeln!(definer.out(), "typedef uint8_t MhfGameBytes{N}[{N}];\n")
    }
}

// SAFETY: Rust and generated C fields share the same layout and every byte
// pattern is valid. The wrapper adds no ownership or lifetime requirements.
unsafe impl<const N: usize> safer_ffi::layout::ReprC for Bytes<N> {
    type CLayout = Self;

    fn is_valid(_: &Self::CLayout) -> bool {
        true
    }
}

pub const LAUNCH_PARAMS_SIZE: usize = 0x2010;
pub const GLOBAL_DATA_SIZE: usize = 0x8ae0;
const LOGIN_NOTICE_SLOTS: usize = 4;
const LOGIN_NOTICE_BYTES: usize = 0x1000;
const FESTA_STALL_SLOTS: usize = 8;

// This 0x2010-byte block is passed to mhfo[-hd].dll. Comments retain mhf-iel's
// DataZZ names where they differ from the domain terminology used by Rust.
#[derive_ReprC]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LaunchParams32 {
    pub module_instance: u32,                           // 0x0000
    pub unknown_0004: [u8; 8],                          // 0x0004
    pub launch_mode: u32,                               // 0x000c
    pub launch_flags: u32,                              // 0x0010
    pub game_dir: [u8; 0x400],                          // 0x0014
    pub launcher_dir: [u8; 0x400],                      // 0x0414
    pub username: Bytes<0x800>,                         // 0x0814, user_name
    pub password: Bytes<0x800>,                         // 0x1014, user_password
    pub command_number: u32,                            // 0x1814
    pub command_netcf_update: u32,                      // 0x1818
    pub command_dmm: u32,                               // 0x181c
    pub mhf_mutex_number: u32,                          // 0x1820
    pub instance_mutex: u32,                            // 0x1824
    pub master_ready_mutex: u32,                        // 0x1828
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
    pub global_alloc: u32,                              // 0x1d54
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
    pub font_name: Bytes<0x68>,                         // 0x1dbc
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

impl Default for LaunchParams32 {
    fn default() -> Self {
        // Every field has an all-zero representation.
        unsafe { core::mem::zeroed() }
    }
}

#[derive_ReprC]
#[repr(C)]
pub struct GlobalData32 {
    reserved_0000: Bytes<0x0a0c>,
    pub notice_lengths: [u32; LOGIN_NOTICE_SLOTS],
    reserved_0a1c: [u8; 8],
    pub notice_flags: [u16; LOGIN_NOTICE_SLOTS],
    pub notices: [Bytes<LOGIN_NOTICE_BYTES>; LOGIN_NOTICE_SLOTS],
    reserved_4a2c: Bytes<0x4080>,
    pub festa_id: u32,
    pub festa_starts_at: u32,
    pub festa_expires_at: u32,
    pub festa_solo_tickets: u32,
    pub festa_group_tickets: u32,
    pub festa_stalls: [u32; FESTA_STALL_SLOTS],
}

impl Default for GlobalData32 {
    fn default() -> Self {
        // SAFETY: every field accepts an all-zero representation.
        unsafe { core::mem::zeroed() }
    }
}

const _: () = {
    assert!(core::mem::size_of::<LaunchParams32>() == LAUNCH_PARAMS_SIZE);
    assert!(core::mem::offset_of!(LaunchParams32, global_alloc) == 0x1d54);
    assert!(core::mem::offset_of!(LaunchParams32, font_name) == 0x1dbc);
    assert!(core::mem::size_of::<GlobalData32>() == GLOBAL_DATA_SIZE);
    assert!(core::mem::offset_of!(GlobalData32, notice_lengths) == 0x0a0c);
    assert!(core::mem::offset_of!(GlobalData32, festa_stalls) == 0x8ac0);
};
