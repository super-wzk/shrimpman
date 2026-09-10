use crate::{Error, ErrorKind, Host, LogLevel, Mod, Phase, Result};
use crate::{
    abi as api,
    error::{error_from_status, error_status},
    host::{game_module_ptr, host_from_raw, host_raw},
    interface::{Interface, bind},
};
use std::ffi::c_void;

#[derive(Default)]
struct State {
    logs: Vec<String>,
    levels: Vec<u32>,
    destroyed: bool,
    game: Option<api::GameInfoV2>,
}

unsafe extern "C" fn log(context: *mut c_void, level: u32, message: api::Str) {
    let state = unsafe { &mut *context.cast::<State>() };
    state.logs.push(unsafe { message.as_str() }.to_owned());
    state.levels.push(level);
}

unsafe fn text(value: &str, buffer: *mut u8, capacity: u32, required: *mut u32) -> api::Status {
    unsafe { required.write(value.len() as u32) };
    if capacity < value.len() as u32 {
        return api::BUFFER_TOO_SMALL;
    }
    if !value.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), buffer, value.len()) };
    }
    api::OK
}

unsafe extern "C" fn config(
    _: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    unsafe { text("name = \"字体\"\n", buffer, capacity, required) }
}

unsafe extern "C" fn last_error(
    _: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    unsafe { text("dependency is not declared", buffer, capacity, required) }
}

unsafe extern "C" fn register(_: *mut c_void, _: api::Str, _: *const c_void) -> api::Status {
    api::OK
}
unsafe extern "C" fn dependency(
    _: *mut c_void,
    _: api::Str,
    _: api::Str,
    _: *mut *const c_void,
) -> api::Status {
    api::NOT_FOUND
}
unsafe extern "C" fn game_info(context: *mut c_void, out: *mut api::GameInfoV2) -> api::Status {
    let state = unsafe { &*context.cast::<State>() };
    unsafe {
        out.write(state.game.unwrap_or(api::GameInfoV2 {
            module_base: std::ptr::null_mut(),
            phase: api::PHASE_PREPARE,
        }))
    };
    api::OK
}
unsafe extern "C" fn prepare_group(
    _: *mut c_void,
    _: api::Str,
    _: *mut c_void,
    _: Option<api::DrainFn>,
    _: *mut api::HookGroup,
) -> api::Status {
    api::INVALID_STATE
}
unsafe extern "C" fn create_hook(
    _: *mut c_void,
    _: api::HookGroup,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut *mut c_void,
) -> api::Status {
    api::INVALID_STATE
}
unsafe extern "C" fn change_group(_: *mut c_void, _: api::HookGroup) -> api::Status {
    api::INVALID_STATE
}

fn table(state: &mut State) -> api::HostV2 {
    api::HostV2 {
        context: (state as *mut State).cast(),
        log,
        config,
        resource_root: config,
        last_error,
        register_interface: register,
        dependency,
        game_info,
        hooks: api::HookApiV1 {
            prepare_group,
            create_hook,
            enable_group: change_group,
            discard_group: change_group,
        },
    }
}

struct PanickingMod<'host>(Host<'host>);

impl<'host> Mod<'host> for PanickingMod<'host> {
    fn create(host: Host<'host>) -> Result<Self> {
        Ok(Self(host))
    }
    fn attach(&mut self) -> Result<()> {
        panic!("synthetic attach failure")
    }
    fn detach(&mut self) -> Result<()> {
        Err(Error::new("detach failed"))
    }
}

impl Drop for PanickingMod<'_> {
    fn drop(&mut self) {
        unsafe { &mut *host_raw(self.0).context.cast::<State>() }.destroyed = true;
    }
}

#[test]
fn lifecycle_converts_panics_and_errors_before_crossing_c() {
    let mut state = State::default();
    let host = table(&mut state);
    let module = crate::lifecycle::table::<PanickingMod<'static>>();
    let mut instance = std::ptr::null_mut();
    unsafe {
        assert_eq!((module.create)(&host, &mut instance), api::OK);
        assert_eq!(module.attach.unwrap()(instance), api::ERROR);
        assert_eq!(module.detach.unwrap()(instance), api::ERROR);
        (module.destroy)(instance);
    }
    assert!(state.destroyed);
    assert_eq!(
        state.logs,
        ["mod panicked in lifecycle callback", "detach failed"]
    );
}

enum MissingInterface {}
unsafe impl Interface for MissingInterface {
    type Table = u32;
    const PROVIDER: &'static str = "missing";
    const ID: &'static str = "missing.v1";
}

#[test]
fn host_copies_utf8_and_preserves_dependency_error() {
    let mut state = State::default();
    let raw = table(&mut state);
    let host = unsafe { host_from_raw(&raw) };
    assert_eq!(host.config().unwrap(), "name = \"字体\"\n");
    let error = bind::<MissingInterface>(host.dependencies()).err().unwrap();
    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert_eq!(error_status(&error), api::NOT_FOUND);
    assert_eq!(error.to_string(), "dependency is not declared");
}

#[test]
fn typed_host_values_map_to_the_existing_c_protocol() {
    let mut state = State::default();
    let raw = table(&mut state);
    let host = unsafe { host_from_raw(&raw) };
    let game = host.game_info().unwrap();
    assert_eq!(game.phase, Phase::Prepare);
    assert!(game.module.is_none());
    host.log(LogLevel::Warn, "typed warning");
    assert_eq!(state.levels, [api::LOG_WARN]);

    // The mock supplies an opaque module identity. No native operation uses it.
    let mut image = 0_u8;
    let pointer = (&mut image as *mut u8).cast();
    state.game = Some(api::GameInfoV2 {
        module_base: pointer,
        phase: api::PHASE_RUNNING,
    });
    let game = host.game_info().unwrap();
    assert_eq!(game.phase, Phase::Running);
    assert_eq!(game_module_ptr(game.module.unwrap()), pointer);
}

#[test]
fn typed_errors_preserve_known_and_foreign_failure_codes() {
    let error = Error::with_kind(ErrorKind::Conflict, "duplicate hook");
    assert_eq!(error.kind(), ErrorKind::Conflict);
    assert_eq!(error_status(&error), api::CONFLICT);
    let foreign = error_from_status(42, "provider detail");
    assert_eq!(foreign.kind(), ErrorKind::Other);
    assert_eq!(foreign.to_string(), "provider detail");
    assert_eq!(error_status(&foreign), 42);
    assert_eq!(
        error_from_status(api::OK, "failure").kind(),
        ErrorKind::OperationFailed
    );
}
