use crate::{Error, ErrorKind, Result};
use crate::{abi as api, error::error_from_status};
use std::{ffi::c_void, fmt, marker::PhantomData, path::PathBuf, ptr::NonNull, rc::Rc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// A lifecycle-thread host handle. Neither Send nor Sync. Its lifetime bounds
/// dependency bindings; providers outlive their consumer's destruction.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Host<'host> {
    pub(crate) raw: &'host crate::abi::HostV2,
    pub(crate) _thread: PhantomData<Rc<()>>,
}

impl<'host> Host<'host> {
    #[inline]
    pub fn log(self, level: LogLevel, message: &str) {
        log(self, level, message);
    }

    #[inline]
    pub fn config(self) -> Result<String> {
        config(self)
    }

    #[inline]
    pub fn resource_root(self) -> Result<PathBuf> {
        resource_root(self)
    }

    #[inline]
    pub fn game_info(self) -> Result<GameInfo> {
        game_info(self)
    }

    #[inline]
    pub fn dependencies(self) -> Dependencies<'host> {
        Dependencies { host: self }
    }
}

/// This Mod's published interfaces and its declared dependencies. A provider's Rust SDK accepts this handle
/// and performs its explicit ABI binding internally.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Dependencies<'host> {
    pub(crate) host: Host<'host>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Prepare,
    Check,
    Attach,
    Running,
    Stop,
    Detach,
    Destroy,
}

/// Opaque identity of the currently loaded game module. This token neither
/// retains the DLL nor grants a native-memory borrow. Native use must still
/// satisfy the active game phase and calling-thread contract. Raw addresses are
/// available through this module's explicit native helpers.
#[safer_ffi::derive_ReprC]
#[repr(transparent)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct GameModule(pub(crate) NonNull<c_void>);

impl fmt::Debug for GameModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GameModule(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameInfo {
    /// Absent before game loading. A present token does not outlive the game's
    /// active module simply because its consumer instance still exists.
    pub module: Option<GameModule>,
    pub phase: Phase,
}

/// # Safety
/// The table, context and callable code must satisfy mhf_mod.h for `'host`.
/// Calls must occur on the Mod lifecycle thread. The host and dependency
/// providers must remain alive through this Mod's destruction.
#[inline]
pub unsafe fn host_from_raw<'host>(raw: *const api::HostV2) -> Host<'host> {
    Host {
        raw: unsafe { &*raw },
        _thread: PhantomData,
    }
}

#[inline]
pub fn host_raw(host: Host<'_>) -> &api::HostV2 {
    host.raw
}

/// # Safety
/// A non-null pointer must identify the currently loaded game module. The token
/// does not retain it; future native use must check its own phase/thread rules.
#[inline]
pub unsafe fn game_module_from_raw(pointer: *mut c_void) -> Option<GameModule> {
    NonNull::new(pointer).map(GameModule)
}

#[inline]
pub fn game_module_ptr(module: GameModule) -> *mut c_void {
    module.0.as_ptr()
}

/// Register during prepare or attach; published after that phase succeeds.
///
/// # Safety
/// The table and referenced state must implement the public interface contract
/// and remain at stable addresses through this Mod's destruction. All callbacks
/// must prevent unwind across the C boundary.
pub unsafe fn register_interface(
    host: Host<'_>,
    interface_id: &str,
    table: *const c_void,
) -> Result<()> {
    let status = unsafe {
        (host.raw.register_interface)(host.raw.context, api::Str::new(interface_id), table)
    };
    check(host, status)
}

#[inline]
pub(crate) fn log(host: Host<'_>, level: LogLevel, message: &str) {
    let level = match level {
        LogLevel::Error => api::LOG_ERROR,
        LogLevel::Warn => api::LOG_WARN,
        LogLevel::Info => api::LOG_INFO,
        LogLevel::Debug => api::LOG_DEBUG,
        LogLevel::Trace => api::LOG_TRACE,
    };
    unsafe { (host.raw.log)(host.raw.context, level, api::Str::new(message)) }
}

#[inline]
pub(crate) fn config(host: Host<'_>) -> Result<String> {
    read_host_text(host, host.raw.config)
}

#[inline]
pub(crate) fn resource_root(host: Host<'_>) -> Result<PathBuf> {
    read_host_text(host, host.raw.resource_root).map(PathBuf::from)
}

pub(crate) fn game_info(host: Host<'_>) -> Result<GameInfo> {
    let mut output = std::mem::MaybeUninit::uninit();
    let status = unsafe { (host.raw.game_info)(host.raw.context, output.as_mut_ptr()) };
    check(host, status)?;
    let output: api::GameInfoV2 = unsafe { output.assume_init() };
    let phase = match output.phase {
        api::PHASE_PREPARE => Phase::Prepare,
        api::PHASE_CHECK => Phase::Check,
        api::PHASE_ATTACH => Phase::Attach,
        api::PHASE_RUNNING => Phase::Running,
        api::PHASE_STOP => Phase::Stop,
        api::PHASE_DETACH => Phase::Detach,
        api::PHASE_DESTROY => Phase::Destroy,
        _ => {
            return Err(Error::with_kind(
                ErrorKind::InvalidState,
                "host returned an unknown lifecycle phase",
            ));
        }
    };
    Ok(GameInfo {
        module: unsafe { game_module_from_raw(output.module_base) },
        phase,
    })
}

pub(crate) fn check(host: Host<'_>, status: api::Status) -> Result<()> {
    if status == api::OK {
        return Ok(());
    }
    let message = read_text(host.raw, host.raw.last_error)
        .ok()
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("host operation failed ({status})"));
    Err(error_from_status(status, message))
}

fn read_host_text(host: Host<'_>, read: api::ReadTextFn) -> Result<String> {
    match read_text(host.raw, read) {
        Ok(value) => Ok(value),
        Err(status) => {
            check(host, status)?;
            unreachable!("a failed read has nonzero status")
        }
    }
}

fn read_text(
    host: &api::HostV2,
    read: api::ReadTextFn,
) -> std::result::Result<String, api::Status> {
    let mut required = 0;
    let status = unsafe { read(host.context, std::ptr::null_mut(), 0, &mut required) };
    if status != api::OK && status != api::BUFFER_TOO_SMALL {
        return Err(status);
    }
    if required == 0 {
        return Ok(String::new());
    }
    let mut buffer = vec![0; required as usize];
    let status = unsafe { read(host.context, buffer.as_mut_ptr(), required, &mut required) };
    if status != api::OK {
        return Err(status);
    }
    buffer.truncate(required as usize);
    String::from_utf8(buffer).map_err(|_| api::ERROR)
}
