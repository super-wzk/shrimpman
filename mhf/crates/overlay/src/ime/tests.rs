use egui::{Id, Rect, pos2};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::Ime::{
    CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, GCS_RESULTSTR, HIMC, ISC_SHOWUIALL,
    ISC_SHOWUICOMPOSITIONWINDOW, ImmAssociateContext, ImmGetCandidateWindow,
    ImmGetCompositionWindow, ImmGetContext, ImmReleaseContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    WM_IME_CHAR, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT,
    WM_IME_STARTCOMPOSITION,
};

use super::{Ime, Owner, Target, clear_preedit};
use crate::window::DummyWindow;

fn current_context(hwnd: HWND) -> HIMC {
    let context = unsafe { ImmGetContext(hwnd) };
    if !context.is_invalid() {
        unsafe {
            let _ = ImmReleaseContext(hwnd, context);
        }
    }
    context
}

// Keep the native lifecycle in one test: DummyWindow's registered class and
// IMM's default context are shared resources on their owning window thread.
#[test]
fn private_context_suspends_during_capture_and_restores_native_input_afterward() {
    let window = DummyWindow::create().expect("native test requires a working window driver");
    let hwnd = window.hwnd();
    let previous_focus = unsafe { GetFocus() };
    let _ = unsafe { SetFocus(Some(hwnd)) };
    assert_eq!(
        unsafe { GetFocus() },
        hwnd,
        "native test requires the window driver to grant keyboard focus"
    );

    let host = current_context(hwnd);
    let ime = Ime::default();
    let (events, restored) = ime.sync(hwnd);
    assert!(events.is_empty());
    assert!(!restored);
    assert_eq!(current_context(hwnd), host);
    assert!(ime.lock().native.is_none());
    assert!(
        ime.handle_message(WM_IME_SETCONTEXT, LPARAM(ISC_SHOWUIALL as isize))
            .is_none(),
        "native IME messages must pass through before the overlay captures input"
    );
    ime.lock().block_host = true;
    let (events, restored) = ime.sync(hwnd);
    assert!(events.is_empty());
    assert!(!restored);
    assert!(
        current_context(hwnd).is_invalid(),
        "a blocking overlay must suspend IME before an editor receives focus"
    );
    let reply = ime
        .handle_message(WM_IME_SETCONTEXT, LPARAM(ISC_SHOWUIALL as isize))
        .unwrap();
    assert!(reply.default_proc);
    assert_eq!(reply.lparam.0, 0, "idle IME must not display candidate UI");
    let target = Target {
        owner: Owner::Overlay(Some(Id::new("first"))),
        cursor: Rect::from_min_max(pos2(10.25, 20.75), pos2(12.25, 40.75)),
    };
    {
        let mut state = ime.lock();
        state.requested = Some(target);
        state.block_host = true;
    }
    let (events, restored) = ime.sync(hwnd);
    assert!(events.is_empty());
    assert!(!restored);
    let private = current_context(hwnd);
    assert!(
        !private.is_invalid(),
        "native test requires IMM to create and associate an input context"
    );
    assert_ne!(private, host);

    let mut candidate = CANDIDATEFORM::default();
    assert!(unsafe { ImmGetCandidateWindow(private, 0, &mut candidate).as_bool() });
    assert_eq!(candidate.dwStyle, CFS_EXCLUDE);
    assert_eq!(
        (candidate.ptCurrentPos.x, candidate.ptCurrentPos.y),
        (10, 20)
    );
    assert_eq!(
        (
            candidate.rcArea.left,
            candidate.rcArea.top,
            candidate.rcArea.right,
            candidate.rcArea.bottom,
        ),
        (10, 20, 13, 41)
    );
    let mut composition = COMPOSITIONFORM::default();
    assert!(unsafe { ImmGetCompositionWindow(private, &mut composition).as_bool() });
    assert_eq!(composition.dwStyle, CFS_POINT);
    assert_eq!(
        (composition.ptCurrentPos.x, composition.ptCurrentPos.y),
        (10, 41)
    );

    let reply = ime
        .handle_message(WM_IME_SETCONTEXT, LPARAM(ISC_SHOWUIALL as isize))
        .unwrap();
    assert!(reply.default_proc, "system IME retains candidate handling");
    assert_eq!(
        reply.lparam.0 as u32,
        ISC_SHOWUIALL & !ISC_SHOWUICOMPOSITIONWINDOW
    );
    let reply = ime.handle_message(WM_IME_CHAR, LPARAM(0)).unwrap();
    assert!(!reply.default_proc);
    assert!(
        reply.events.is_empty(),
        "WM_IME_CHAR must not duplicate commits"
    );

    ime.handle_message(WM_IME_STARTCOMPOSITION, LPARAM(0))
        .unwrap();
    assert!(ime.composing());
    ime.lock().requested = Some(Target {
        owner: Owner::Overlay(Some(Id::new("second"))),
        ..target
    });
    let (events, restored) = ime.sync(hwnd);
    assert_eq!(events, vec![clear_preedit()]);
    assert!(!restored);
    assert!(!ime.composing());
    assert_eq!(current_context(hwnd), private);

    ime.handle_message(WM_IME_STARTCOMPOSITION, LPARAM(0))
        .unwrap();
    let reply = ime
        .handle_message(WM_IME_ENDCOMPOSITION, LPARAM(0))
        .unwrap();
    assert_eq!(reply.events, vec![clear_preedit()]);
    let reply = ime
        .handle_message(WM_IME_COMPOSITION, LPARAM(GCS_RESULTSTR.0 as isize))
        .unwrap();
    assert!(
        reply.events.is_empty(),
        "ignore a trailing Hangul result after END"
    );
    assert!(!reply.default_proc);

    ime.handle_message(WM_IME_STARTCOMPOSITION, LPARAM(0))
        .unwrap();
    let _ = unsafe { SetFocus(None) };
    assert_ne!(unsafe { GetFocus() }, hwnd);
    let (events, restored) = ime.sync(hwnd);
    assert_eq!(events, vec![clear_preedit()]);
    assert!(!restored);
    assert!(current_context(hwnd).is_invalid());
    let reply = ime.handle_message(WM_IME_CHAR, LPARAM(0)).unwrap();
    assert!(!reply.default_proc);
    assert!(reply.events.is_empty());

    let _ = unsafe { SetFocus(Some(hwnd)) };
    assert_eq!(unsafe { GetFocus() }, hwnd);
    let (_, restored) = ime.sync(hwnd);
    assert!(!restored);
    assert_eq!(current_context(hwnd), private);
    let (_, restored) = ime.stop(hwnd, false);
    assert!(!restored);
    assert!(current_context(hwnd).is_invalid());

    ime.lock().requested = Some(target);
    let (_, restored) = ime.sync(hwnd);
    assert!(!restored);
    assert_eq!(current_context(hwnd), private);
    ime.handle_message(WM_IME_STARTCOMPOSITION, LPARAM(0))
        .unwrap();
    {
        let mut state = ime.lock();
        state.requested = None;
        state.block_host = false;
    }
    let (events, restored) = ime.sync(hwnd);
    assert_eq!(events, vec![clear_preedit()]);
    assert!(restored);
    assert_eq!(current_context(hwnd), host);
    assert!(ime.lock().native.is_none());
    assert!(!ime.composing());
    assert!(ime.handle_message(WM_IME_CHAR, LPARAM(0)).is_none());
    let (events, restored) = ime.sync(hwnd);
    assert!(events.is_empty());
    assert!(!restored);
    assert_eq!(current_context(hwnd), host);

    {
        let mut state = ime.lock();
        state.requested = Some(target);
        state.block_host = true;
    }
    let (_, restored) = ime.sync(hwnd);
    assert!(!restored);
    assert!(!current_context(hwnd).is_invalid());
    assert_ne!(current_context(hwnd), host);
    let (_, restored) = ime.stop(hwnd, true);
    assert!(restored);
    assert_eq!(current_context(hwnd), host);

    // A game can explicitly disassociate its context while no native editor is
    // active. Restoring that state must preserve null instead of enabling IME.
    let detached_host = unsafe { ImmAssociateContext(hwnd, HIMC::default()) };
    assert_eq!(detached_host, host);
    assert!(current_context(hwnd).is_invalid());
    let ime = Ime::default();
    let (_, restored) = ime.sync(hwnd);
    assert!(!restored);
    assert!(current_context(hwnd).is_invalid());
    {
        let mut state = ime.lock();
        state.requested = Some(target);
        state.block_host = true;
    }
    let (_, restored) = ime.sync(hwnd);
    assert!(!restored);
    assert!(!current_context(hwnd).is_invalid());
    let (_, restored) = ime.stop(hwnd, true);
    assert!(restored);
    assert!(current_context(hwnd).is_invalid());
    unsafe {
        let _ = ImmAssociateContext(hwnd, detached_host);
        let _ = SetFocus((!previous_focus.is_invalid()).then_some(previous_focus));
    }
}
