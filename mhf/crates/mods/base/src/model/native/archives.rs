//! Replace process-wide ABN indices only while the native I/O queue is idle.

use super::{Client, get, put};
use crate::model::archive_index::Index;
use std::{
    ffi::{CStr, c_void},
    fs::File,
    io::ErrorKind,
    mem::{size_of, size_of_val, transmute},
    ptr,
};

const TABLE: usize = 0x1e83_1e00;
const PATHS: usize = 0x115f_4798;
const COUNT: usize = 77;

#[repr(C)]
struct NativeIndex {
    first: i16,
    count: i16,
    entries: *mut [u32; 2],
}
const _: () = assert!(size_of::<NativeIndex>() == 8);

type Allocate = unsafe extern "C" fn(usize) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);

struct OwnedIndex {
    pointer: *mut NativeIndex,
    free: Free,
}

impl OwnedIndex {
    unsafe fn new(index: &Index, allocate: Allocate, free: Free) -> Result<Self, String> {
        let pointer = unsafe { allocate(size_of::<NativeIndex>()) }.cast::<NativeIndex>();
        if pointer.is_null() {
            return Err("无法分配原生资源目录".into());
        }
        unsafe {
            ptr::write(
                pointer,
                NativeIndex {
                    first: index.first,
                    count: index.entries.len() as i16,
                    entries: ptr::null_mut(),
                },
            )
        };
        let result = Self { pointer, free };
        if !index.entries.is_empty() {
            let entries =
                unsafe { allocate(size_of_val(index.entries.as_slice())) }.cast::<[u32; 2]>();
            if entries.is_null() {
                return Err("无法分配原生资源索引".into());
            }
            unsafe {
                ptr::copy_nonoverlapping(index.entries.as_ptr(), entries, index.entries.len());
                (*pointer).entries = entries;
            }
        }
        Ok(result)
    }

    fn into_raw(mut self) -> *mut NativeIndex {
        std::mem::take(&mut self.pointer)
    }
}

impl Drop for OwnedIndex {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe {
                (self.free)((*self.pointer).entries.cast());
                (self.free)(self.pointer.cast());
            }
        }
    }
}

unsafe fn idle(client: Client) -> bool {
    unsafe { client.read::<u32>(0x1e86_6ce0) == 0 && client.read::<u32>(0x1e86_6d20) == 0 }
}

/// The caller is the game/task thread. All indices are prepared before any
/// pointer is published; a bad file or allocation leaves the old table intact.
/// Allocations use the game's CRT because 1158C880 frees them at shutdown.
pub(super) unsafe fn refresh(client: Client) -> Result<(), String> {
    if !unsafe { idle(client) } {
        return Err("资源读取仍在进行，请稍后重试".into());
    }
    let allocate: Allocate = unsafe { transmute(client.address(0x115a_b67e)) };
    let free: Free = unsafe { transmute(client.address(0x115a_b644)) };
    let mut prepared = Vec::new();
    for slot in 0..COUNT {
        let path = unsafe { client.read::<*const i8>(PATHS + slot * 16) };
        if path.is_null() {
            return Err("原生资源目录路径为空".into());
        }
        let path = unsafe { CStr::from_ptr(path) }
            .to_str()
            .map_err(|e| e.to_string())?;
        // File::open reaches dat-redirect's CreateFileW hook, including fallback
        // to the original when an override was removed.
        let index = match File::open(path) {
            Ok(mut file) => Index::read(&mut file).map_err(|e| format!("{path}：{e}"))?,
            Err(error) if error.kind() == ErrorKind::NotFound => Index::default(),
            Err(error) => return Err(format!("无法读取 {path}：{error}")),
        };
        let at = client.address(TABLE + slot * 4);
        let old: *mut NativeIndex = unsafe { get(at) };
        let matches = unsafe { old.as_ref() }.is_some_and(|old| {
            old.first == index.first
                && old.count as usize == index.entries.len()
                && (index.entries.is_empty()
                    || !old.entries.is_null()
                        && unsafe { std::slice::from_raw_parts(old.entries, index.entries.len()) }
                            == index.entries)
        });
        if !matches {
            prepared.push((at, unsafe { OwnedIndex::new(&index, allocate, free) }?));
        }
    }
    if !unsafe { idle(client) } {
        return Err("资源读取状态发生变化，请稍后重试".into());
    }
    for (at, replacement) in prepared {
        let old: *mut NativeIndex = unsafe { get(at) };
        unsafe { put(at, replacement.into_raw()) };
        drop(OwnedIndex { pointer: old, free });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        alloc::{Layout, alloc, dealloc},
        cell::RefCell,
    };

    #[derive(Default)]
    struct Heap {
        remaining: usize,
        live: Vec<(*mut u8, Layout)>,
    }
    thread_local! { static HEAP: RefCell<Heap> = RefCell::default(); }

    unsafe extern "C" fn allocate(size: usize) -> *mut c_void {
        HEAP.with_borrow_mut(|heap| {
            if heap.remaining == 0 {
                return ptr::null_mut();
            }
            heap.remaining -= 1;
            let layout = Layout::from_size_align(size, 8).unwrap();
            let pointer = unsafe { alloc(layout) };
            if !pointer.is_null() {
                heap.live.push((pointer, layout));
            }
            pointer.cast()
        })
    }

    unsafe extern "C" fn free(pointer: *mut c_void) {
        if !pointer.is_null() {
            HEAP.with_borrow_mut(|heap| {
                let at = heap
                    .live
                    .iter()
                    .position(|(p, _)| *p == pointer.cast())
                    .unwrap();
                let (pointer, layout) = heap.live.swap_remove(at);
                unsafe { dealloc(pointer, layout) };
            });
        }
    }

    #[test]
    fn allocation_failure_releases_all_unpublished_tables_and_transfer_keeps_ownership() {
        let index = Index {
            first: 521,
            entries: vec![[128, 23725], [23853, 12]],
        };
        // Fail either allocation in the second table after preparing the first.
        for limit in [2, 3] {
            HEAP.with_borrow_mut(|heap| heap.remaining = limit);
            let result = (0..2)
                .map(|_| unsafe { OwnedIndex::new(&index, allocate, free) })
                .collect::<Result<Vec<_>, _>>();
            assert!(result.is_err());
            HEAP.with_borrow(|heap| assert!(heap.live.is_empty()));
        }
        HEAP.with_borrow_mut(|heap| heap.remaining = 2);
        let pointer = unsafe { OwnedIndex::new(&index, allocate, free) }
            .unwrap()
            .into_raw();
        HEAP.with_borrow(|heap| assert_eq!(heap.live.len(), 2));
        unsafe {
            assert_eq!((*pointer).first, 521);
            assert_eq!((*pointer).count, 2);
            assert_eq!(
                std::slice::from_raw_parts((*pointer).entries, 2),
                index.entries
            );
        }
        drop(OwnedIndex { pointer, free });
        HEAP.with_borrow(|heap| assert!(heap.live.is_empty()));
    }
}
