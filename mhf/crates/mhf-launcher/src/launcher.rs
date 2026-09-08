use crate::{
    GraphicsVersion, MhfConfig, MhfLaunchParams32, MhfLaunchProfile,
    abi::{
        GameMain, HostServices32, MhfGlobalData32, MhfHostData32, copy_ascii_c_string,
        copy_utf8_c_string, function32, ptr32,
    },
};
#[cfg(feature = "login")]
use crate::{SignInSuccess, sign::Config};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    sync::atomic::{AtomicPtr, Ordering},
};
#[cfg(feature = "login")]
use windows::{
    Win32::System::Memory::{GlobalLock, GlobalUnlock},
    core::Error,
};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HGLOBAL, HINSTANCE, HMODULE},
        System::{
            LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA},
            Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc},
            Threading::{CreateMutexA, GetCurrentProcessId},
        },
        UI::Input::KeyboardAndMouse::GetKeyboardLayout,
    },
    core::{Owned, PCSTR},
};

const MHFO_MAIN: &CStr = c"mhDLL_Main";

static HOST_MESSAGE: AtomicPtr<c_char> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) enum LaunchMode<'a> {
    #[cfg(feature = "login")]
    Online(&'a Config),
    #[cfg(feature = "debug")]
    Debug {
        settings: &'a MhfConfig,
        translation: Option<&'a crate::TranslationConfig>,
        session: crate::debug::DebugSession,
    },
}

