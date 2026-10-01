//! Relocate the 128-KiB raw equipment caches at their native I/O boundaries.

use super::{BASE, SLOT, State, abi, memory};
use crate::{
    cache::{Buffer, Completion, Use},
    equipment_cache::{CACHE_REFERENCES, CACHE_RVAS, INVALID_RESOURCE, required_size},
};
use std::{
    ffi::{CStr, c_void},
    mem::transmute,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) const SYNC_LOAD: usize = 0x008e2338;
pub(super) const SYNC_PART_LOAD: usize = 0x008e22cb;
pub(super) const DISPATCH: usize = 0x015904c0;
const SYNC_BUILD: usize = 0x008e1b90;
const ASYNC_BUILD: usize = 0x008e1170;
const ASYNC_FINISH: usize = 0x008e13c0;
const RESET: usize = 0x015902d0;
pub(super) static SYNC_RETURN: AtomicUsize = AtomicUsize::new(0);
pub(super) static SYNC_PART_RETURN: AtomicUsize = AtomicUsize::new(0);

pub(super) struct CompletionOriginals {
    build: usize,
    reset: usize,
}

pub(super) unsafe fn create_completion_hooks(
    hooks: &mut mhf_hooks::HookSet<State>,
    base: usize,
) -> Result<CompletionOriginals, String> {
    unsafe {
        Ok(CompletionOriginals {
            build: hooks.create(
                "equipment synchronous batch completion",
                (base + SYNC_BUILD) as _,
                finish_sync as *mut c_void,
            )? as usize,
            reset: hooks.create(
                "equipment queue cancellation",
                (base + RESET) as _,
                reset as *mut c_void,
            )? as usize,
        })
    }
}

pub(super) unsafe fn validate(base: usize) -> Result<(), String> {
    unsafe {
        memory::check(base + SYNC_LOAD, &[0xe8, 0x93, 0x04, 0x00, 0x00])?;
        memory::check(base + SYNC_PART_LOAD, &[0xe8, 0x00, 0x05, 0x00, 0x00])?;
        memory::check(base + DISPATCH, &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08])?;
        memory::check(
            base + SYNC_BUILD,
            &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x83, 0xec, 0x1c],
        )?;
        let mut reset = [
            0x53, 0x56, 0x8b, 0x35, 0, 0, 0, 0, 0x33, 0xdb, 0x83, 0x3e, 0x06,
        ];
        reset[4..8].copy_from_slice(&((base + 0x0e879d40) as u32).to_le_bytes());
        memory::check(base + RESET, &reset)?;
        memory::check(
            base + 0xaff0,
            &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x08, 0x01, 0x00, 0x00],
        )?;
        // The unchanged producer addresses identify weapon requests.
        let mut sync_producer = [0x68, 0, 0, 0, 0];
        sync_producer[1..].copy_from_slice(&((base + CACHE_RVAS[1]) as u32).to_le_bytes());
        memory::check(base + 0x8e232e, &sync_producer)?;
        let mut async_producer = [0xc7, 0x41, 0x04, 0, 0, 0, 0];
        async_producer[3..].copy_from_slice(&((base + CACHE_RVAS[0]) as u32).to_le_bytes());
        memory::check(base + 0x8e1ab7, &async_producer)?;
        for reference in CACHE_REFERENCES {
            let bytes = reference.bytes(base + CACHE_RVAS[reference.cache]);
            memory::check(base + reference.rva, &bytes[..reference.original.len()])?;
        }
    }
    Ok(())
}

pub(super) struct Caches {
    base: usize,
    buffers: [Buffer; CACHE_RVAS.len()],
    addresses: [usize; CACHE_RVAS.len()],
    dirty: [bool; CACHE_RVAS.len()],
    pending: [Option<Use>; CACHE_RVAS.len()],
}

impl Caches {
    pub(super) fn new(base: usize) -> Self {
        Self {
            base,
            buffers: std::array::from_fn(|_| Buffer::default()),
            addresses: CACHE_RVAS.map(|rva| base + rva),
            dirty: [false; CACHE_RVAS.len()],
            pending: [None; CACHE_RVAS.len()],
        }
    }

