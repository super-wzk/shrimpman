use std::sync::atomic::{AtomicUsize, Ordering};

use egui::{Event, Id, ImeEvent, Key, Rect, output::IMEOutput, pos2};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::Ime::{
    HIMC, IME_CMODE_FULLSHAPE, IME_CMODE_NATIVE, IME_CONVERSION_MODE, IME_SENTENCE_MODE,
    IME_SMODE_NONE, IMN_OPENCANDIDATE, ImmGetContext, ImmGetConversionStatus, ImmGetOpenStatus,
    ImmReleaseContext, ImmSetConversionStatus, ImmSetOpenStatus,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateCaret, CreateWindowExA, DefWindowProcA, DestroyCaret, GUI_CARETBLINKING, GUITHREADINFO,
    GetGUIThreadInfo, GetWindowLongPtrW, IsWindowUnicode, RegisterClassExA, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetCaretPos, SetWindowPos, ShowCaret, WM_CHAR,
    WM_IME_CHAR, WM_IME_COMPOSITION, WM_IME_NOTIFY, WM_IME_STARTCOMPOSITION, WNDCLASSEXA,
};
use windows::core::PCSTR;

use super::*;
use crate::{HostIme, HostImeTarget, InputCapture};

static WINDOW_TEST_LOCK: Mutex<()> = Mutex::new(());
static HOST_IME_CHAR: AtomicUsize = AtomicUsize::new(0);
static HOST_COMPOSITION: AtomicUsize = AtomicUsize::new(0);
static HOST_IME_NOTIFY: AtomicUsize = AtomicUsize::new(0);
static HOST_CHAR: AtomicUsize = AtomicUsize::new(0);
static HOST_KEY: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let counter = match message {
        WM_IME_CHAR => Some(&HOST_IME_CHAR),
        WM_IME_COMPOSITION => Some(&HOST_COMPOSITION),
        WM_IME_NOTIFY => Some(&HOST_IME_NOTIFY),
        WM_CHAR => Some(&HOST_CHAR),
        WM_KEYDOWN | WM_KEYUP => Some(&HOST_KEY),
        _ => None,
    };
    if let Some(counter) = counter {
        counter.fetch_add(1, Ordering::Relaxed);
        LRESULT(71)
    } else {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}

struct HostWindow {
    hwnd: HWND,
    instance: HINSTANCE,
}

impl HostWindow {
    fn new_ansi() -> Self {
        unsafe extern "system" fn ansi_proc(
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if matches!(message, WM_CHAR | WM_KEYDOWN | WM_KEYUP) {
                unsafe { host_proc(hwnd, message, wparam, lparam) }
            } else {
                unsafe { DefWindowProcA(hwnd, message, wparam, lparam) }
            }
        }
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let class = WNDCLASSEXA {
            cbSize: mem::size_of::<WNDCLASSEXA>() as u32,
            lpfnWndProc: Some(ansi_proc),
            hInstance: instance,
            lpszClassName: PCSTR(c"ShrimpmanOverlayImeRoutingTest".as_ptr().cast()),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassExA(&class) }, 0);
        let hwnd = unsafe {
            CreateWindowExA(
                WINDOW_EX_STYLE::default(),
                class.lpszClassName,
                PCSTR(c"ANSI host input test".as_ptr().cast()),
                WS_OVERLAPPED,
                0,
                0,
                200,
                100,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .unwrap();
        assert!(!unsafe { IsWindowUnicode(hwnd).as_bool() });
        Self { hwnd, instance }
    }

    fn new() -> Self {
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        let class = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: instance,
            lpszClassName: w!("ShrimpmanOverlayImeRoutingTest"),
            ..WNDCLASSEXW::default()
        };
        assert_ne!(unsafe { RegisterClassExW(&class) }, 0);
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class.lpszClassName,
                w!("Overlay IME routing test"),
                WS_OVERLAPPED,
                0,
                0,
                200,
                100,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .expect("native routing test requires a working window driver");
        Self { hwnd, instance }
    }
}

impl Drop for HostWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(w!("ShrimpmanOverlayImeRoutingTest"), Some(self.instance));
        }
    }
}

fn send(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }
}

