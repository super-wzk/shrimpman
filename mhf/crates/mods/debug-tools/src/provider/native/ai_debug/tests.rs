//! Windows/x86 tests for the real naked shim and continuation frame contract.
use super::hooks::{self, Registers};
use std::sync::atomic::{AtomicBool, Ordering};

/// Runs inside the opt-in DLL lifecycle test. The tiny script uses only inline
/// native opcodes, with command mode suppressing the actual action dispatcher.
/// The game's actor-pool global is restored even when an assertion fails.
pub(in crate::provider::native) unsafe fn exercise_supported_native_dispatch(state: &super::State) {
    use super::{AiDebugOperation, AiTarget, apply_command, before_frame, get, monster};
    use crate::provider::native::put;
    use std::sync::PoisonError;

    struct Restore(usize, u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe { put(self.0, self.1) };
        }
    }
    let mut actor = vec![0_u32; monster::STRIDE / 4];
    let address = actor.as_mut_ptr() as usize;
    let code = [0x92_u8, 0x48, 2, 0x92, 0x05, 0, 0, 0, 0xff, 0];
    let mut main = vec![0_u32; 256];
    main[0] = code.as_ptr() as usize as u32;
    let mut root = vec![0_u32; 271];
    root[0] = main.as_ptr() as usize as u32;
    let pool = state.address(monster::POOL);
    let _restore = Restore(pool, unsafe { get(pool) });
    unsafe { put(pool, address as u32) };
    let target = AiTarget {
        epoch: 77,
        pool: address as u32,
        slot: 0,
        serial: 11,
        model: 1,
        species: 1,
    };
    let reset = || unsafe {
        std::ptr::write_bytes(address as *mut u8, 0, monster::STRIDE);
        put(address, 1_u8);
        put(address + 3, 1_u8);
        put(address + 1656, 1_u32);
        put(address + 3448, 11_u32);
        put(address + 2544, root.as_ptr() as usize as u32);
        put(address + 2548, code.as_ptr() as usize as u32);
        put(address + 2652, code.as_ptr() as usize as u32);
        put(address + 2739, 1_u8);
    };
    let original: unsafe extern "thiscall" fn(usize) -> i8 =
        unsafe { std::mem::transmute(state.monster_ai) };

    reset();
    let project=mhf_monster::ai::dsl::Project::single(None,1,"mhf_ai 1; species 1; base native; fn main() { native(0x92, 0x48, 2, 0x92, 0x05, 0, 0, 0); }".into());
    let compiled = project.compile().unwrap();
    let descriptor = root.as_ptr() as usize as u32;
    let bindings = mhf_monster::ai::matched_script_bindings(
        &compiled.program,
        descriptor,
        &super::monster_ai::Live,
    )
    .unwrap();
    assert_eq!(
        bindings.len(),
        1,
        "source mapping matches unchanged native bytes without publishing"
    );
    let source = super::InstalledSource {
        target,
        descriptor,
        debug_info: std::sync::Arc::new(compiled.debug_info),
        scripts: bindings,
    };
    unsafe { apply_command(state, target, AiDebugOperation::Attach, Some(&source)) }.unwrap();
    unsafe {
        put(address + 2601, 1_u8);
        monster::select_action(address);
        put(address + 2601, 0_u8)
    };
    unsafe { apply_command(state, target, AiDebugOperation::Continue, None) }.unwrap();
    unsafe { before_frame(state, &[target]) };
    assert_eq!(
        unsafe { get::<u8>(address + 2601) },
        1,
        "Continue restores the suppressed native trigger"
    );
    assert!(
        super::snapshot(state).unwrap().recording.entries.is_empty(),
        "Continue does not start an untriggered decision early"
    );
    unsafe { original(address) };
    let capture = super::snapshot(state).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&capture, &super::snapshot(state).unwrap()),
        "unchanged UI snapshots reuse recordings and source metadata"
    );
    assert!(!capture.recording.scripts[0].source_spans.is_empty());
    assert_eq!(
        capture
            .recording
            .entries
            .iter()
            .map(|e| e.opcode)
            .collect::<Vec<_>>(),
        [0x92, 0x48, 0x92, 0x05]
    );
    assert_eq!(
        capture.recording.entries.last().unwrap().outcome,
        mhf_ai_debug::Outcome::Yield
    );
    capture.recording.validate().unwrap();
    unsafe { apply_command(state, target, AiDebugOperation::Detach, None) }.unwrap();
    assert!(!super::snapshot(state).unwrap().attached);
    assert_eq!(super::snapshot(state).unwrap().recording.entries.len(), 4);

    reset();
    unsafe { apply_command(state, target, AiDebugOperation::Attach, None) }.unwrap();
    unsafe { apply_command(state, target, AiDebugOperation::StepInstruction, None) }.unwrap();
    unsafe { original(address) };
    {
        let runtime = state
            .ai_debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let session = runtime.session.as_ref().unwrap();
        assert_eq!(session.resume.as_ref().unwrap().continuation.count, 1);
        assert_eq!(session.debug.trace.entries().len(), 1);
    }
    unsafe { apply_command(state, target, AiDebugOperation::StepInstruction, None) }.unwrap();
    unsafe { before_frame(state, &[target]) };
    {
        let runtime = state
            .ai_debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let session = runtime.session.as_ref().unwrap();
        assert_eq!(session.resume.as_ref().unwrap().continuation.count, 2);
        assert_eq!(session.debug.trace.entries().len(), 2);
        assert_eq!(
            unsafe { get::<u16>(address + 3228) },
            2,
            "48 writes delay without yielding this pass"
        );
    }
    unsafe { apply_command(state, target, AiDebugOperation::Detach, None) }.unwrap();
    assert_eq!(
        unsafe { get::<u32>(address + 2548) },
        code.as_ptr() as usize as u32 + 8,
        "detach completes the parked native turn exactly once"
    );
    assert!(
        state
            .ai_debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session
            .is_none()
    );

    reset();
    unsafe { apply_command(state, target, AiDebugOperation::Attach, None) }.unwrap();
    unsafe { apply_command(state, target, AiDebugOperation::StepInstruction, None) }.unwrap();
    unsafe { original(address) };
    unsafe {
        put(address + 2680, 1_u8);
        before_frame(state, &[target])
    };
    let capture = super::snapshot(state).unwrap();
    assert!(capture.reason.contains("变化"));
    assert!(unsafe { apply_command(state, target, AiDebugOperation::Continue, None) }.is_err());
    unsafe { apply_command(state, target, AiDebugOperation::Detach, None) }.unwrap();

    reset();
    unsafe { apply_command(state, target, AiDebugOperation::Attach, None) }.unwrap();
    unsafe { put(address + 3448, 12_u32) };
    assert!(
        !super::should_suspend(state, address, false),
        "a reused slot must not inherit the old instance's pause"
    );
    assert!(super::should_suspend(state, address, true));
    unsafe { before_frame(state, &[]) };
    assert!(!super::snapshot(state).unwrap().attached);
}

