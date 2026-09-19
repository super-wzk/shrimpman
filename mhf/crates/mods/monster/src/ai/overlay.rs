//! Game-side loading of `dat/monster-ai/*.mhai` onto the live descriptor.
//!
//! The verified client selects a species block in `0x10860360` (record
//! initialization), keeps it in the actor at `+9F0`, and derives the state
//! script `+9F4`/`+A5C` from that block. Every later descriptor read goes
//! through the same field — `0x10860340` and `0x108697C6` for the state table,
//! `0x108604C0`/`0x10860500` for the event cells, `0x10862010` for the area
//! route-move table — so replacing the block pointer after the initializer
//! returns routes the whole AI through the private overlay. No other writer of
//! `+9F0` exists in the client, and no reader caches a table pointer across
//! calls.
//!
//! The files are read from the session's own data root (spec §8.3); the
//! descriptor they inherit is the one the actor just selected, which is why a
//! binding is built per `(map, species)` on first use and kept for the session.

use crate::ai::bind::{Arena, NativeMemory};
use crate::ai::{Base, Error as AiError};
use crate::native::{put, read, verify_image};
use mhf_hooks::{HookGuard, HookSlot, ModuleReference};
use std::{
    collections::HashMap,
    ffi::c_void,
    fs,
    io::ErrorKind,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};
use windows::Win32::Foundation::HMODULE;

/// `sub_10860360`: record initialization and species selection.
const INIT: usize = 0x0086_0360;
/// The verified client's first eight bytes of `INIT`. The hook rewrites the
/// entry, so a different build has to fail here instead of being reinterpreted.
const INIT_SIGNATURE: [u8; 8] = [0x8a, 0x56, 0x03, 0xa1, 0x3c, 0xff, 0x7f, 0x1e];
/// `dword_1E7FFF3C`, the session pointer whose `+0x34` holds the MapID
/// (`0x10AA5D19` writes it).
const SESSION: usize = 0x00e7_ff3c;
const MAP_ID: usize = 0x34;
/// Above this species the client leaves the shared row table and selects a
/// per-species block, so a row overlay does not apply (spec §8.2).
const SPECIES_LIMIT: u8 = 0x83;
/// Actor offset of the selected species (`[actor+3]`).
const SPECIES: usize = 0x03;
/// Actor fields the initializer derives from the block.
const DESCRIPTOR: usize = 0x09f0;
const SCRIPT: usize = 0x09f4;
const STATE: usize = 0x0a10;
const CURSOR: usize = 0x0a5c;
/// Where the install layer expects `(map, species)` files (spec §8.3).
const ROOT: &str = "dat/monster-ai";

static SLOT: HookSlot<State> = HookSlot::new();
/// Trampoline address for the naked detour, which cannot read hook state before
/// it has entered an invocation.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// One cell's overlay, or `None` once the layer decided the cell has none.
type Bindings = Mutex<HashMap<(u32, u8), Option<Arc<Bound>>>>;

/// Owns the installed hook. Removal needs every native caller stopped.
pub(crate) struct Hook {
    hook: HookGuard<State>,
}

impl Hook {
    /// Install the overlay hook for the verified ZZ HD client.
    ///
    /// # Safety
    ///
    /// `module` must be the live, fully mapped supported i686 game image and
    /// must stay loaded until [`Self::prepare_release`]. Install before the game
    /// entrypoint runs; stop every native caller before uninstalling.
    pub(crate) unsafe fn install(module: HMODULE) -> Result<Self, String> {
        let base = module.0 as usize;
        unsafe { validate(base) }?;
        let retained = unsafe { ModuleReference::acquire(module) }?;
        let mut hooks = SLOT.prepare()?;
        let original = unsafe {
            hooks.create(
                "monster AI record initialization",
                (base + INIT) as _,
                init_detour as *mut c_void,
            )
        }?;
        ORIGINAL.store(original as usize, Ordering::Release);
        let hook = unsafe {
            hooks.install(State {
                module: retained,
                base,
                bindings: Mutex::new(HashMap::new()),
            })
        }?;
        Ok(Self { hook })
    }