    // Each native cache already serializes its read and subsequent construction.
    // Publish at the actual read boundary, with no consumer of this cache running;
    // publishing at enqueue time would redirect older requests prematurely.
    unsafe fn prepare(&mut self, cache: usize, size: usize) -> Result<*mut u8, String> {
        // A failed attempt must not let an older completion acknowledge this load.
        self.pending[cache] = None;
        let pointer = self.buffers[cache].prepare(size)?;
        if pointer as usize != self.addresses[cache] {
            self.dirty[cache] = true;
            if let Err(error) = unsafe { self.retarget(cache, pointer as usize) } {
                let rollback = unsafe { self.retarget(cache, self.addresses[cache]) };
                return Err(match rollback {
                    Ok(()) => {
                        self.dirty[cache] = self.addresses[cache] != self.base + CACHE_RVAS[cache];
                        error
                    }
                    Err(rollback) => {
                        format!("{error}; equipment cache rollback failed: {rollback}")
                    }
                });
            }
            self.addresses[cache] = pointer as usize;
        }
        self.pending[cache] = self.buffers[cache].begin_use(pointer as usize);
        self.buffers[cache].reclaim(self.addresses[cache]);
        Ok(pointer)
    }

    fn snapshot(&self, asynchronous: bool) -> [Option<Use>; CACHE_RVAS.len()] {
        std::array::from_fn(|index| {
            if (index == 0 || (2..8).contains(&index)) == asynchronous {
                self.pending[index]
            } else {
                None
            }
        })
    }

    fn finish(&mut self, snapshot: [Option<Use>; CACHE_RVAS.len()]) {
        for (index, usage) in snapshot.into_iter().enumerate() {
            if let Some(usage) = usage
                && self.pending[index] == Some(usage)
            {
                self.pending[index] = None;
                self.buffers[index].complete(usage);
                self.buffers[index].reclaim(self.addresses[index]);
            }
        }
    }

    fn cancel_async(&mut self) {
        self.pending[0] = None;
        self.pending[2..8].fill(None);
    }

    unsafe fn retarget(&self, cache: usize, address: usize) -> Result<(), String> {
        for reference in CACHE_REFERENCES
            .iter()
            .filter(|reference| reference.cache == cache)
        {
            let bytes = reference.bytes(address);
            unsafe { memory::write(self.base, reference.rva, &bytes[..reference.original.len()]) }?;
        }
        Ok(())
    }

    fn invalidate(&mut self, cache: usize) {
        self.pending[cache] = None;
        self.buffers[cache].write_prefix(&INVALID_RESOURCE);
        unsafe {
            ptr::copy_nonoverlapping(
                INVALID_RESOURCE.as_ptr(),
                (self.base + CACHE_RVAS[cache]) as *mut u8,
                INVALID_RESOURCE.len(),
            );
        }
    }

    pub(super) fn restore(&mut self) -> Result<(), String> {
        for (cache, rva) in CACHE_RVAS.into_iter().enumerate() {
            if self.dirty[cache] {
                unsafe { self.retarget(cache, self.base + rva) }?;
                self.addresses[cache] = self.base + rva;
                self.dirty[cache] = false;
            }
        }
        Ok(())
    }
}

unsafe extern "C" fn finish_sync(player: usize) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(usize) -> i32 =
            unsafe { transmute(BASE.load(Ordering::Acquire) + SYNC_BUILD) };
        return unsafe { original(player) };
    };
    let snapshot = state
        .equipment_caches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .snapshot(false);
    let original: unsafe extern "C" fn(usize) -> i32 =
        unsafe { transmute(state.cache_completion_originals.build) };
    let _completion = Completion::on_return(|| {
        state
            .equipment_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .finish(snapshot);
    });
    unsafe { original(player) }
}

unsafe extern "C" fn reset() -> usize {
    let invocation = SLOT.enter();
    let target = if let Some(state) = invocation.state() {
        // Do not acknowledge a cancelled I/O as completed, including the native
        // reset's direct completion-callback path outside the dispatcher.
        state
            .equipment_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancel_async();
        state.cache_completion_originals.reset
    } else {
        BASE.load(Ordering::Acquire) + RESET
    };
    let original: unsafe extern "C" fn() -> usize = unsafe { transmute(target) };
    unsafe { original() }
}

impl Drop for Caches {
    fn drop(&mut self) {
        // A failed instruction restore must not leave native operands dangling.
        for (cache, dirty) in self.dirty.iter().enumerate() {
            if *dirty {
                self.buffers[cache].retain_for_native();
            }
        }
    }
}

