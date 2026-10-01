//! Shared allocation policy for native equipment and stage workspaces.

fn capacity(required: usize) -> Result<usize, String> {
    crate::mesh::byte_size(required, 1)?;
    Ok(required
        .checked_next_power_of_two()
        .filter(|&size| size <= i32::MAX as usize)
        .unwrap_or(required))
}

#[derive(Default)]
pub(crate) struct Buffer {
    // Native jobs can retain earlier addresses after I/O has finished.
    allocations: Vec<Allocation>,
    next_use: u64,
}

struct Allocation {
    bytes: Vec<u8>,
    usage: Option<Use>,
    completed: bool,
    pinned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Use {
    pub address: usize,
    serial: u64,
}

/// Ends a native use at a verified completion boundary. This guard deliberately
/// does not acknowledge unwinding as completion: foreign raw pointers may live on.
#[must_use = "keep the completion guard alive across the native operation"]
pub(crate) struct Completion<F: FnOnce()> {
    finish: Option<F>,
    confirmed: bool,
}

impl<F: FnOnce()> Completion<F> {
    /// For synchronous native operations whose return ends the captured use.
    pub(crate) fn on_return(finish: F) -> Self {
        Self {
            finish: Some(finish),
            confirmed: true,
        }
    }

    /// A queue poll can return while its worker is still using the allocation.
    pub(crate) fn pending(finish: F) -> Self {
        Self {
            finish: Some(finish),
            confirmed: false,
        }
    }

    pub(crate) fn confirm(&mut self) {
        self.confirmed = true;
    }
}

impl<F: FnOnce()> Drop for Completion<F> {
    fn drop(&mut self) {
        if self.confirmed
            && !std::thread::panicking()
            && let Some(finish) = self.finish.take()
        {
            finish();
        }
    }
}

impl Buffer {
    /// Reuse sufficient storage; grow geometrically without freeing old addresses.
    pub(crate) fn prepare(&mut self, required: usize) -> Result<*mut u8, String> {
        let size = capacity(required)?;
        if self
            .allocations
            .last()
            .is_none_or(|allocation| allocation.bytes.len() < size)
        {
            self.allocations.try_reserve(1).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(size).map_err(|e| e.to_string())?;
            bytes.resize(size, 0);
            self.allocations.push(Allocation {
                bytes,
                usage: None,
                completed: false,
                pinned: false,
            });
        }
        Ok(self.allocations.last_mut().unwrap().bytes.as_mut_ptr())
    }

    /// Call after publication, before handing the address to a native consumer.
    pub(crate) fn begin_use(&mut self, address: usize) -> Option<Use> {
        let allocation = self
            .allocations
            .iter_mut()
            .find(|a| a.bytes.as_ptr() as usize == address)?;
        // Reuse following an unconfirmed/abandoned operation cannot prove that
        // its older native borrowers have gone away. Retain this allocation.
        allocation.pinned |= allocation.usage.is_some() && !allocation.completed;
        allocation.completed = false;
        let Some(serial) = self.next_use.checked_add(1) else {
            allocation.pinned = true;
            return None;
        };
        self.next_use = serial;
        let usage = Use { address, serial };
        allocation.usage = Some(usage);
        Some(usage)
    }

    /// Only acknowledge a use after every native consumer of it has returned.
    pub(crate) fn complete(&mut self, usage: Use) {
        if let Some(allocation) = self.allocations.iter_mut().find(|a| a.usage == Some(usage)) {
            allocation.completed = true;
        }
    }

    /// Publication must have succeeded. Keep its address and the reusable latest
    /// allocation; only confirmed, unpinned older allocations can be freed.
    pub(crate) fn reclaim(&mut self, published: usize) {
        let latest = self.allocations.last().map(|a| a.bytes.as_ptr() as usize);
        self.allocations.retain(|a| {
            let address = a.bytes.as_ptr() as usize;
            Some(address) == latest || address == published || !a.completed || a.pinned
        });
    }

    #[cfg(test)]
    pub(crate) fn allocation_count(&self) -> usize {
        self.allocations.len()
    }

    /// Replace the prefix in every retained allocation; callers supply its format.
    /// The prefix must fit the smallest allocation prepared for this buffer.
    pub(crate) fn write_prefix(&mut self, prefix: &[u8]) {
        for allocation in &mut self.allocations {
            allocation.bytes[..prefix.len()].copy_from_slice(prefix);
        }
    }