static CALLBACK_OK: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn probe(registers: &mut Registers, floating: &mut [u8; 512]) -> usize {
    CALLBACK_OK.store(
        registers.edi == 0x1122_3344
            && registers.esi == 0x2233_4455
            && registers.ebp == 0x3344_5566
            && registers.ebx == 0x4455_6677
            && registers.edx == 0x5566_7788
            && registers.ecx == 0x6677_8899
            && registers.eax == 0x7788_99aa
            && registers.flags & 1 != 0
            && (floating.as_ptr() as usize).is_multiple_of(16)
            && floating[160..164] == 1.0_f32.to_bits().to_le_bytes(),
        Ordering::Relaxed,
    );
    registers.eax = 0x1020_3040;
    registers.flags |= 0x40;
    // The callback deliberately destroys both FP register families. The shim
    // must restore them even when it also propagates integer/flags edits.
    unsafe { core::arch::asm!("fninit", "pxor xmm0, xmm0", clobber_abi("C")) };
    probe_return as *const () as usize
}

hooks::shim!(probe_shim, probe);

#[unsafe(naked)]
unsafe extern "C" fn probe_return() {
    core::arch::naked_asm!("ret");
}

#[unsafe(naked)]
unsafe extern "C" fn probe_caller(_: *mut u32) -> u32 {
    core::arch::naked_asm!(
        "push ebp", "push ebx", "push esi", "push edi",
        "fld1", "mov eax, 0x3f800000", "movd xmm0, eax",
        "mov edi, 0x11223344", "mov esi, 0x22334455",
        "mov ebp, 0x33445566", "mov ebx, 0x44556677",
        "mov edx, 0x55667788", "mov ecx, 0x66778899",
        "mov eax, 0x778899aa", "stc", "call {shim}",
        // Capture the state before modifying registers or flags again. The
        // caller's output argument is 4 + 16 + 36 bytes above this saved frame.
        "pushfd", "pushad", "mov edi, [esp + 56]", "mov esi, esp",
        "mov ecx, 9", "cld", "rep movsd",
        "movd eax, xmm0", "mov [edi], eax", "fstp dword ptr [edi + 4]",
        "popad", "popfd", "pop edi", "pop esi", "pop ebx", "pop ebp", "ret",
        shim = sym probe_shim,
    );
}