fn current_context(hwnd: HWND) -> HIMC {
    let context = unsafe { ImmGetContext(hwnd) };
    if !context.is_invalid() {
        unsafe {
            let _ = ImmReleaseContext(hwnd, context);
        }
    }
    context
}

fn input_mode(context: HIMC) -> (bool, IME_CONVERSION_MODE, IME_SENTENCE_MODE) {
    let mut conversion = IME_CONVERSION_MODE::default();
    let mut sentence = IME_SENTENCE_MODE::default();
    assert!(unsafe {
        ImmGetConversionStatus(context, Some(&mut conversion), Some(&mut sentence)).as_bool()
    });
    (
        unsafe { ImmGetOpenStatus(context).as_bool() },
        conversion,
        sentence,
    )
}

fn gui_thread_info() -> GUITHREADINFO {
    let mut info = GUITHREADINFO {
        cbSize: mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(GetCurrentThreadId(), &mut info) }.unwrap();
    info
}

fn caret_bounds(info: &GUITHREADINFO) -> (i32, i32, i32, i32) {
    let rect = info.rcCaret;
    (rect.left, rect.top, rect.right, rect.bottom)
}

#[test]
fn system_caret_tracks_both_editors_and_window_position_for_macos_ime() {
    let _guard = WINDOW_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let window = HostWindow::new();
    let hwnd = window.hwnd;
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            300,
            200,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .unwrap();
    let _ = unsafe { SetFocus(Some(hwnd)) };
    assert!(gui_thread_info().hwndCaret.is_invalid());

    let host_input = Arc::new(Mutex::new(HostInput {
        target: Some(HostImeTarget {
            id: 7,
            cursor_rect: Rect::from_min_max(pos2(20.0, 30.0), pos2(21.0, 50.0)),
        }),
        ..Default::default()
    }));
    let host = Arc::new(HostAdapter {
        input: Arc::clone(&host_input),
        thread: std::thread::current().id(),
        window: hwnd.0 as usize,
    });
    let state = Arc::new(WindowState::new(
        hwnd,
        InputCaptureState::default(),
        Some(host),
    ));
    let binding = WindowBinding::install(hwnd, Arc::clone(&state)).unwrap();
    send(hwnd, control_message().unwrap(), 0, 0);
    let info = gui_thread_info();
    assert_eq!(info.hwndCaret, hwnd);
    assert_eq!(caret_bounds(&info), (20, 30, 21, 50));
    // Wine 9 sets GUI_CARETBLINKING for any caret handle, including a hidden
    // one. The helper uses CreateCaret's hidden default and never ShowCaret.

    let cursor = Rect::from_min_max(pos2(40.0, 25.0), pos2(41.0, 45.0));
    state.ime.update(
        hwnd,
        Some(Id::new("scaled-overlay-editor")),
        Some(IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect: cursor.expand(5.0),
            cursor_rect: cursor,
            should_interrupt_composition: false,
        }),
        2.0,
        true,
    );
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(caret_bounds(&gui_thread_info()), (80, 50, 82, 90));

    // Match WineCX's query_ime_char_rect path: read the thread caret and map its
    // client coordinates using its owning HWND, even when the window moves.
    let screen_position = || {
        let info = gui_thread_info();
        let mut point = POINT {
            x: info.rcCaret.left,
            y: info.rcCaret.top,
        };
        assert!(unsafe { ClientToScreen(info.hwndCaret, &mut point).as_bool() });
        (point.x, point.y)
    };
    let before = screen_position();
    assert_ne!(before, (0, 0));
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            450,
            320,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .unwrap();
    assert_eq!(screen_position(), (before.0 + 150, before.1 + 120));

    unsafe { DestroyCaret() }.unwrap();
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(
        caret_bounds(&gui_thread_info()),
        (80, 50, 82, 90),
        "restore a lost caret even if egui's cursor did not change"
    );
    state.ime.update(hwnd, None, None, 1.0, false);
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(caret_bounds(&gui_thread_info()), (20, 30, 21, 50));
    let _ = unsafe { SetFocus(None) };
    assert!(gui_thread_info().hwndCaret.is_invalid());
    let _ = unsafe { SetFocus(Some(hwnd)) };
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(caret_bounds(&gui_thread_info()), (20, 30, 21, 50));
    drop(binding);
    assert!(gui_thread_info().hwndCaret.is_invalid());
}