    #[cfg(all(windows, target_arch = "x86"))]
    pub(crate) fn retain_for_native(&mut self) {
        std::mem::forget(std::mem::take(&mut self.allocations));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_drop_releases_finished_uses_but_not_waiting_or_unwinding_uses() {
        use std::{
            panic::{AssertUnwindSafe, catch_unwind},
            sync::Mutex,
        };

        let buffer = Mutex::new(Buffer::default());
        let old = buffer.lock().unwrap().prepare(32).unwrap() as usize;
        let usage = buffer.lock().unwrap().begin_use(old).unwrap();
        let current = buffer.lock().unwrap().prepare(64).unwrap() as usize;
        let finish = || {
            let mut buffer = buffer.lock().unwrap();
            buffer.complete(usage);
            buffer.reclaim(current);
        };
        {
            let _waiting = Completion::pending(finish);
        }
        assert_eq!(buffer.lock().unwrap().allocation_count(), 2);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _interrupted = Completion::on_return(finish);
                panic!("native completion was not reached");
            }))
            .is_err()
        );
        assert_eq!(buffer.lock().unwrap().allocation_count(), 2);
        {
            let mut completed = Completion::pending(finish);
            completed.confirm();
            assert_eq!(buffer.lock().unwrap().allocation_count(), 2);
        }
        assert_eq!(buffer.lock().unwrap().allocation_count(), 1);

        let usage = buffer.lock().unwrap().begin_use(current).unwrap();
        let newer = buffer.lock().unwrap().prepare(128).unwrap() as usize;
        let normal_return = || {
            let _completed = Completion::on_return(|| {
                let mut buffer = buffer.lock().unwrap();
                buffer.complete(usage);
                buffer.reclaim(newer);
            });
            7
        };
        assert_eq!(normal_return(), 7);
        assert_eq!(buffer.lock().unwrap().allocation_count(), 1);
    }

    #[test]
    fn requests_within_the_allocated_capacity_reuse_the_same_address() {
        let mut buffer = Buffer::default();
        let old = buffer.prepare(17).unwrap();
        unsafe { old.write(0x5a) };
        assert_eq!(buffer.prepare(31).unwrap(), old);
        assert_eq!(buffer.prepare(32).unwrap(), old);
        let new = buffer.prepare(33).unwrap();
        assert_ne!(new, old);
        assert_eq!(unsafe { old.read() }, 0x5a);
        assert_eq!(buffer.prepare(17).unwrap(), new);
        assert!(buffer.prepare(usize::MAX).is_err());
        assert_eq!(buffer.prepare(64).unwrap(), new);
        assert_eq!(buffer.allocations.len(), 2);
    }

    #[test]
    fn growth_respects_the_native_allocation_limit_without_rounding_past_it() {
        assert_eq!(capacity(33 * 1024 * 1024).unwrap(), 64 * 1024 * 1024);
        assert_eq!(capacity(i32::MAX as usize).unwrap(), i32::MAX as usize);
        assert!(capacity(i32::MAX as usize + 1).is_err());
        assert!(capacity(usize::MAX).is_err());
    }

    #[test]
    fn reclamation_requires_completion_and_successful_pointer_publication() {
        let mut buffer = Buffer::default();
        let old = buffer.prepare(32).unwrap() as usize;
        let usage = buffer.begin_use(old).unwrap();
        let new = buffer.prepare(64).unwrap() as usize;
        buffer.reclaim(new);
        assert_eq!(
            buffer.allocation_count(),
            2,
            "in-flight old data stays alive"
        );
        buffer.complete(usage);
        buffer.reclaim(old);
        assert_eq!(
            buffer.allocation_count(),
            2,
            "rollback still publishes old data"
        );
        let current = buffer.begin_use(new).unwrap();
        buffer.reclaim(new);
        assert_eq!(buffer.allocation_count(), 1);
        buffer.complete(current);
        buffer.reclaim(new);
        assert_eq!(buffer.prepare(64).unwrap() as usize, new);
    }

    #[test]
    fn late_completion_cannot_release_reused_or_abandoned_storage() {
        let mut buffer = Buffer::default();
        let old = buffer.prepare(32).unwrap() as usize;
        let first = buffer.begin_use(old).unwrap();
        let second = buffer.begin_use(old).unwrap();
        buffer.complete(first);
        let new = buffer.prepare(64).unwrap() as usize;
        buffer.reclaim(new);
        assert_eq!(buffer.allocation_count(), 2);
        buffer.complete(second);
        buffer.reclaim(new);
        assert_eq!(
            buffer.allocation_count(),
            2,
            "unconfirmed earlier use stays pinned"
        );
    }
}