unsafe fn prepare(state: &State, cache: usize, path: &CStr) -> Result<*mut u8, String> {
    let bytes = path.to_bytes_with_nul();
    let relative = bytes
        .strip_prefix(b"dat\\")
        .or_else(|| bytes.strip_prefix(b"dat/"))
        .unwrap_or(bytes);
    let size: unsafe extern "fastcall" fn(*const u8) -> i32 =
        unsafe { transmute(state.address(0xaff0)) };
    let file_size = unsafe { size(relative.as_ptr()) };
    if file_size <= 0 {
        return Err(format!(
            "cannot size equipment resource {}",
            path.to_string_lossy()
        ));
    }
    let required = required_size(file_size as usize, path.to_bytes().len())?;
    unsafe {
        state
            .equipment_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .prepare(cache, required)
    }
}

fn failed(state: &State, cache: usize, error: &str) {
    state
        .equipment_caches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .invalidate(cache);
    eprintln!("equipment source cache: {error}");
}

pub(super) unsafe extern "C" fn load(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let path = registers.eax as *const u8;
    let destination = unsafe { registers.argument(0) } as *mut u8;
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        registers.eax = unsafe {
            abi::read_equipment_file(BASE.load(Ordering::Acquire) + 0x8e27d0, path, destination)
        };
        return;
    };
    let Some(cache) = CACHE_RVAS
        .iter()
        .position(|&rva| state.address(rva) == destination as usize)
    else {
        registers.eax =
            unsafe { abi::read_equipment_file(state.address(0x8e27d0), path, destination) };
        return;
    };
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let path = CStr::from_ptr(path.cast());
        let buffer = prepare(state, cache, path)?;
        // Let the native reader fill the prepared equipment cache buffer.
        let loaded =
            abi::read_equipment_file(state.address(0x8e27d0), path.as_ptr().cast(), buffer);
        if loaded == 0 {
            return Err(format!(
                "failed to read equipment resource {}",
                path.to_string_lossy()
            ));
        }
        Ok(loaded)
    }));
    registers.eax = match result {
        Ok(Ok(loaded)) => loaded,
        Ok(Err(error)) => {
            failed(state, cache, &error);
            0
        }
        Err(_) => {
            failed(state, cache, "equipment file loading panicked");
            0
        }
    };
}

unsafe fn current_request(state: &State) -> Option<usize> {
    unsafe {
        let alternate = memory::get::<u32>(state.address(0x0e866d20));
        if memory::get::<u32>(state.address(0x0e866ce0)) == 0 && alternate == 0 {
            return None;
        }
        let index = memory::get::<u32>(state.address(0x0e866ce4));
        if index >= 1024 {
            return None;
        }
        let request = state.address(0x0e866d40) + index as usize * 76;
        let kind = memory::get::<u32>(request);
        Some(
            if alternate == 2 || (alternate == 1 && !matches!(kind, 3 | 6)) {
                state.address(0x0e879d44)
            } else {
                request
            },
        )
    }
}

unsafe extern "C" fn skip_file(_a: u32, _b: u32, _c: u32, _d: u32) {}

