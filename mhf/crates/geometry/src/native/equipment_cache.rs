//! Relocate the 128-KiB raw equipment caches at their native I/O boundaries.

use super::{BASE, SLOT, State, abi, memory};
use crate::equipment_cache::{Buffer, CACHE_REFERENCES, CACHE_RVAS, INVALID_RESOURCE, capacity};
use std::{
    ffi::CStr,
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
pub(super) static SYNC_RETURN: AtomicUsize = AtomicUsize::new(0);
pub(super) static SYNC_PART_RETURN: AtomicUsize = AtomicUsize::new(0);

pub(super) unsafe fn validate(base: usize) -> Result<(), String> {
    unsafe {
        memory::check(base + SYNC_LOAD, &[0xe8, 0x93, 0x04, 0x00, 0x00])?;
        memory::check(base + SYNC_PART_LOAD, &[0xe8, 0x00, 0x05, 0x00, 0x00])?;
        memory::check(base + DISPATCH, &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08])?;
        memory::check(
            base + 0xaff0,
            &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x08, 0x01, 0x00, 0x00],
        )?;
        // The unchanged producer addresses identify weapon requests.
        memory::check(
            base + 0x8e232e,
            &[&[0x68][..], &((base + CACHE_RVAS[1]) as u32).to_le_bytes()].concat(),
        )?;
        memory::check(
            base + 0x8e1ab7,
            &[
                &[0xc7, 0x41, 0x04][..],
                &((base + CACHE_RVAS[0]) as u32).to_le_bytes(),
            ]
            .concat(),
        )?;
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
}

impl Caches {
    pub(super) fn new(base: usize) -> Self {
        Self {
            base,
            buffers: std::array::from_fn(|_| Buffer::default()),
            addresses: CACHE_RVAS.map(|rva| base + rva),
            dirty: [false; CACHE_RVAS.len()],
        }
    }

    // Each native cache already serializes its read and subsequent construction.
    // Publish at the actual read boundary, with no consumer of this cache running;
    // publishing at enqueue time would redirect older requests prematurely.
    unsafe fn prepare(&mut self, cache: usize, size: usize) -> Result<*mut u8, String> {
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
        Ok(pointer)
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
        self.buffers[cache].invalidate();
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
    let capacity = capacity(file_size as usize, path.to_bytes().len())?;
    unsafe {
        state
            .equipment_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .prepare(cache, capacity)
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
        // Call through the existing localization hook as well as the native reader.
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
    if let Some(request) = unsafe { current_request(state) }
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
    unsafe { original() }
}
