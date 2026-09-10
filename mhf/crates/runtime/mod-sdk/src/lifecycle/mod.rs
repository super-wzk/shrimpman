//! The only lifetime erasure. The host owns the opaque instance and guarantees
//! its borrowed host/dependencies outlive destruction of this box.

use crate::{Host, LogLevel, Result};
use crate::{abi as api, error::error_status, host::host_from_raw};
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
};

/// Implement for a lifetime-parameterized instance, then `export_mod!(MyMod)`.
/// Keep host and dependency handles within that lifetime. The host creates and
/// calls instances in dependency order, and destroys consumers before providers.
pub trait Mod<'host>: Sized {
    fn create(host: Host<'host>) -> Result<Self>;
    fn prepare(&mut self) -> Result<()> {
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        Ok(())
    }
    fn attach(&mut self) -> Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
    fn detach(&mut self) -> Result<()> {
        Ok(())
    }
}

struct Instance<M> {
    host: Host<'static>,
    module: M,
}

fn guard(host: Host<'_>, operation: impl FnOnce() -> Result<()>) -> api::Status {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => api::OK,
        Ok(Err(error)) => {
            host.log(LogLevel::Error, &error.to_string());
            error_status(&error)
        }
        Err(_) => {
            host.log(LogLevel::Error, "mod panicked in lifecycle callback");
            api::ERROR
        }
    }
}

unsafe extern "C" fn create<M: Mod<'static>>(
    host: *const api::HostV2,
    out: *mut *mut c_void,
) -> api::Status {
    let host = unsafe { host_from_raw(host) };
    unsafe { out.write(std::ptr::null_mut()) };
    guard(host, || {
        let module = M::create(host)?;
        let instance = Box::new(Instance { host, module });
        unsafe { out.write(Box::into_raw(instance).cast()) };
        Ok(())
    })
}

macro_rules! stage {
    ($name:ident) => {
        unsafe extern "C" fn $name<M: Mod<'static>>(raw: *mut c_void) -> api::Status {
            let instance = unsafe { &mut *raw.cast::<Instance<M>>() };
            guard(instance.host, || instance.module.$name())
        }
    };
}
stage!(prepare);
stage!(check);
stage!(attach);
stage!(stop);
stage!(detach);

unsafe extern "C" fn destroy<M: Mod<'static>>(raw: *mut c_void) {
    let instance = unsafe { Box::from_raw(raw.cast::<Instance<M>>()) };
    let host = instance.host;
    let _ = guard(host, || {
        drop(instance);
        Ok(())
    });
}

#[doc(hidden)]
pub const fn table<M: Mod<'static>>() -> api::ModV2 {
    api::ModV2 {
        create: create::<M>,
        prepare: Some(prepare::<M>),
        check: Some(check::<M>),
        attach: Some(attach::<M>),
        stop: Some(stop::<M>),
        detach: Some(detach::<M>),
        destroy: destroy::<M>,
    }
}

/// Generate the fixed C entry through the explicit ABI bridge. MyMod must
/// implement Mod<'host> for every host lifetime, not just 'static.
#[macro_export]
macro_rules! export_mod {
    ($module:ident) => {
        const _: () = {
            fn require_all_lifetimes<'host>() {
                fn require<'host, M: $crate::Mod<'host>>() {}
                require::<'host, $module<'host>>();
            }
        };

        #[unsafe(no_mangle)]
        pub extern "C" fn mhf_mod_query_v2() -> *const $crate::abi::ModV2 {
            static TABLE: $crate::abi::ModV2 = $crate::lifecycle::table::<$module<'static>>();
            &TABLE
        }
    };
}
