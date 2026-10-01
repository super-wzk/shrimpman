//! Protect actual weapon texture owners from native bank overlap and recycling.

use super::{BASE, SLOT, State, abi, memory};
use crate::weapon_textures::{
    CreationResult, HANDLE_LIMIT, OwnerInfo, Ownership, ReleaseDecision, Texture, Token,
};
use mhf_hooks::HookSet;
use std::{
    cell::Cell,
    ffi::c_void,
    mem::transmute,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

const CONSTRUCT: usize = 0x008f88e0;
const CREATE: usize = 0x00011af0;
const RELEASE: usize = 0x00011960;
const RESERVE: usize = 0x000113f0;
const FREE: usize = 0x008f8eb0;
const BULK: usize = 0x008f9080;
const RENDERER: usize = 0x0000ac00;
const TEXTURES: usize = 0x01aa7d80;
const TEXTURE_STRIDE: usize = 216;
const PLAYERS: usize = 0x0dc6b750;
const PLAYER_STRIDE: usize = 4176;
static GUARD_LOGS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static CONSTRUCTING: Cell<Option<Token>> = const { Cell::new(None) };
}

struct Scope(Option<Token>);

impl Scope {
    fn enter(token: Option<Token>) -> Self {
        Self(CONSTRUCTING.with(|current| current.replace(token)))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CONSTRUCTING.with(|current| current.set(self.0));
    }
}

pub(super) struct Originals {
    construct: usize,
    create: usize,
    release: usize,
    reserve: usize,
    free: usize,
    bulk: usize,
    renderer: usize,
}

pub(super) unsafe fn validate(base: usize) -> Result<(), String> {
    let mut renderer = [0xa1, 0, 0, 0, 0, 0x53, 0x56, 0x57];
    renderer[1..5].copy_from_slice(&((base + 0x01a97024) as u32).to_le_bytes());
    let mut reserve = [0x56, 0x68, 0, 0, 0, 0];
    reserve[2..].copy_from_slice(&((base + 0x0e73ac68) as u32).to_le_bytes());
    unsafe {
        memory::check(base + CONSTRUCT, &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8])?;
        memory::check(base + CREATE, &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8])?;
        memory::check(base + RELEASE, &[0x55, 0x8b, 0xec, 0x56, 0x8b, 0x75, 0x08])?;
        memory::check(base + FREE, &[0x55, 0x8b, 0xec, 0x8b, 0x45, 0x08, 0x56])?;
        memory::check(base + BULK, &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08])?;
        memory::check(base + RENDERER, &renderer)?;
        memory::check(base + RESERVE, &reserve)?;
    }
    Ok(())
}

pub(super) unsafe fn create(hooks: &mut HookSet<State>, base: usize) -> Result<Originals, String> {
    unsafe {
        Ok(Originals {
            construct: hooks.create(
                "weapon texture owners",
                (base + CONSTRUCT) as _,
                abi::weapon_texture_constructor_detour as *mut c_void,
            )? as usize,
            create: hooks.create(
                "weapon pending textures",
                (base + CREATE) as _,
                create_texture as *mut c_void,
            )? as usize,
            release: hooks.create(
                "weapon texture release guard",
                (base + RELEASE) as _,
                abi::weapon_texture_release_detour as *mut c_void,
            )? as usize,
            reserve: hooks.create(
                "texture handle generations",
                (base + RESERVE) as _,
                reserve_texture as *mut c_void,
            )? as usize,
            free: hooks.create(
                "weapon resource cleanup",
                (base + FREE) as _,
                free_resource as *mut c_void,
            )? as usize,
            bulk: hooks.create(
                "weapon scene cleanup",
                (base + BULK) as _,
                free_all as *mut c_void,
            )? as usize,
            renderer: hooks.create(
                "weapon renderer cleanup",
                (base + RENDERER) as _,
                free_renderer as *mut c_void,
            )? as usize,
        })
    }
}

fn ownership(state: &State) -> MutexGuard<'_, Ownership> {
    state
        .weapon_texture_ownership
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

unsafe fn texture_pointer(state: &State, handle: u32) -> usize {
    if handle == 0 || handle as usize >= HANDLE_LIMIT {
        return 0;
    }
    unsafe { memory::get(state.address(TEXTURES) + TEXTURE_STRIDE * handle as usize) }
}