pub(super) unsafe extern "C" fn dispatch() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Acquire) + DISPATCH) };
        return unsafe { original() };
    };
    let request = unsafe { current_request(state) };
    // Type 6 is shared by file workers and construction workers. Only the
    // completed full equipment constructor is a reclamation boundary.
    let mut completion = request
        .filter(|&request| unsafe {
            memory::get::<u32>(request) == 6
                && memory::get::<usize>(request + 8) == state.address(ASYNC_BUILD)
                && memory::get::<usize>(request + 16) == state.address(ASYNC_FINISH)
        })
        .map(|request| {
            let snapshot = state
                .equipment_caches
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .snapshot(true);
            (
                request,
                Completion::pending(move || {
                    state
                        .equipment_caches
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .finish(snapshot);
                }),
            )
        });
    if let Some(request) = request
        && matches!(unsafe { memory::get::<u32>(request) }, 2 | 7)
        && let Some(cache) = CACHE_RVAS
            .iter()
            .position(|&rva| state.address(rva) == unsafe { memory::get::<usize>(request + 4) })
    {
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            let bytes = std::slice::from_raw_parts((request + 12) as *const u8, 64);
            let path = CStr::from_bytes_until_nul(bytes)
                .map_err(|_| "unterminated equipment cache path".to_owned())?;
            let buffer = prepare(state, cache, path)?;
            memory::put(request + 4, buffer as usize);
            Ok::<_, String>(())
        }));
        let error = match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(_) => Some("queued equipment loading panicked".into()),
        };
        if let Some(error) = error {
            failed(state, cache, &error);
            // A normal callback job advances this queue without starting unsafe I/O.
            // The following model callback sees a rejected ECD resource and skips it.
            unsafe {
                memory::put(request, 1u32);
                memory::put(request + 4, skip_file as *const () as usize);
            }
        }
    }
    let original: unsafe extern "C" fn() -> i32 =
        unsafe { transmute(state.cache_dispatch_original) };
    let result = unsafe { original() };
    if result != 0
        && let Some((request, completion)) = completion.as_mut()
        && unsafe { memory::get::<u32>(*request) } == 0
    {
        // The dispatcher observed the worker event, ran its completion callback
        // and advanced the queue. Waiting/file-read-only ticks never reach here.
        completion.confirm();
    }
    result
}

#[cfg(test)]
pub(super) unsafe fn verify_reclamation(base: usize) {
    let invocation = SLOT.enter();
    let state = invocation.state().unwrap();
    {
        let mut caches = state.equipment_caches.lock().unwrap();
        unsafe { caches.prepare(1, 0x20000) }.unwrap();
    }
    // The real synchronous builder's inactive-player branch safely returns
    // without rendering; its installed completion hook still closes the batch.
    let player = vec![0u8; 4176];
    let build: unsafe extern "C" fn(usize) -> i32 = unsafe { transmute(base + SYNC_BUILD) };
    assert_eq!(unsafe { build(player.as_ptr() as usize) }, 0);
    let mut caches = state.equipment_caches.lock().unwrap();
    assert!(caches.pending[1].is_none());
    unsafe { caches.prepare(1, 0x40000) }.unwrap();
    assert_eq!(
        caches.buffers[1].allocation_count(),
        1,
        "completed synchronous old block reclaimed"
    );

    unsafe { caches.prepare(0, 0x20000) }.unwrap();
    let snapshot = caches.snapshot(true);
    caches.finish(snapshot);
    assert!(
        caches.pending[1].is_some(),
        "async completion cannot finish the sync batch"
    );
    let cancelled = unsafe { caches.prepare(0, 0x40000) }.unwrap();
    assert_eq!(caches.buffers[0].allocation_count(), 1);
    unsafe { cancelled.write(0x5a) };
    let stale = caches.snapshot(true);
    drop(caches);
    // Run the actual hooked queue reset with an idle queue. Preserve the DLL's
    // pre-entrypoint queue state, and avoid touching any real OS handles.
    let queue_start = base + 0x0e866ce0;
    let queue_bytes = 0x0e879d90 - 0x0e866ce0;
    let saved =
        unsafe { std::slice::from_raw_parts(queue_start as *const u8, queue_bytes) }.to_vec();
    unsafe {
        memory::put(base + 0x0e879d40, base + 0x0e866d40);
        memory::put(base + 0x0e866d40, 0u32);
        memory::put(base + 0x0e866cec, 0usize);
        let reset: unsafe extern "C" fn() -> usize = transmute(base + RESET);
        reset();
        ptr::copy_nonoverlapping(saved.as_ptr(), queue_start as *mut u8, queue_bytes);
    }
    let mut caches = state.equipment_caches.lock().unwrap();
    assert!(
        caches.pending[0].is_none(),
        "native cancellation hook abandons the pending use"
    );
    caches.finish(stale);
    unsafe { caches.prepare(0, 0x80000) }.unwrap();
    assert_eq!(
        caches.buffers[0].allocation_count(),
        2,
        "cancelled block retained"
    );
    let completed = caches.snapshot(true);
    caches.finish(completed);
    unsafe { caches.prepare(0, 0x100000) }.unwrap();
    assert_eq!(
        caches.buffers[0].allocation_count(),
        2,
        "only the cancelled and current blocks remain"
    );
    assert_eq!(unsafe { cancelled.read() }, 0x5a);
}