pub(crate) fn launch(
    profile: &MhfLaunchProfile<'_>,
    game_dir: &str,
    mode: LaunchMode<'_>,
) -> Result<i32, String> {
    let (settings, translation) = match &mode {
        #[cfg(feature = "login")]
        LaunchMode::Online(config) => (&config.mhf, config.translation.as_ref()),
        #[cfg(feature = "debug")]
        LaunchMode::Debug {
            settings,
            translation,
            ..
        } => {
            if settings.video.graphics_version != GraphicsVersion::HighDefinition {
                return Err("离线调试目前需要 HD 客户端".into());
            }
            (*settings, *translation)
        }
    };
    let _host_message = set_host_message(profile.host_message)?;

    let process_id = unsafe { GetCurrentProcessId() };
    let mutex_name_text = format!("{} {process_id}", profile.instance_mutex_prefix);
    let ready_name_text = format!("{} {process_id}", profile.ready_mutex_prefix);
    let mutex_name = CString::new(mutex_name_text.as_str())
        .map_err(|_| "instance mutex prefix must not contain NUL".to_owned())?;
    let ready_name = CString::new(ready_name_text.as_str())
        .map_err(|_| "ready mutex prefix must not contain NUL".to_owned())?;
    let instance_mutex = create_unique_mutex(&mutex_name)?;
    let ready_mutex = create_unique_mutex(&ready_name)?;

    // Keep the 0x2010-byte parameter block adjacent to the launcher globals
    // that its embedded pointers reference, matching mhf-iel's host layout.
    let mut data = Box::new(MhfHostData32::default());
    data.data_ptr = ptr32(&mut data.params);
    data.keyboard_layout = ptr32(unsafe { GetKeyboardLayout(0) }.0);
    data.host_services = HostServices32::new(
        data.host_request.as_mut_ptr(),
        data.host_response.as_mut_ptr(),
        function32(host_validate as *const ()),
        function32(host_message as *const ()),
    );
    data.params = MhfLaunchParams32 {
        module_instance: module_handle()?,
        mhf_mutex_number: 0,
        instance_mutex: *instance_mutex,
        master_ready_mutex: *ready_mutex,
        host_callback_release: function32(guard_release as *const ()),
        host_callback_state: function32(guard_state as *const ()),
        host_callback_query: function32(guard_query as *const ()),
        host_services: ptr32(&mut data.host_services),
        ..Default::default()
    };
    fill_launcher_fields(&mut data.params, profile, game_dir, &mutex_name_text)?;
    apply_config(&mut data.params, settings)?;
    copy_ascii_c_string(
        "ready mutex name",
        &mut data.ready_mutex_name,
        &ready_name_text,
    )?;

    let game_global_alloc = allocate_global()?;
    data.params.global_alloc = *game_global_alloc;
    match &mode {
        #[cfg(feature = "login")]
        LaunchMode::Online(config) => {
            initialize_global_data(*game_global_alloc, &config.sign_in)?;
            apply_sign_in(&mut data.params, config)?;
        }
        #[cfg(feature = "debug")]
        LaunchMode::Debug { .. } => {
            data.params.selected_character_id_1 = 1;
            data.params.selected_character_id_2 = 1;
            data.params.character_ids[0] = 1;
            data.params.fixed_1d58_one = 1;
            data.params.fixed_200c_one = 1;
            copy_ascii_c_string(
                "debug hunter",
                &mut data.params.selected_character_name,
                "Debug",
            )?;
        }
    }

    let game_name = match settings.video.graphics_version {
        GraphicsVersion::Standard => profile.mhfo_dll,
        GraphicsVersion::HighDefinition => profile.mhfo_hd_dll,
    };
    let game = MhfoModule::load(game_name)?;
    let entry = game.main()?;
    let mut geometry = if settings.video.graphics_version == GraphicsVersion::HighDefinition {
        Some(unsafe { mhf_geometry::install(game.handle()) }?)
    } else {
        None
    };
    // Resource ingress always converts legacy text, even without a translation.
    let mut localization = unsafe { crate::localization::install(game.handle(), translation) }?;
    data.mhfo_module = game.handle();
    data.mhfo_main = Some(entry);
    #[cfg(feature = "debug")]
    let debug = match mode {
        #[cfg(feature = "login")]
        LaunchMode::Online(_) => None,
        LaunchMode::Debug { session, .. } => Some(session),
    };
    #[cfg(feature = "debug")]
    let control = debug.as_ref().map(crate::debug::DebugSession::control);
    #[cfg(feature = "debug")]
    let mut debug = debug
        .map(|session| unsafe { crate::debug::install(game.handle(), session) })
        .transpose()?;
    let overlay = unsafe {
        crate::overlay::install(
            game.handle(),
            #[cfg(feature = "debug")]
            control,
        )
    }?;
    let mut text = unsafe { crate::text::install(game.handle(), &data.params.font_name) }?;
    let code = unsafe { entry(&mut data.params) };

    // Stop hooks before unloading the DLL. The localization guard retains the
    // module and translated buffers until cleanup (including DllMain) finishes.
    let overlay_cleanup = overlay.uninstall().map_err(|error| error.to_string());
    let text_cleanup = text.uninstall();
    #[cfg(feature = "debug")]
    let debug_cleanup = debug.as_mut().map(|debug| debug.uninstall()).transpose();
    let geometry_cleanup = geometry
        .as_mut()
        .map(|geometry| geometry.uninstall())
        .transpose();
    let localization_cleanup = localization.uninstall();
    drop(game);
    let errors = [
        overlay_cleanup.err(),
        text_cleanup.err(),
        #[cfg(feature = "debug")]
        debug_cleanup.err(),
        geometry_cleanup.err(),
        localization_cleanup.err(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(code)
    } else {
        Err(errors.join("; "))
    }
}

fn fill_launcher_fields(
    params: &mut MhfLaunchParams32,
    profile: &MhfLaunchProfile<'_>,
    game_dir: &str,
    mutex_name: &str,
) -> Result<(), String> {
    copy_ascii_c_string("game directory", &mut params.game_dir, game_dir)?;
    copy_ascii_c_string("launcher directory", &mut params.launcher_dir, game_dir)?;
    copy_ascii_c_string("mutex name", &mut params.mutex_name, mutex_name)?;
    copy_ascii_c_string("INI name", &mut params.ini_name, profile.ini_name)
}

fn apply_config(params: &mut MhfLaunchParams32, config: &MhfConfig) -> Result<(), String> {
    params.preset_level = config.set.preset_level;
    params.custom = u32::from(config.set.custom);
    params.screen_mode = config.screen.mode.into();
    params.window_width = config.screen.window_resolution.width;
    params.window_height = config.screen.window_resolution.height;
    params.fullscreen_width = config.screen.fullscreen_resolution.width;
    params.fullscreen_height = config.screen.fullscreen_resolution.height;
    params.display_character_limit = config.video.display_character_limit;
    params.use_dxt_textures = u32::from(config.video.use_dxt_textures);
    params.now_monitor_wh = u32::from(config.video.now_monitor_wh);
    params.graphics_version = config.video.graphics_version.into();
    params.sound_disabled = u32::from(config.sound.disabled);
    params.sound_volume = config.sound.volume;
    params.inactive_sound_volume = config.sound.inactive_volume;
    params.minimized_sound_volume = config.sound.minimized_volume;
    params.sound_sample_rate = config.sound.sample_rate;
    params.sound_buffer_size = config.sound.buffer_size;
    params.language = config.localization.language.into();
    params.font_quality = config.font.quality.into();
    params.font_weight = u32::from(config.font.weight);
    copy_utf8_c_string("font name", &mut params.font_name, &config.font.name)?;
    params.draw_skip = u32::from(config.option.draw_skip);
    params.clog_disabled = u32::from(config.option.clog_disabled);
    params.use_proxy = u32::from(config.launch.use_proxy);
    params.use_ie_proxy = u32::from(config.launch.use_ie_proxy);
    params.proxy_configured = u32::from(config.launch.proxy_configured);
    copy_ascii_c_string(
        "proxy address",
        &mut params.proxy_address,
        &config.launch.proxy_address.to_string(),
    )?;
    params.proxy_port = u32::from(config.launch.proxy_port);
    params.server_selection = config.launch.server_selection;
    Ok(())
}

#[cfg(feature = "login")]
fn apply_sign_in(params: &mut MhfLaunchParams32, config: &Config) -> Result<(), String> {
    let sign_in = &config.sign_in;
    let character = sign_in.selected_character(config.selected_character_id)?;
    let entrance_server = sign_in
        .entrance_servers
        .first()
        .expect("validated sign-in result has an entrance server");
    let entrance_host = entrance_server.ip().to_string();
    let alternate_address = format!("{}:8080", entrance_server.ip());

    copy_utf8_c_string(
        "selected character name",
        &mut params.selected_character_name,
        &character.name,
    )?;
    copy_utf8_c_string(
        "username",
        &mut params.username,
        &config.credentials.username,
    )?;
    copy_utf8_c_string(
        "password",
        &mut params.password,
        &config.credentials.password,
    )?;
    copy_ascii_c_string(
        "entrance server host",
        &mut params.entrance_server_host,
        &entrance_host,
    )?;
    copy_ascii_c_string(
        "entrance server address",
        &mut params.entrance_server_address,
        &entrance_server.to_string(),
    )?;
    copy_ascii_c_string(
        "alternate entrance server address",
        &mut params.alternate_entrance_server_address,
        &alternate_address,
    )?;

    let character_id = u32::from(character.id);
    params.selected_character_id_1 = character_id;
    params.selected_character_id_2 = character_id;
    params.sign_session_id = u32::from(sign_in.session.session_id);
    params
        .sign_session_token
        .copy_from_slice(&sign_in.session.token);
    params.sign_session_issued_at =
        unix_timestamp32("session issued_at", &sign_in.session.issued_at)?;
    params.fixed_18ec_zero = 0;
    params.patch_server_count = 0;
    params.entrance_server_count = u32::try_from(sign_in.entrance_servers.len())
        .map_err(|_| "entrance server count exceeds u32".to_owned())?;
    params.selected_character_status = if character.is_new { 2 } else { 0 };
    params.course_rights = sign_in.rights.bits();
    params.selected_character_hr = u32::from(character.hr);
    params.character_ids.fill(0);
    for (target, character) in params.character_ids.iter_mut().zip(&sign_in.characters) {
        *target = u32::from(character.id);
    }
    params.fixed_1d58_one = 1;
    params.selected_character_gr = u32::from(character.gr);
    params.return_expires_at = unix_timestamp32("return_expires_at", &sign_in.return_expires_at)?;
    params.fixed_200c_one = 1;
    Ok(())
}

#[cfg(feature = "login")]
fn unix_timestamp32(field: &str, timestamp: &jiff::Timestamp) -> Result<u32, String> {
    u32::try_from(timestamp.as_second())
        .map_err(|_| format!("{field} is outside the unsigned 32-bit Unix timestamp range"))
}

#[cfg(feature = "login")]
fn initialize_global_data(global_alloc: HGLOBAL, sign_in: &SignInSuccess) -> Result<(), String> {
    let pointer = unsafe { GlobalLock(global_alloc) };
    if pointer.is_null() {
        return Err(format!("GlobalLock failed: {}", Error::from_thread()));
    }

    let result = apply_global_sign_in(unsafe { &mut *pointer.cast::<MhfGlobalData32>() }, sign_in);
    let unlock_result = unsafe { GlobalUnlock(global_alloc) };
    result?;

    match unlock_result {
        Ok(()) => Ok(()),
        Err(error) if error.code().is_ok() => Ok(()),
        Err(error) => Err(format!("GlobalUnlock failed: {error}")),
    }
}

#[cfg(feature = "login")]
fn apply_global_sign_in(data: &mut MhfGlobalData32, sign_in: &SignInSuccess) -> Result<(), String> {
    let notice_slots = data.notices.len();
    let notice_bytes = data.notices[0].len();
    if sign_in.notices.len() > notice_slots {
        return Err(format!(
            "sign-in result has {} notices; at most {notice_slots} are supported",
            sign_in.notices.len()
        ));
    }
    let notices = sign_in
        .notices
        .iter()
        .enumerate()
        .map(|(index, notice)| {
            let encoded = notice.as_bytes();
            if encoded.contains(&0) {
                return Err(format!("sign-in notice {} contains a NUL byte", index + 1));
            }
            if encoded.len() > notice_bytes {
                return Err(format!(
                    "sign-in notice {} is {} encoded bytes; at most {notice_bytes} bytes are supported",
                    index + 1,
                    encoded.len()
                ));
            }
            Ok(encoded)
        })
        .collect::<Result<Vec<_>, String>>()?;

    let festa_stall_slots = data.festa_stalls.len();
    let festa = sign_in
        .festa
        .as_ref()
        .map(|festa| {
            if festa.id == 0 {
                return Err("active Mezeporta Festa ID must not be 0".to_owned());
            }
            if festa.stalls.len() > festa_stall_slots {
                return Err(format!(
                    "Mezeporta Festa has {} stalls; at most {festa_stall_slots} are supported",
                    festa.stalls.len()
                ));
            }
            Ok((
                festa,
                unix_timestamp32("Festa starts_at", &festa.period.starts_at())?,
                unix_timestamp32("Festa expires_at", &festa.period.expires_at())?,
            ))
        })
        .transpose()?;

    data.notice_lengths.fill(0);
    data.notice_flags.fill(0);
    for notice in &mut data.notices {
        notice.fill(0);
    }
    for (index, notice) in notices.iter().enumerate() {
        data.notice_lengths[index] = notice.len() as u32;
        data.notices[index][..notice.len()].copy_from_slice(notice);
    }

    data.festa_id = 0;
    data.festa_starts_at = 0;
    data.festa_expires_at = 0;
    data.festa_solo_tickets = 0;
    data.festa_group_tickets = 0;
    data.festa_stalls.fill(0);
    if let Some((festa, starts_at, expires_at)) = festa {
        data.festa_id = festa.id;
        data.festa_starts_at = starts_at;
        data.festa_expires_at = expires_at;
        data.festa_solo_tickets = festa.solo_ticket_allowance;
        data.festa_group_tickets = festa.group_ticket_allowance;
        for (target, stall) in data.festa_stalls.iter_mut().zip(&festa.stalls) {
            *target = u32::from(*stall as u8);
        }
    }

    Ok(())
}

fn set_host_message(message: &str) -> Result<CString, String> {
    let message =
        CString::new(message).map_err(|_| "host message must not contain NUL".to_owned())?;
    HOST_MESSAGE.store(message.as_ptr().cast_mut(), Ordering::Relaxed);
    Ok(message)
}

fn module_handle() -> Result<HINSTANCE, String> {
    let handle = unsafe { GetModuleHandleA(PCSTR::null()) }
        .map_err(|error| format!("GetModuleHandleA failed: {error}"))?;
    Ok(handle.into())
}

fn pcstr(value: &CStr) -> PCSTR {
    PCSTR(value.as_ptr().cast())
}

struct MhfoModule(Owned<HMODULE>);

impl MhfoModule {
    fn load(name: &str) -> Result<Self, String> {
        let name = CString::new(name).map_err(|_| "DLL name must not contain NUL".to_owned())?;
        let handle = unsafe { LoadLibraryA(pcstr(&name)) }
            .map_err(|error| format!("LoadLibraryA({}) failed: {error}", name.to_string_lossy()))?;
        Ok(Self(unsafe { Owned::new(handle) }))
    }

    fn handle(&self) -> HMODULE {
        *self.0
    }

    fn main(&self) -> Result<GameMain, String> {
        let address = unsafe { GetProcAddress(self.handle(), pcstr(MHFO_MAIN)) }
            .map(|address| address as *const () as *mut c_void)
            .ok_or_else(|| {
                let code = unsafe { GetLastError() }.0;
                format!("GetProcAddress(mhDLL_Main) failed with Win32 error {code}")
            })?;
        Ok(unsafe { std::mem::transmute::<*mut c_void, GameMain>(address) })
    }
}

fn create_unique_mutex(name: &CStr) -> Result<Owned<HANDLE>, String> {
    let handle = unsafe { CreateMutexA(None, false, pcstr(name)) }
        .map_err(|error| format!("CreateMutexA({}) failed: {error}", name.to_string_lossy()))?;
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = unsafe { Owned::new(handle) };
    if already_exists {
        Err(format!("{} already exists", name.to_string_lossy()))
    } else {
        Ok(handle)
    }
}

fn allocate_global() -> Result<Owned<HGLOBAL>, String> {
    let handle = unsafe {
        GlobalAlloc(
            GMEM_MOVEABLE | GMEM_ZEROINIT,
            std::mem::size_of::<MhfGlobalData32>(),
        )
    }
    .map_err(|error| format!("GlobalAlloc failed: {error}"))?;
    Ok(unsafe { Owned::new(handle) })
}

extern "C" fn guard_release(_context: *mut c_void) -> u32 {
    0
}

extern "C" fn guard_state() -> i32 {
    // mhf-iel's gg_proc returns success so mhfo can pass its launcher check.
    1
}

extern "C" fn guard_query(_context: *const c_void) -> i32 {
    0
}

extern "C" fn host_validate() -> i32 {
    0
}

extern "C" fn host_message() -> *const c_char {
    HOST_MESSAGE.load(Ordering::Relaxed)
}

#[cfg(all(test, feature = "login"))]
mod tests {
    use super::*;
    use crate::{IssuedSignSession, PasswordCredentials, SignCharacter, SignInSuccess};
    use jiff::{SignedDuration, Timestamp};
    use shrimpman_domain::{
        TimeRange,
        account::CourseRights,
        character::{CharacterId, Gender, WeaponType},
        mezeporta::{MezeportaFesta, MezeportaStall},
        session::SignSessionId,
    };

    #[test]
    fn sign_in_domain_maps_to_the_mhfo_abi() {
        let issued_at = Timestamp::new(1_700_000_000, 0).expect("valid test timestamp");
        let config = Config {
            credentials: PasswordCredentials {
                username: "账号é".to_owned(),
                password: "密碼🙂".to_owned(),
            },
            sign_in: SignInSuccess {
                session: IssuedSignSession {
                    session_id: SignSessionId::from(1),
                    token: *b"KySJuNnR2PJu00Uw",
                    issued_at,
                },
                entrance_servers: vec!["127.0.0.1:53310".parse().expect("valid entrance server")],
                characters: vec![SignCharacter {
                    id: CharacterId::from(1),
                    name: "啊啊🙂".to_owned(),
                    gr: 50,
                    hr: 999,
                    weapon_type: WeaponType::GreatSword,
                    gender: Gender::Male,
                    last_sign_in_at: Some(issued_at),
                    is_new: false,
                }],
                notices: vec!["Welcome".to_owned(), "テスト".to_owned()],
                last_character_id: Some(CharacterId::from(1)),
                rights: CourseRights::from_bits_retain(12),
                return_expires_at: Timestamp::new(i64::from(u32::MAX), 0)
                    .expect("valid expiry timestamp"),
                festa: Some(MezeportaFesta {
                    id: 7,
                    period: TimeRange::from_duration(issued_at, SignedDuration::from_hours(1)),
                    solo_ticket_allowance: 5,
                    group_ticket_allowance: 2,
                    stalls: vec![MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether],
                }),
            },
            selected_character_id: CharacterId::from(1),
            translation: None,
            mhf: MhfConfig::default(),
        };
        let mut params = MhfLaunchParams32::default();

        apply_sign_in(&mut params, &config).expect("domain config should fit the ABI");

        for (bytes, expected) in [
            (params.username.as_slice(), "账号é"),
            (params.password.as_slice(), "密碼🙂"),
            (params.selected_character_name.as_slice(), "啊啊🙂"),
        ] {
            assert_eq!(&bytes[..expected.len()], expected.as_bytes());
            assert_eq!(bytes[expected.len()], 0);
        }
        assert_eq!(params.selected_character_id_1, 1);
        assert_eq!(params.selected_character_id_2, 1);
        assert_eq!(params.sign_session_id, 1);
        assert_eq!(&params.sign_session_token, b"KySJuNnR2PJu00Uw");
        assert_eq!(params.sign_session_issued_at, 1_700_000_000);
        assert_eq!(params.entrance_server_count, 1);
        assert_eq!(&params.entrance_server_address[..15], b"127.0.0.1:53310");
        assert_eq!(&params.entrance_server_host[..9], b"127.0.0.1");
        assert_eq!(params.selected_character_hr, 999);
        assert_eq!(params.selected_character_gr, 50);
        assert_eq!(params.fixed_200c_one, 1);

        let mut global_data = MhfGlobalData32::default();
        apply_global_sign_in(&mut global_data, &config.sign_in)
            .expect("global Sign data should fit the ABI");
        assert_eq!(global_data.notice_lengths[..2], [7, 9]);
        assert_eq!(&global_data.notices[0][..7], b"Welcome");
        assert_eq!(&global_data.notices[1][..9], "テスト".as_bytes());
        assert_eq!(global_data.festa_id, 7);
        assert_eq!(global_data.festa_starts_at, 1_700_000_000);
        assert_eq!(global_data.festa_expires_at, 1_700_003_600);
        assert_eq!(global_data.festa_solo_tickets, 5);
        assert_eq!(global_data.festa_group_tickets, 2);
        assert_eq!(global_data.festa_stalls[..2], [3, 4]);

        let global_alloc = allocate_global().expect("global Sign data should allocate");
        initialize_global_data(*global_alloc, &config.sign_in)
            .expect("global Sign data should initialize");
    }
}
