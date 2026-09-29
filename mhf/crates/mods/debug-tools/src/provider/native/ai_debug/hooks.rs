//! Verified ZZ HD interpreter safe points. No native stack is retained while paused.
//!
//! At all three points ESI is the actor, and the interpreter's saved registers
//! and security cookie already exist. A paused dispatch exits through the real
//! epilogue. Resumption creates a fresh native frame, restores scratch registers
//! and the dispatch count, and skips restart/event selection for that same turn.
use super::super::{SLOT, State};
use mhf_hooks::HookSet;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(in crate::provider::native) const SIGNATURES: &[(usize, &[u8])] = &[
    (0x008696ee, &[0x89, 0x5c, 0x24, 0x0c, 0x3c, 0x05]),
    (0x00869723, &[0x8b, 0x44, 0x24, 0x0c, 0x40]),
    (0x0086a077, &[0x8b, 0x4c, 0x24, 0x54, 0x5f, 0x5e, 0x5b]),
];

static ENTRY: AtomicUsize = AtomicUsize::new(0);
static DISPATCH: AtomicUsize = AtomicUsize::new(0);
static EXIT: AtomicUsize = AtomicUsize::new(0);
static LOOP: AtomicUsize = AtomicUsize::new(0);
static RETURN: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_ACTOR: AtomicUsize = AtomicUsize::new(0);

pub(super) fn select_actor(actor: usize) {
    ACTIVE_ACTOR.store(actor, Ordering::Release);
}

/// PUSHAD/PUSHFD layout. ESP is the value after PUSHFD, not the native ESP.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Registers {
    pub edi: u32,
    pub esi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
    pub flags: u32,
}

#[derive(Clone)]
pub(super) struct Continuation {
    pub registers: Registers,
    pub count: u32,
    pub floating: [u8; 512],
}

impl Registers {
    pub(super) unsafe fn count(&self) -> u32 {
        // Native var_4C is [ESP+0x0c] at each safe point.
        unsafe { std::ptr::read_unaligned((self.esp as usize + 16) as *const u32) }
    }

    pub(super) unsafe fn save(&self, floating: &[u8; 512]) -> Continuation {
        Continuation {
            registers: *self,
            count: unsafe { self.count() },
            floating: *floating,
        }
    }

    pub(super) unsafe fn restore(&mut self, saved: &Continuation, floating: &mut [u8; 512]) {
        self.edi = saved.registers.edi;
        self.ebx = saved.registers.ebx;
        self.edx = saved.registers.edx;
        self.ecx = saved.registers.ecx;
        self.eax = saved.registers.eax;
        self.flags = saved.registers.flags;
        *floating = saved.floating;
        // Keep this invocation's ESI, EBP, ESP and callee-saved stack intact.
        unsafe { std::ptr::write_unaligned((self.esp as usize + 16) as *mut u32, saved.count) };
    }
}

pub(in crate::provider::native) unsafe fn install(
    hooks: &mut HookSet<State>,
    base: usize,
) -> Result<(), String> {
    select_actor(0);
    LOOP.store(base + 0x00869723, Ordering::Release);
    RETURN.store(base + 0x0086a077, Ordering::Release);
    for ((rva, _), (name, detour, slot)) in SIGNATURES.iter().zip([
        ("AI debugger resume", entry_shim as *mut _, &ENTRY),
        (
            "AI debugger instruction",
            dispatch_shim as *mut _,
            &DISPATCH,
        ),
        ("AI debugger return", exit_shim as *mut _, &EXIT),
    ]) {
        let trampoline = unsafe { hooks.create(name, (base + rva) as *mut _, detour) }?;
        slot.store(trampoline as usize, Ordering::Release);
    }
    Ok(())
}

// Preserve all integer, flags, x87 and SSE state around Rust. The unused saved
// ESP slot carries the destination across POPAD; no global scratch is shared.
macro_rules! shim {
    ($name:ident, $callback:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "pushfd", "pushad", "mov ebp, esp", "sub esp, 528", "and esp, -16",
                // FXSAVE leaves reserved bytes untouched. Initialize the whole
                // buffer before Rust copies it into an owned continuation.
                "mov edi, esp", "xor eax, eax", "mov ecx, 128", "cld", "rep stosd",
                "fxsave [esp]", "mov eax, esp", "push eax", "push ebp",
                "call {callback}", "add esp, 8", "mov [ebp + 12], eax",
                "fxrstor [esp]", "mov esp, ebp", "popad", "popfd", "jmp dword ptr [esp - 24]",
                callback = sym $callback,
            );
        }
    };
}
#[cfg(test)]
pub(super) use shim;
shim!(entry_body, enter);
shim!(dispatch_body, dispatch);
shim!(exit_body, leave);

// Unselected actors avoid Rust, locks and FP state saves entirely.
macro_rules! selected_shim {
    ($name:ident, $body:ident, $fallback:ident) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "pushfd", "cmp esi, dword ptr [{active}]", "je 2f", "popfd",
                "jmp dword ptr [{fallback}]", "2:", "popfd", "jmp {body}",
                active = sym ACTIVE_ACTOR, fallback = sym $fallback, body = sym $body,
            );
        }
    };
}
selected_shim!(entry_shim, entry_body, ENTRY);
selected_shim!(dispatch_shim, dispatch_body, DISPATCH);
selected_shim!(exit_shim, exit_body, EXIT);

unsafe extern "C" fn enter(registers: &mut Registers, floating: &mut [u8; 512]) -> usize {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let invocation = SLOT.enter();
        if let Some(state) = invocation.state() {
            unsafe { super::enter(state, registers, floating) }
        } else {
            false
        }
    }));
    if matches!(result, Ok(true)) {
        LOOP.load(Ordering::Acquire)
    } else {
        ENTRY.load(Ordering::Acquire)
    }
}

unsafe extern "C" fn dispatch(registers: &mut Registers, floating: &mut [u8; 512]) -> usize {
    let pause = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let invocation = SLOT.enter();
        if let Some(state) = invocation.state() {
            unsafe { super::instruction(state, registers, floating) }
        } else {
            false
        }
    }))
    .unwrap_or(false);
    if pause {
        RETURN.load(Ordering::Acquire)
    } else {
        DISPATCH.load(Ordering::Acquire)
    }
}

unsafe extern "C" fn leave(registers: &mut Registers, _: &mut [u8; 512]) -> usize {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let invocation = SLOT.enter();
        if let Some(state) = invocation.state() {
            unsafe { super::leave(state, registers) };
        }
    }));
    EXIT.load(Ordering::Acquire)
}