#[test]
fn naked_shim_preserves_native_stack_registers_flags_and_floating_state() {
    let mut output = [0_u32; 11];
    let result = unsafe { probe_caller(output.as_mut_ptr()) };
    assert!(CALLBACK_OK.load(Ordering::Relaxed));
    assert_eq!(result, 0x1020_3040);
    assert_eq!(&output[..3], &[0x1122_3344, 0x2233_4455, 0x3344_5566]);
    assert_eq!(
        &output[4..8],
        &[0x4455_6677, 0x5566_7788, 0x6677_8899, 0x1020_3040]
    );
    assert_eq!(
        output[8] & 0x41,
        0x41,
        "restored flags include carry and callback's zero flag"
    );
    assert_eq!(
        output[9],
        1.0_f32.to_bits(),
        "XMM0 survives the Rust callback"
    );
    assert_eq!(
        output[10],
        1.0_f32.to_bits(),
        "x87 stack survives the Rust callback"
    );
}

fn registers(stack: &mut [u32; 32], seed: u32) -> Registers {
    Registers {
        edi: seed + 1,
        esi: seed + 2,
        ebp: seed + 3,
        // The PUSHAD ESP field is four bytes below native ESP, after PUSHFD.
        esp: stack.as_mut_ptr() as usize as u32,
        ebx: seed + 4,
        edx: seed + 5,
        ecx: seed + 6,
        eax: seed + 7,
        flags: seed + 8,
    }
}

#[test]
fn continuation_restores_budget_and_scratch_without_restoring_old_frame() {
    let mut old_stack = [0xaaaa_aaaa; 32];
    old_stack[4] = 998;
    let original = registers(&mut old_stack, 0x1000);
    let floating = std::array::from_fn(|index| (index % 251) as u8);
    let saved = unsafe { original.save(&floating) };
    let mut fresh_stack = [0xbbbb_bbbb; 32];
    let mut fresh = registers(&mut fresh_stack, 0x2000);
    let expected_frame = (fresh.esi, fresh.ebp, fresh.esp);
    let mut restored_floating = [0_u8; 512];
    unsafe { fresh.restore(&saved, &mut restored_floating) };
    assert_eq!((fresh.esi, fresh.ebp, fresh.esp), expected_frame);
    assert_eq!(
        (
            fresh.edi,
            fresh.ebx,
            fresh.edx,
            fresh.ecx,
            fresh.eax,
            fresh.flags
        ),
        (
            original.edi,
            original.ebx,
            original.edx,
            original.ecx,
            original.eax,
            original.flags
        ),
    );
    assert_eq!(
        unsafe { fresh.count() },
        998,
        "single-step does not grant a new dispatch budget"
    );
    assert_eq!(restored_floating, floating);
    assert_eq!(old_stack[4], 998);
    for (index, value) in fresh_stack.iter().enumerate() {
        assert_eq!(
            *value,
            if index == 4 { 998 } else { 0xbbbb_bbbb },
            "fresh stack slot {index}"
        );
    }
}

#[test]
fn saved_register_layout_matches_pushad_and_pushfd() {
    assert_eq!(std::mem::size_of::<Registers>(), 36);
    assert_eq!(std::mem::offset_of!(Registers, edi), 0);
    assert_eq!(std::mem::offset_of!(Registers, esi), 4);
    assert_eq!(std::mem::offset_of!(Registers, ebp), 8);
    assert_eq!(std::mem::offset_of!(Registers, esp), 12);
    assert_eq!(std::mem::offset_of!(Registers, ebx), 16);
    assert_eq!(std::mem::offset_of!(Registers, edx), 20);
    assert_eq!(std::mem::offset_of!(Registers, ecx), 24);
    assert_eq!(std::mem::offset_of!(Registers, eax), 28);
    assert_eq!(std::mem::offset_of!(Registers, flags), 32);
}

#[test]
fn parked_control_stamp_detects_control_changes_but_allows_world_inputs() {
    let mut actor = vec![0_u8; 4096];
    let address = actor.as_mut_ptr() as usize;
    let original = unsafe { super::ControlStamp::read(address) };
    for offset in [
        2544, 2548, 2552, 2556, 2588, 2596, 2604, 2608, 2616, 2632, 2636, 2640, 2644, 2648, 2652,
        3288, 2580, 2576, 2622, 2680,
    ] {
        actor[offset] = 1;
        assert!(
            unsafe { super::ControlStamp::read(address) } != original,
            "control change at actor+{offset} invalidates continuation"
        );
        actor[offset] = 0;
    }
    // Sense/target/timer values are fresh world inputs when AI alone is paused.
    for offset in [2612, 2684, 2687, 2910, 2912, 2914] {
        actor[offset] = 1;
    }
    assert!(unsafe { super::ControlStamp::read(address) } == original);
}