    pub(crate) fn uninstall(&mut self) -> Result<(), String> {
        self.hook.uninstall()
    }

    /// Release the module reference itself, once removal has drained callbacks.
    ///
    /// # Safety
    ///
    /// Call only after [`Self::uninstall`] succeeded and all game callers
    /// stopped.
    pub(crate) unsafe fn prepare_release(&mut self) -> Result<(), String> {
        let state = self
            .hook
            .retired_state_mut()
            .ok_or("monster AI hooks have not finished detaching")?;
        unsafe { state.module.release() }
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        if let Err(error) = self.uninstall() {
            eprintln!("monster AI hook cleanup failed: {error}");
        }
    }
}

/// Retained callback state: the module it hooks and one binding per cell.
struct State {
    module: ModuleReference,
    base: usize,
    /// `None` records "this cell has no file", so the two misses are not
    /// re-read for every spawn. A file that exists but does not bind is *not*
    /// cached: it is a hard error (spec §8.3), so the next spawn retries it and
    /// a fixed file takes effect without restarting the session.
    bindings: Bindings,
}

impl State {
    /// Overlay one actor's AI, if its cell has a file.
    unsafe fn apply(&self, actor: usize) -> Result<(), String> {
        let species = unsafe { read::<u8>(actor + SPECIES) };
        if species > SPECIES_LIMIT {
            return Ok(());
        }
        let session = unsafe { read::<u32>(self.base + SESSION) } as usize;
        if session == 0 {
            return Err(format!("no session is loaded at {SESSION:#010x}"));
        }
        let map = unsafe { read::<u32>(session + MAP_ID) };
        let descriptor = unsafe { read::<u32>(actor + DESCRIPTOR) };
        let Some(bound) = self.binding(map, species, descriptor)? else {
            return Ok(());
        };
        unsafe {
            put(actor + DESCRIPTOR, bound.descriptor);
            // The initializer loaded the cursor from the native block; repeat it
            // against the overlay so `+9F4`/`+A5C` never point at a native state
            // the document replaced.
            let index = read::<u8>(actor + STATE) as usize;
            let script = read::<u32>(bound.state_table as usize + index * 4);
            put(actor + SCRIPT, script);
            put(actor + CURSOR, script);
        }
        Ok(())
    }

    fn binding(
        &self,
        map: u32,
        species: u8,
        descriptor: u32,
    ) -> Result<Option<Arc<Bound>>, String> {
        if let Some(cached) = self
            .bindings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(map, species))
        {
            return Ok(cached.clone());
        }
        // A file that exists is an override, so its errors are propagated
        // instead of being read as "no file". The actor keeps its native AI
        // because there is no third state to publish, but the layer reports the
        // failure on every spawn until the file binds.
        let bound = self.load(map, species, descriptor)?.map(Arc::new);
        self.bindings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((map, species), bound.clone());
        Ok(bound)
    }

    /// Read, parse, compile and bind one cell. `Ok(None)` means neither
    /// candidate path exists, so the native block stays installed; an existing
    /// file that cannot be read or bound is `Err`.
    fn load(&self, map: u32, species: u8, descriptor: u32) -> Result<Option<Bound>, String> {
        for path in [
            format!("{ROOT}/{map}/{species}.mhai"),
            format!("{ROOT}/{species}.mhai"),
        ] {
            let source = match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("{path}: {error}")),
            };
            return Ok(Some(self.compile(&path, &source, species, descriptor)?));
        }
        Ok(None)
    }

    fn compile(
        &self,
        path: &str,
        source: &str,
        species: u8,
        descriptor: u32,
    ) -> Result<Bound, String> {
        let document = crate::ai::dsl::parse(source).map_err(|error| format!("{path}: {error}"))?;
        if document.species != species {
            return Err(format!(
                "{path}: declares species {} but the file selects species {species}",
                document.species
            ));
        }
        let compiled = document
            .compile()
            .map_err(|error| format!("{path}: {error}"))?;
        if compiled.program.base != Base::Native {
            return Err(format!(
                "{path}: the game binding overlays a live descriptor, so this file needs `base native;`; without it the file describes a self-contained graph"
            ));
        }
        for warning in &compiled.warnings {
            eprintln!("monster AI {path}: {warning}");
        }
        let mut blocks = Blocks::default();
        let overlay =
            crate::ai::bind::materialize(&compiled.program, descriptor, &Live, &mut blocks)
                .map_err(|error| format!("{path}: {error}"))?;
        Ok(Bound {
            descriptor: overlay.descriptor,
            state_table: overlay.state_table,
            _blocks: blocks,
        })
    }
}