#[test]
fn borrowing_host_caret_preserves_shape_visibility_and_restores_position() {
    let _guard = WINDOW_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let window = HostWindow::new();
    let hwnd = window.hwnd;
    let _ = unsafe { SetFocus(Some(hwnd)) };
    unsafe {
        CreateCaret(hwnd, None, 4, 17).unwrap();
        SetCaretPos(3, 7).unwrap();
        ShowCaret(Some(hwnd)).unwrap();
    }
    let original = gui_thread_info();
    let state = Arc::new(WindowState::new(hwnd, InputCaptureState::default(), None));
    let binding = WindowBinding::install(hwnd, Arc::clone(&state)).unwrap();
    activate_editor(&state, hwnd);
    let active = gui_thread_info();
    assert_eq!(caret_bounds(&active), (10, 10, 14, 27));
    assert_eq!(
        active.flags & GUI_CARETBLINKING,
        original.flags & GUI_CARETBLINKING
    );
    drop(binding);
    let restored = gui_thread_info();
    assert_eq!(restored.hwndCaret, original.hwndCaret);
    assert_eq!(caret_bounds(&restored), caret_bounds(&original));
    assert_eq!(
        restored.flags & GUI_CARETBLINKING,
        original.flags & GUI_CARETBLINKING
    );
    unsafe { DestroyCaret() }.unwrap();
}

fn activate_editor(state: &WindowState, hwnd: HWND) {
    state.capture.lock().update(
        InputPolicy {
            keyboard: InputCapture::Block,
            ..InputPolicy::default()
        },
        false,
        true,
    );
    let cursor = Rect::from_min_max(pos2(10.0, 10.0), pos2(11.0, 30.0));
    state.ime.update(
        hwnd,
        Some(Id::new("routing-test-editor")),
        Some(IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect: cursor.expand(5.0),
            cursor_rect: cursor,
            should_interrupt_composition: false,
        }),
        1.0,
        true,
    );
    assert_eq!(send(hwnd, control_message().unwrap(), 0, 0), LRESULT(0));
}

#[derive(Default)]
struct HostInput {
    target: Option<HostImeTarget>,
    events: Vec<(usize, ImeEvent, usize)>,
}

struct HostAdapter {
    input: Arc<Mutex<HostInput>>,
    thread: std::thread::ThreadId,
    window: usize,
}

impl HostIme for HostAdapter {
    fn target(&self) -> Option<HostImeTarget> {
        assert_eq!(std::thread::current().id(), self.thread);
        self.input
            .lock()
            .unwrap()
            .target
            .as_ref()
            .map(|target| HostImeTarget {
                id: target.id,
                cursor_rect: target.cursor_rect,
            })
    }

    fn event(&self, id: usize, event: &ImeEvent) {
        assert_eq!(std::thread::current().id(), self.thread);
        let context = current_context(HWND(self.window as *mut _));
        self.input
            .lock()
            .unwrap()
            .events
            .push((id, event.clone(), context.0 as usize));
    }
}

fn take_host_events(input: &Mutex<HostInput>, expected_context: HIMC) -> Vec<(usize, ImeEvent)> {
    mem::take(&mut input.lock().unwrap().events)
        .into_iter()
        .map(|(id, event, context)| {
            assert_eq!(
                context, expected_context.0 as usize,
                "host callbacks must observe the shared context, including old-owner cancellation"
            );
            (id, event)
        })
        .collect()
}

fn empty_preedit() -> ImeEvent {
    ImeEvent::Preedit {
        text: String::new(),
        active_range_chars: None,
    }
}

fn take_overlay_ime(state: &WindowState) -> Vec<ImeEvent> {
    state
        .take_input(1.0)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event {
            Event::Ime(event) => Some(event),
            _ => None,
        })
        .collect()
}

