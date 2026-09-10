//! A DLL-owned target hooked only through the host's public C Hook API.

#[path = "probe.rs"]
mod probe;

use mhf_mod_sdk::{Error, Host, Mod, Result, abi as api, export_mod};
use probe::{HookProbeV1, HookSnapshotV1, Target};
pub use probe::{INTERFACE_ID, PROVIDER_ID};
use std::{
    ffi::c_void,
    hint::black_box,
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering},
    },
};

static CALLBACKS: AtomicPtr<Callbacks> = AtomicPtr::new(std::ptr::null_mut());

#[derive(Default)]
struct Callbacks {
    original: AtomicUsize,
    active: Mutex<u32>,
    idle: Condvar,
    entered: AtomicU32,
    completed: AtomicU32,
    drains: AtomicU32,
}

struct Invocation<'a>(&'a Callbacks);

impl Callbacks {
    fn enter(&self) -> Invocation<'_> {
        *self.active.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.entered.fetch_add(1, Ordering::Relaxed);
        Invocation(self)
    }
}

impl Drop for Invocation<'_> {
    fn drop(&mut self) {
        self.0.completed.fetch_add(1, Ordering::Relaxed);
        let mut active = self.0.active.lock().unwrap_or_else(PoisonError::into_inner);
        *active -= 1;
        if *active == 0 {
            self.0.idle.notify_all();
        }
    }
}

struct HookMod<'host> {
    host: Host<'host>,
    callbacks: Arc<Callbacks>,
    table: Rc<HookProbeV1>,
}

impl<'host> Mod<'host> for HookMod<'host> {
    fn create(host: Host<'host>) -> Result<Self> {
        let callbacks = Arc::<Callbacks>::default();
        let table = Rc::new(HookProbeV1 {
            context: Arc::as_ptr(&callbacks).cast_mut().cast(),
            invoke,
            snapshot,
        });
        Ok(Self {
            host,
            callbacks,
            table,
        })
    }

    fn attach(&mut self) -> Result<()> {
        if unsafe { invoke(10) } != 11 {
            return Err(Error::new("the example target is already modified"));
        }
        let callbacks = Arc::as_ptr(&self.callbacks).cast_mut().cast();
        // The instance owns callback storage through successful disable/drain;
        // no new probe calls are issued between stop and completed detach.
        let mut group = unsafe {
            mhf_mod_sdk::hooks::hooks(self.host).prepare_group(
                "example target",
                callbacks,
                Some(drain),
            )?
        };
        let original = unsafe { group.create_hook(target as *mut c_void, detour as *mut c_void)? };
        self.callbacks
            .original
            .store(original as usize, Ordering::Release);
        CALLBACKS.store(callbacks.cast(), Ordering::Release);
        group.enable()?;
        if unsafe { invoke(10) } != 111 {
            return Err(Error::new("the host did not enable the example detour"));
        }
        unsafe {
            mhf_mod_sdk::host::register_interface(
                self.host,
                INTERFACE_ID,
                (&*self.table as *const HookProbeV1).cast(),
            )
        }
    }
}

impl Drop for HookMod<'_> {
    fn drop(&mut self) {
        // The host never destroys a Mod until all owned hooks have drained.
        CALLBACKS.store(std::ptr::null_mut(), Ordering::Release);
    }
}

#[inline(never)]
unsafe extern "C" fn target(value: u32) -> u32 {
    black_box(value).wrapping_add(1)
}

unsafe extern "C" fn invoke(value: u32) -> u32 {
    // Keep an actual indirect call through the patched entry even with LTO.
    unsafe { black_box(target as Target)(value) }
}

unsafe extern "C" fn detour(value: u32) -> u32 {
    let callbacks = unsafe { &*CALLBACKS.load(Ordering::Acquire) };
    let _invocation = callbacks.enter();
    let original: Target =
        unsafe { std::mem::transmute(callbacks.original.load(Ordering::Acquire)) };
    unsafe { original(value) }.wrapping_add(100)
}

unsafe extern "C" fn drain(state: *mut c_void) -> api::Status {
    let callbacks = unsafe { &*state.cast::<Callbacks>() };
    let mut active = callbacks
        .active
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    while *active != 0 {
        active = callbacks
            .idle
            .wait(active)
            .unwrap_or_else(PoisonError::into_inner);
    }
    callbacks.drains.fetch_add(1, Ordering::Relaxed);
    api::OK
}

unsafe extern "C" fn snapshot(state: *mut c_void, out: *mut HookSnapshotV1) -> api::Status {
    let callbacks = unsafe { &*state.cast::<Callbacks>() };
    let active = *callbacks
        .active
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    unsafe {
        out.write(HookSnapshotV1 {
            entered: callbacks.entered.load(Ordering::Relaxed),
            completed: callbacks.completed.load(Ordering::Relaxed),
            active,
            drains: callbacks.drains.load(Ordering::Relaxed),
        });
    }
    api::OK
}

export_mod!(HookMod);