/// One installed overlay. `_blocks` owns every word the game can now read, so it
/// has to outlive the actors that were pointed at it.
struct Bound {
    descriptor: u32,
    state_table: u32,
    _blocks: Blocks,
}

/// The game's own address space, as the binding's read side.
struct Live;

impl NativeMemory for Live {
    fn read(&self, address: u32, words: usize) -> Result<Vec<u32>, AiError> {
        if address == 0 || !address.is_multiple_of(4) {
            return Err(AiError::new(format!(
                "live AI address {address:#010x} is not a word address"
            )));
        }
        // The address comes from the actor's own descriptor, which the client
        // just selected; there is no portable way to validate it further.
        let words = unsafe { std::slice::from_raw_parts(address as *const u32, words) };
        Ok(words.to_vec())
    }
}

/// Private storage for one binding. Boxed slices never move, so the addresses
/// handed to the game stay valid until this value is dropped.
#[derive(Default)]
struct Blocks {
    blocks: Vec<Box<[u32]>>,
}

impl Arena for Blocks {
    fn allocate(&mut self, words: usize) -> Result<u32, AiError> {
        let block = vec![0u32; words.max(1)].into_boxed_slice();
        let address = u32::try_from(block.as_ptr() as usize)
            .map_err(|_| AiError::new("a private AI block is outside the i686 address space"))?;
        self.blocks.push(block);
        Ok(address)
    }

    fn write(&mut self, address: u32, words: &[u32]) -> Result<(), AiError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.as_ptr() as usize == address as usize)
            .ok_or_else(|| AiError::new("write to an AI block that was never allocated"))?;
        block.copy_from_slice(words);
        Ok(())
    }
}

/// Saved register frame of the hooked function. `pushfd` runs before `pushad`,
/// so the flags sit above the register block the pointer addresses.
#[repr(C)]
struct Registers {
    /// `pushad` saves EDI at the lowest address; the actor follows it.
    _edi: u32,
    esi: u32,
}

/// `esi` is the actor record and the function takes no stack arguments, so the
/// detour runs the original first and publishes the overlay afterwards.
#[unsafe(naked)]
unsafe extern "C" fn init_detour() {
    core::arch::naked_asm!(
        "call dword ptr [{original}]",
        "pushfd",
        "pushad",
        "push esp",
        "call {dispatch}",
        "add esp, 4",
        "popad",
        "popfd",
        "ret",
        original = sym ORIGINAL,
        dispatch = sym dispatch,
    );
}

unsafe extern "C" fn dispatch(registers: *const Registers) {
    let registers = unsafe { &*registers };
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        return;
    };
    let actor = registers.esi as usize;
    match catch_unwind(AssertUnwindSafe(|| unsafe { state.apply(actor) })) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("monster AI binding failed: {error}"),
        Err(_) => eprintln!("monster AI binding panicked"),
    }
}

unsafe fn validate(base: usize) -> Result<(), String> {
    unsafe { verify_image(base) }?;
    let bytes =
        unsafe { std::slice::from_raw_parts((base + INIT) as *const u8, INIT_SIGNATURE.len()) };
    if bytes != INIT_SIGNATURE.as_slice() {
        return Err("unsupported or already modified monster AI initializer".to_owned());
    }
    Ok(())
}