#[test]
fn unicode_character_messages_reach_the_host_before_the_ansi_thunk() {
    let _guard = WINDOW_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    HOST_CHAR.store(0, Ordering::Relaxed);
    HOST_KEY.store(0, Ordering::Relaxed);
    let window = HostWindow::new_ansi();
    let hwnd = window.hwnd;
    let previous_focus = unsafe { SetFocus(Some(hwnd)) }.unwrap_or_default();
    let input = Arc::new(Mutex::new(HostInput {
        target: Some(HostImeTarget {
            id: 41,
            cursor_rect: Rect::from_min_max(pos2(10.0, 10.0), pos2(11.0, 30.0)),
        }),
        events: Vec::new(),
    }));
    let capture = InputCaptureState::default();
    capture.lock().update(
        InputPolicy {
            keyboard: InputCapture::PassThrough,
            ..InputPolicy::default()
        },
        false,
        false,
    );
    let host = Arc::new(HostAdapter {
        input: Arc::clone(&input),
        thread: std::thread::current().id(),
        window: hwnd.0 as usize,
    });
    let state = Arc::new(WindowState::new(hwnd, capture, Some(host)));
    let binding = WindowBinding::install(hwnd, Arc::clone(&state)).unwrap();
    assert!(unsafe { IsWindowUnicode(hwnd).as_bool() });
    let private = current_context(hwnd);
    assert!(!private.is_invalid());

    for unit in [0xe9, 0x4f60, 0xd83d, 0xde00] {
        assert_eq!(send(hwnd, WM_CHAR, unit, 1), LRESULT(0));
    }
    assert_eq!(
        take_host_events(&input, private),
        [
            (41, ImeEvent::Commit("é".to_owned())),
            (41, ImeEvent::Commit("你".to_owned())),
            (41, ImeEvent::Commit("😀".to_owned()))
        ]
    );
    assert_eq!(HOST_CHAR.load(Ordering::Relaxed), 0);
    assert!(
        state
            .take_input(1.0)
            .unwrap()
            .events
            .iter()
            .all(|event| !matches!(event, Event::Text(_) | Event::Ime(_)))
    );

    // Native editing commands still receive their key and control-character messages.
    assert_eq!(send(hwnd, WM_KEYDOWN, 8, 1), LRESULT(71));
    assert_eq!(send(hwnd, WM_CHAR, 8, 1), LRESULT(71));
    assert!(take_host_events(&input, private).is_empty());
    assert_eq!(HOST_CHAR.load(Ordering::Relaxed), 1);
    assert_eq!(HOST_KEY.load(Ordering::Relaxed), 1);

    // A pending high surrogate cannot follow the cursor into another native editor.
    send(hwnd, WM_CHAR, 0xd83d, 1);
    input.lock().unwrap().target.as_mut().unwrap().id = 42;
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(take_host_events(&input, private), [(41, empty_preedit())]);
    send(hwnd, WM_CHAR, 0xde00, 1);
    assert!(take_host_events(&input, private).is_empty());

    // Composition owns character messages, and its end cannot complete an old pair.
    send(hwnd, WM_CHAR, 0xd83d, 1);
    send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
    send(hwnd, WM_CHAR, 0xe9, 1);
    assert!(take_host_events(&input, private).is_empty());
    send(hwnd, WM_IME_COMPOSITION, 0, 0);
    assert_eq!(take_host_events(&input, private), [(42, empty_preedit())]);
    send(hwnd, WM_CHAR, 0xde00, 1);
    assert!(take_host_events(&input, private).is_empty());

    send(hwnd, WM_CHAR, 0xd83d, 1);
    let _ = unsafe { SetFocus(None) };
    assert_eq!(take_host_events(&input, private), [(42, empty_preedit())]);
    let _ = unsafe { SetFocus(Some(hwnd)) };
    send(hwnd, WM_CHAR, 0xde00, 1);
    assert!(take_host_events(&input, private).is_empty());
    send(hwnd, WM_CHAR, usize::from(b'A'), 1);
    assert_eq!(
        take_host_events(&input, private),
        [(42, ImeEvent::Commit("A".to_owned()))]
    );
    assert_eq!(HOST_CHAR.load(Ordering::Relaxed), 1);

    drop(binding);
    assert!(!unsafe { IsWindowUnicode(hwnd).as_bool() });
    let _ = unsafe { SetFocus((!previous_focus.is_invalid()).then_some(previous_focus)) };
}

