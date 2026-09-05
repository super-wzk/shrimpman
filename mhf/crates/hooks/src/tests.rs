use super::*;
use std::{
    mem,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

type Unary = unsafe extern "C" fn(u32) -> u32;

// Fixed, distinct prologues keep these actual inline-hook tests independent of
// optimizer inlining, tail calls, and identical-code folding.
macro_rules! target {
    ($name:ident, $increment:literal) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name(_: u32) -> u32 {
            core::arch::naked_asm!(
                "nop", "nop", "nop", "nop", "nop",
                "mov eax, [esp + 4]",
                "add eax, {increment}",
                "ret",
                increment = const $increment,
            );
        }
    };
}

target!(target_a, 1);
target!(target_b, 2);
target!(target_c, 3);
target!(target_blocking, 4);

static SLOT_A: HookSlot<Unary> = HookSlot::new();
static SLOT_B: HookSlot<Unary> = HookSlot::new();
static SLOT_C: HookSlot<Unary> = HookSlot::new();

unsafe extern "C" fn detour_a(value: u32) -> u32 {
    let invocation = SLOT_A.enter();
    match invocation.state() {
        Some(original) => unsafe { original(value) + 100 },
        None => unsafe { target_a(value) },
    }
}

unsafe extern "C" fn detour_b(value: u32) -> u32 {
    let invocation = SLOT_B.enter();
    match invocation.state() {
        Some(original) => unsafe { original(value) + 200 },
        None => unsafe { target_b(value) },
    }
}

unsafe fn prepare<T: Send + Sync + 'static>(
    hooks: &mut HookSet<T>,
    name: &str,
    target: Unary,
    detour: Unary,
) -> Unary {
    let original =
        unsafe { hooks.create(name, target as *mut c_void, detour as *mut c_void) }.unwrap();
    unsafe { mem::transmute::<*mut c_void, Unary>(original) }
}

fn call(function: Unary) -> u32 {
    unsafe { std::hint::black_box(function)(10) }
}

#[test]
fn groups_roll_back_without_enabling_or_removing_each_others_hooks() {
    let mut pending_b = SLOT_B.prepare().unwrap();
    assert!(
        SLOT_B.prepare().is_err(),
        "reserve the slot before any native pointer changes"
    );
    let original_b = unsafe { prepare(&mut pending_b, "B", target_b, detour_b) };
    let mut pending_a = SLOT_A.prepare().unwrap();
    let original_a = unsafe { prepare(&mut pending_a, "A", target_a, detour_a) };
    let mut a = unsafe { pending_a.install(original_a) }.unwrap();
    assert_eq!(call(target_a), 111);
    assert_eq!(call(target_b), 12, "preparing B must not enable it with A");

    // A later create failure must undo C, but leave the pre-existing A alone.
    let mut failed = SLOT_C.prepare().unwrap();
    unsafe { prepare(&mut failed, "C", target_c, detour_a) };
    assert!(
        unsafe {
            failed.create(
                "duplicate A",
                target_a as *mut c_void,
                detour_a as *mut c_void,
            )
        }
        .is_err()
    );
    drop(failed);
    assert_eq!(call(target_a), 111);
    let mut retry_c = SLOT_C.prepare().unwrap();
    unsafe { prepare(&mut retry_c, "C", target_c, detour_a) };
    assert!(
        SLOT_A.prepare().is_err(),
        "an occupied slot must reject another group"
    );
    drop(retry_c);

    let b = unsafe { pending_b.install(original_b) }.unwrap();
    a.uninstall().unwrap();
    assert_eq!(call(target_a), 11);
    assert_eq!(
        call(detour_a),
        11,
        "late callbacks must use the restored target"
    );
    assert_eq!(call(target_b), 212, "uninstalling A must leave B active");
    drop(b);

    // Fault injection: remove the second prepared target before activation so
    // MinHook fails after enabling A. The group must roll A back and clear state.
    let mut failed = SLOT_A.prepare().unwrap();
    let original_a = unsafe { prepare(&mut failed, "A", target_a, detour_a) };
    unsafe { prepare(&mut failed, "B", target_b, detour_b) };
    unsafe { MinHook::remove_hook(target_b as *mut c_void) }.unwrap();
    assert!(unsafe { failed.install(original_a) }.is_err());
    assert_eq!(call(target_a), 11);
    assert!(SLOT_A.enter().state().is_none());

    let mut retry = SLOT_A.prepare().unwrap();
    a.uninstall().unwrap(); // An empty old handle must not lock the new preparation.
    let original_a = unsafe { prepare(&mut retry, "A", target_a, detour_a) };
    let retry = unsafe { retry.install(original_a) }.unwrap();
    assert_eq!(call(target_a), 111);
    drop(a); // A's old, already uninstalled handle cannot remove its replacement.
    assert_eq!(call(target_a), 111);
    drop(retry);
    assert_eq!(call(target_a), 11);
    assert_eq!(call(target_b), 12);
}

struct BlockingState {
    original: Unary,
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

static BLOCKING: HookSlot<BlockingState> = HookSlot::new();

unsafe extern "C" fn blocking_detour(value: u32) -> u32 {
    let invocation = BLOCKING.enter();
    let Some(state) = invocation.state() else {
        return unsafe { target_blocking(value) };
    };
    state.entered.send(()).unwrap();
    state
        .release
        .lock()
        .unwrap()
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    // Cleanup is already waiting. The trampoline must still be callable here.
    unsafe { (state.original)(value) + 400 }
}

#[test]
fn uninstall_drains_the_original_call_and_allows_late_callbacks_to_return() {
    let (entered, arrival) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut hooks = BLOCKING.prepare().unwrap();
    let original = unsafe { prepare(&mut hooks, "blocking", target_blocking, blocking_detour) };
    let state = BlockingState {
        original,
        entered,
        release: Mutex::new(wait),
    };
    let mut guard = unsafe { hooks.install(state) }.unwrap();
    let callback = thread::spawn(|| call(target_blocking));
    arrival.recv_timeout(Duration::from_secs(10)).unwrap();

    let completed = Arc::new(AtomicBool::new(false));
    let observer_completed = Arc::clone(&completed);
    let observer = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while BLOCKING.enter().state().is_some() && Instant::now() < deadline {
            thread::yield_now();
        }
        let retired = BLOCKING.enter().state().is_none();
        let returned_early = observer_completed.load(Ordering::Acquire);
        let late_result = if retired {
            Some(call(blocking_detour))
        } else {
            None
        };
        release.send(()).unwrap();
        (retired, returned_early, late_result)
    });

    guard.uninstall().unwrap();
    completed.store(true, Ordering::Release);
    assert_eq!(observer.join().unwrap(), (true, false, Some(14)));
    assert_eq!(callback.join().unwrap(), 414);
    assert_eq!(call(target_blocking), 14);
}