unsafe fn weapon_owner(state: &State, registers: &abi::Registers) -> Option<OwnerInfo> {
    unsafe {
        if registers.argument(2) != 0 || registers.edx == 0 {
            return None;
        }
        let base = registers.argument(3);
        let player = match base {
            217..317 => base - 217,
            4028..4044 => base - 4028,
            _ => return None,
        };
        let record = state.address(PLAYERS) + PLAYER_STRIDE * player as usize;
        let resource = registers.argument(0) as usize;
        if memory::get::<u16>(record + 12) != player as u16
            || memory::get::<usize>(record + 1664) != resource
            || resource == 0
        {
            return None;
        }
        Some(OwnerInfo {
            resource,
            base,
            count: memory::get::<u32>(registers.edx as usize),
            player: player as u16,
            weapon: memory::get(record + 1026),
            model: memory::get(record + 908),
        })
    }
}

unsafe fn call_construct(target: usize, registers: &abi::Registers) -> usize {
    unsafe {
        abi::build_resource_original(
            target,
            registers.ecx as usize,
            registers.edx as usize,
            registers.argument(0) as usize,
            registers.argument(1) as usize,
            registers.argument(2),
            registers.argument(3),
            registers.argument(4),
            registers.argument(5),
            registers.argument(6),
        )
    }
}

pub(super) unsafe extern "C" fn construct(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        registers.eax =
            unsafe { call_construct(BASE.load(Ordering::Acquire) + CONSTRUCT, registers) } as u32;
        return;
    };
    let tracked = catch_unwind(AssertUnwindSafe(|| unsafe {
        weapon_owner(state, registers).map(|info| ownership(state).begin(info))
    }))
    .unwrap_or_else(|_| {
        eprintln!("weapon texture ownership: resource tracking panicked");
        None
    });
    let (token, retired) = tracked.map_or((None, Vec::new()), |(token, retired)| {
        (Some(token), retired)
    });
    // A nested non-weapon construction must not inherit its caller's owner.
    let _scope = Scope::enter(token);
    {
        let _stage_completion = unsafe { super::stage_cache::construction(state, registers) };
        registers.eax =
            unsafe { call_construct(state.weapon_texture_originals.construct, registers) } as u32;
    }
    if catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(token) = token {
            ownership(state).finish(token);
        }
        release_retired(state, &retired);
    }))
    .is_err()
    {
        eprintln!("weapon texture ownership: construction cleanup panicked");
    }
}

unsafe extern "C" fn reserve_texture() -> u32 {
    let invocation = SLOT.enter();
    let target = invocation
        .state()
        .map_or(BASE.load(Ordering::Acquire) + RESERVE, |state| {
            state.weapon_texture_originals.reserve
        });
    let original: unsafe extern "C" fn() -> u32 = unsafe { transmute(target) };
    let handle = unsafe { original() };
    if let Some(state) = invocation.state()
        && catch_unwind(AssertUnwindSafe(|| ownership(state).reserve(handle))).is_err()
    {
        eprintln!("weapon texture ownership: handle reservation tracking panicked");
    }
    handle
}

unsafe extern "C" fn create_texture(source: usize, size: u32, handle: u32) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(usize, u32, u32) -> i32 =
            unsafe { transmute(BASE.load(Ordering::Acquire) + CREATE) };
        return unsafe { original(source, size, handle) };
    };
    let ticket = catch_unwind(AssertUnwindSafe(|| {
        CONSTRUCTING
            .with(Cell::get)
            .and_then(|token| ownership(state).pending(token, handle))
    }))
    .unwrap_or(None);
    let original: unsafe extern "C" fn(usize, u32, u32) -> i32 =
        unsafe { transmute(state.weapon_texture_originals.create) };
    let result = unsafe { original(source, size, handle) };
    let Some(ticket) = ticket else {
        return result;
    };
    let completion = catch_unwind(AssertUnwindSafe(|| {
        let mut ownership = ownership(state);
        let pointer = unsafe { texture_pointer(state, handle) };
        ownership.created(ticket, pointer)
    }))
    .unwrap_or(CreationResult::Discarded);
    match completion {
        CreationResult::Published => result,
        CreationResult::Retired(texture) => {
            let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
                release_retired(state, &[texture]);
            }));
            // The owner was retired while GPU creation was pending. The loader
            // must not publish that texture after cleanup completed.
            0
        }
        // A stale creator cannot publish another generation's numeric handle.
        CreationResult::Discarded => 0,
    }
}