fn host_owner_handoffs_reuse_context(hwnd: HWND, original: HIMC, host_window_proc: WindowLong) {
    let cursor = Rect::from_min_max(pos2(20.0, 30.0), pos2(21.0, 50.0));
    let host_input = Arc::new(Mutex::new(HostInput {
        target: Some(HostImeTarget {
            id: 7,
            cursor_rect: cursor,
        }),
        events: Vec::new(),
    }));
    let host = Arc::new(HostAdapter {
        input: Arc::clone(&host_input),
        thread: std::thread::current().id(),
        window: hwnd.0 as usize,
    });
    let capture = InputCaptureState::default();
    capture.lock().update(
        InputPolicy {
            keyboard: InputCapture::PassThrough,
            ..InputPolicy::default()
        },
        false,
        false,
    );
    let state = Arc::new(WindowState::new(hwnd, capture.clone(), Some(host)));
    let binding = WindowBinding::install(hwnd, Arc::clone(&state)).unwrap();
    send(hwnd, control_message().unwrap(), 0, 0);
    let private = current_context(hwnd);
    assert!(!private.is_invalid());
    assert_ne!(private, original);

    send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
    assert!(state.ime.composing());
    assert!(!capture.captures_keyboard());
    assert_eq!(send(hwnd, WM_KEYDOWN, 0x25, 0x014b_0001), LRESULT(1));
    assert_eq!(capture.scan_code_owner(0xcb), Some(false));
    assert_eq!(HOST_KEY.load(Ordering::Relaxed), 0);
    assert_eq!(send(hwnd, WM_KEYUP, 0x25, 0x414b_0001), LRESULT(1));
    assert_eq!(capture.scan_code_owner(0xcb), None);
    assert!(!capture.captures_keyboard());
    assert_eq!(HOST_KEY.load(Ordering::Relaxed), 0);
    assert!(
        state
            .take_input(1.0)
            .unwrap()
            .events
            .iter()
            .all(|event| !matches!(event, Event::Key { .. }))
    );
    send(hwnd, WM_IME_COMPOSITION, 0, 0);
    assert_eq!(
        take_host_events(&host_input, private),
        [(7, empty_preedit())]
    );
    assert!(take_overlay_ime(&state).is_empty());
    assert_eq!(send(hwnd, WM_KEYDOWN, 0x25, 0x014b_0001), LRESULT(71));
    assert_eq!(send(hwnd, WM_KEYUP, 0x25, 0x414b_0001), LRESULT(71));
    assert_eq!(HOST_KEY.load(Ordering::Relaxed), 2);

    send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
    activate_editor(&state, hwnd);
    assert_eq!(current_context(hwnd), private);
    assert_eq!(
        take_host_events(&host_input, private),
        [(7, empty_preedit())]
    );
    assert!(take_overlay_ime(&state).is_empty());
    send(hwnd, WM_IME_COMPOSITION, 0, 0);
    assert_eq!(take_overlay_ime(&state), [empty_preedit()]);
    assert!(take_host_events(&host_input, private).is_empty());

    // Even a visible, focused egui editor yields IME ownership when the caller
    // explicitly selects PassThrough for keyboard input.
    let context = egui::Context::default();
    let mut text = String::new();
    let mut platform = egui::PlatformOutput::default();
    for _ in 0..2 {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::Window::new("Pass-through editor").show(ui.ctx(), |ui| {
                    ui.text_edit_singleline(&mut text).request_focus();
                });
            },
        );
        output.textures_delta.clear();
        platform = output.platform_output;
    }
    assert!(platform.ime.is_some());
    send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
    state.update_capture(
        InputPolicy {
            keyboard: InputCapture::PassThrough,
            ..InputPolicy::default()
        },
        &context,
    );
    state.update_ime(&context, &platform);
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(current_context(hwnd), private);
    assert_eq!(take_overlay_ime(&state), [empty_preedit()]);
    assert!(take_host_events(&host_input, private).is_empty());
    send(hwnd, WM_IME_COMPOSITION, 0, 0);
    assert_eq!(
        take_host_events(&host_input, private),
        [(7, empty_preedit())]
    );
    assert!(take_overlay_ime(&state).is_empty());

    send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
    host_input.lock().unwrap().target = None;
    send(hwnd, control_message().unwrap(), 0, 0);
    assert!(current_context(hwnd).is_invalid());
    assert_eq!(
        take_host_events(&host_input, private),
        [(7, empty_preedit())]
    );
    host_input.lock().unwrap().target = Some(HostImeTarget {
        id: 19,
        cursor_rect: cursor,
    });
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(current_context(hwnd), private);

    // A blocking modal with no text field must not re-enable the host editor.
    state.update_capture(
        InputPolicy {
            keyboard: InputCapture::Block,
            ..InputPolicy::default()
        },
        &context,
    );
    state.update_ime(&context, &egui::PlatformOutput::default());
    send(hwnd, control_message().unwrap(), 0, 0);
    assert!(current_context(hwnd).is_invalid());
    assert_eq!(
        take_host_events(&host_input, private),
        [(19, empty_preedit())]
    );
    assert!(take_overlay_ime(&state).is_empty());
    state.update_capture(
        InputPolicy {
            keyboard: InputCapture::PassThrough,
            ..InputPolicy::default()
        },
        &context,
    );
    state.update_ime(&context, &platform);
    send(hwnd, control_message().unwrap(), 0, 0);
    assert_eq!(current_context(hwnd), private);
    drop(binding);
    assert_eq!(
        take_host_events(&host_input, private),
        [(19, empty_preedit())]
    );
    assert!(window_route().is_none());
    assert_eq!(current_context(hwnd), original);
    assert_eq!(
        unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) },
        host_window_proc
    );
}

#[test]
fn window_binding_routes_ime_and_restores_host_synchronously() {
    let _guard = WINDOW_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    HOST_IME_CHAR.store(0, Ordering::Relaxed);
    HOST_COMPOSITION.store(0, Ordering::Relaxed);
    HOST_IME_NOTIFY.store(0, Ordering::Relaxed);
    HOST_CHAR.store(0, Ordering::Relaxed);
    HOST_KEY.store(0, Ordering::Relaxed);
    let window = HostWindow::new();
    let hwnd = window.hwnd;
    let previous_focus = unsafe { GetFocus() };
    let _ = unsafe { SetFocus(Some(hwnd)) };
    assert_eq!(unsafe { GetFocus() }, hwnd);
    let host_context = current_context(hwnd);
    let host_window_proc = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };

    let capture = InputCaptureState::default();
    let state = Arc::new(WindowState::new(hwnd, capture.clone(), None));
    let binding = WindowBinding::install(hwnd, Arc::clone(&state)).unwrap();
    assert!(
        current_context(hwnd).is_invalid(),
        "install must suspend IME before the first editor or control message"
    );
    assert_eq!(send(hwnd, WM_KEYDOWN, 0x57, 0x0011_0001), LRESULT(71));
    assert_eq!(
        send(hwnd, WM_KEYUP, 0x57, 0xc011_0001u32 as isize),
        LRESULT(71)
    );
    assert_eq!(HOST_KEY.load(Ordering::Relaxed), 2);
    assert_eq!(capture.scan_code_owner(0x11), None);
    assert_eq!(send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0), LRESULT(0));
    assert_eq!(send(hwnd, WM_IME_CHAR, 0x4f60, 0), LRESULT(0));
    assert_eq!(send(hwnd, WM_IME_COMPOSITION, 0, 0), LRESULT(0));
    assert_eq!(
        send(hwnd, WM_IME_NOTIFY, IMN_OPENCANDIDATE as usize, 1),
        LRESULT(0)
    );
    assert!(!state.ime.composing());
    assert!(current_context(hwnd).is_invalid());
    assert_eq!(HOST_IME_CHAR.load(Ordering::Relaxed), 0);
    assert_eq!(HOST_COMPOSITION.load(Ordering::Relaxed), 0);
    assert_eq!(HOST_IME_NOTIFY.load(Ordering::Relaxed), 0);
    assert!(take_overlay_ime(&state).is_empty());

    activate_editor(&state, hwnd);
    let private = current_context(hwnd);
    assert!(!private.is_invalid());
    assert_ne!(private, host_context);
    assert!(unsafe { ImmSetOpenStatus(private, true).as_bool() });
    assert!(unsafe {
        ImmSetConversionStatus(
            private,
            IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE,
            IME_SMODE_NONE,
        )
        .as_bool()
    });
    let mode = input_mode(private);

    assert_eq!(send(hwnd, WM_IME_CHAR, 0x4f60, 0), LRESULT(0));
    assert_eq!(send(hwnd, WM_IME_COMPOSITION, 0x4f60, 0), LRESULT(0));
    assert_eq!(HOST_IME_CHAR.load(Ordering::Relaxed), 0);
    assert_eq!(HOST_COMPOSITION.load(Ordering::Relaxed), 0);
    let input = state.take_input(1.0).unwrap();
    assert!(
        input
            .events
            .iter()
            .all(|event| !matches!(event, Event::Text(_)))
    );
    assert!(input.events.iter().any(|event| {
        matches!(event, Event::Ime(ImeEvent::Preedit { text, .. }) if text.is_empty())
    }));

    send(hwnd, WM_CHAR, 0x4f60, 0);
    send(hwnd, WM_CHAR, 0xd83d, 0);
    send(hwnd, WM_CHAR, 0xde00, 0);
    let texts = state
        .take_input(1.0)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event {
            Event::Text(text) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["你", "😀"]);
    assert_eq!(HOST_CHAR.load(Ordering::Relaxed), 0);

    send(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
    assert!(state.ime.composing());
    send(hwnd, WM_KEYDOWN, 0x25, 0x014b_0001);
    assert_eq!(capture.scan_code_owner(0xcb), Some(true));
    send(hwnd, WM_KEYUP, 0x25, 0x414b_0001);
    assert_eq!(capture.scan_code_owner(0xcb), None);
    assert!(capture.captures_keyboard());
    assert!(
        state
            .take_input(1.0)
            .unwrap()
            .events
            .iter()
            .all(|event| !matches!(event, Event::Key { .. }))
    );

    send(hwnd, WM_IME_COMPOSITION, 0, 0);
    assert!(!state.ime.composing());
    send(hwnd, WM_KEYDOWN, 0x25, 0x014b_0001);
    send(hwnd, WM_KEYUP, 0x25, 0x414b_0001);
    assert!(state.take_input(1.0).unwrap().events.iter().any(|event| {
        matches!(
            event,
            Event::Key {
                key: Key::ArrowLeft,
                pressed: true,
                ..
            }
        )
    }));

    state.clear_capture();
    send(hwnd, control_message().unwrap(), 0, 0);
    assert!(current_context(hwnd).is_invalid());
    assert_eq!(input_mode(private), mode);
    assert_eq!(send(hwnd, WM_KEYDOWN, 0x57, 0x0011_0001), LRESULT(71));
    assert_eq!(
        send(hwnd, WM_KEYUP, 0x57, 0xc011_0001u32 as isize),
        LRESULT(71)
    );
    assert_eq!(HOST_KEY.load(Ordering::Relaxed), 4);
    assert_eq!(send(hwnd, WM_IME_CHAR, 0x4f60, 0), LRESULT(0));
    assert_eq!(send(hwnd, WM_IME_COMPOSITION, 0, 0), LRESULT(0));
    assert_eq!(
        send(hwnd, WM_IME_NOTIFY, IMN_OPENCANDIDATE as usize, 1),
        LRESULT(0)
    );
    assert_eq!(HOST_IME_CHAR.load(Ordering::Relaxed), 0);
    assert_eq!(HOST_COMPOSITION.load(Ordering::Relaxed), 0);
    assert_eq!(HOST_IME_NOTIFY.load(Ordering::Relaxed), 0);

    activate_editor(&state, hwnd);
    assert_eq!(current_context(hwnd), private);
    assert_eq!(input_mode(private), mode);
    drop(binding);
    assert!(window_route().is_none());
    assert_eq!(current_context(hwnd), host_context);
    assert_eq!(
        unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) },
        host_window_proc
    );
    assert!(!capture.captures_keyboard());
    assert_eq!(send(hwnd, WM_CHAR, 0x4f60, 0), LRESULT(71));
    assert_eq!(HOST_CHAR.load(Ordering::Relaxed), 1);
    HOST_KEY.store(0, Ordering::Relaxed);
    host_owner_handoffs_reuse_context(hwnd, host_context, host_window_proc);
    let _ = unsafe { SetFocus((!previous_focus.is_invalid()).then_some(previous_focus)) };
}