pub(super) unsafe extern "C" fn release(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let handle = unsafe { registers.argument(0) };
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(u32) -> i32 =
            unsafe { transmute(BASE.load(Ordering::Acquire) + RELEASE) };
        registers.eax = unsafe { original(handle) } as u32;
        return;
    };
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let (pointer, generation, decision) = {
            let mut ownership = ownership(state);
            let pointer = texture_pointer(state, handle);
            let decision = ownership.prepare_release(handle, pointer, None);
            let generation = match decision {
                ReleaseDecision::Protected(_) => ownership.generation(handle).unwrap_or(0),
                _ => 0,
            };
            (pointer, generation, decision)
        };
        let claim = match decision {
            ReleaseDecision::Protected(owner) => {
                if GUARD_LOGS.fetch_add(1, Ordering::Relaxed) < 32 {
                    let caller = memory::get::<u32>(registers.esp as usize + 4);
                    eprintln!(
                        concat!(
                            "weapon texture release deferred: slot={} weapon={} model={} ",
                            "resource={:#x} base={} count={} handle={} generation={} ",
                            "COM={:#x} caller={:#x}"
                        ),
                        owner.player,
                        owner.weapon,
                        owner.model,
                        owner.resource,
                        owner.base,
                        owner.count,
                        handle,
                        generation,
                        pointer,
                        caller
                    );
                }
                // DF700/DF660 only clear a bank entry after callback success (1).
                // Preserve the other live weapon's mapping as well as its COM object.
                return 0;
            }
            ReleaseDecision::Claimed(claim) => claim,
            ReleaseDecision::Ignored => return 0,
        };
        let original: unsafe extern "C" fn(u32) -> i32 =
            transmute(state.weapon_texture_originals.release);
        let result = original(handle) as u32;
        ownership(state).finish_release(claim);
        result
    }));
    registers.eax = result.unwrap_or_else(|_| {
        eprintln!("weapon texture ownership: release tracking panicked; texture retained");
        0
    });
}

struct RetiredRelease {
    state: *const State,
    texture: Texture,
}

unsafe extern "C" fn release_snapshot(argument: *mut c_void) -> i32 {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let request = &*argument.cast::<RetiredRelease>();
        let state = &*request.state;
        let claim = {
            let mut ownership = ownership(state);
            let pointer = texture_pointer(state, request.texture.handle);
            ownership.claim_release(request.texture.handle, pointer, Some(request.texture))
        };
        let Some(claim) = claim else {
            return 0;
        };
        // This callback runs through the game's own synchronous bridge. Check
        // generation and COM identity on that target thread, immediately before
        // native Release; never carry the ownership mutex across the call.
        let original: unsafe extern "C" fn(u32) -> i32 =
            transmute(state.weapon_texture_originals.release);
        let result = original(request.texture.handle);
        ownership(state).finish_release(claim);
        result
    }))
    .unwrap_or(0)
}

unsafe fn release_retired(state: &State, textures: &[Texture]) {
    for &texture in textures {
        let mut request = RetiredRelease { state, texture };
        unsafe {
            abi::schedule(
                state.address(0x0158ffd0),
                release_snapshot,
                (&mut request as *mut RetiredRelease).cast(),
            );
        }
    }
}

unsafe extern "C" fn free_resource(resource_index: u32) {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(u32) =
            unsafe { transmute(BASE.load(Ordering::Acquire) + FREE) };
        unsafe { original(resource_index) };
        return;
    };
    let retired = catch_unwind(AssertUnwindSafe(|| unsafe {
        let resources = memory::get::<usize>(state.address(0x0ed528d0));
        ownership(state).take(resources + 128 * resource_index as usize)
    }))
    .unwrap_or_default();
    let original: unsafe extern "C" fn(u32) =
        unsafe { transmute(state.weapon_texture_originals.free) };
    unsafe { original(resource_index) };
    if catch_unwind(AssertUnwindSafe(|| unsafe {
        release_retired(state, &retired);
    }))
    .is_err()
    {
        eprintln!("weapon texture ownership: resource cleanup panicked");
    }
}

unsafe extern "C" fn free_all() -> usize {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> usize =
            unsafe { transmute(BASE.load(Ordering::Acquire) + BULK) };
        return unsafe { original() };
    };
    let retired =
        catch_unwind(AssertUnwindSafe(|| ownership(state).take_all())).unwrap_or_default();
    let original: unsafe extern "C" fn() -> usize =
        unsafe { transmute(state.weapon_texture_originals.bulk) };
    let result = unsafe { original() };
    if catch_unwind(AssertUnwindSafe(|| unsafe {
        release_retired(state, &retired);
    }))
    .is_err()
    {
        eprintln!("weapon texture ownership: scene cleanup panicked");
    }
    result
}

unsafe extern "C" fn free_renderer() {
    let invocation = SLOT.enter();
    let target = if let Some(state) = invocation.state() {
        // The renderer scans every handle, including orphaned textures. Pending
        // creations remain guarded until their native creator completes.
        let _ = catch_unwind(AssertUnwindSafe(|| ownership(state).take_all()));
        state.weapon_texture_originals.renderer
    } else {
        BASE.load(Ordering::Acquire) + RENDERER
    };
    let original: unsafe extern "C" fn() = unsafe { transmute(target) };
    unsafe { original() };
}
