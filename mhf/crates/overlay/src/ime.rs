//! One IMM context per window, shared by the overlay and an optional host editor.
//! Native operations and host callbacks run on the window thread, outside locks.

mod caret;
mod context;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use egui::{Id, ImeEvent, Rect, output::IMEOutput};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::Ime::{
    GCS_COMPATTR, GCS_COMPCLAUSE, GCS_COMPSTR, GCS_CURSORPOS, GCS_RESULTSTR, ISC_SHOWUIALL,
    ISC_SHOWUICOMPOSITIONWINDOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, PostMessageW, RegisterWindowMessageW, WM_IME_CHAR, WM_IME_COMPOSITION,
    WM_IME_COMPOSITIONFULL, WM_IME_CONTROL, WM_IME_ENDCOMPOSITION, WM_IME_KEYDOWN, WM_IME_KEYUP,
    WM_IME_NOTIFY, WM_IME_REQUEST, WM_IME_SELECT, WM_IME_SETCONTEXT, WM_IME_STARTCOMPOSITION,
};
use windows::core::w;

use crate::input::decode_character;
use crate::{Error, Result};
use caret::Caret;
use context::NativeContext;

/// An editable control in the host application. Its identity must change when
/// another control becomes active, even if the cursor occupies the same pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostImeTarget {
    pub id: usize,
    /// Cursor rectangle in the host window's client-pixel coordinates.
    pub cursor_rect: Rect,
}

/// Connects a native host editor to the Unicode text input and IME used by egui.
///
/// Both callbacks run synchronously on the window thread, without an overlay
/// lock. They must not uninstall the overlay or destroy its window. The adapter
/// owns the host's editor identities, text encoding, buffers, and drawing.
pub trait HostIme: Send + Sync + 'static {
    fn target(&self) -> Option<HostImeTarget>;

    /// Commit carries both ordinary WM_CHAR input and completed IME text.
    /// The shared context is still associated during the callback, so adapters
    /// can inspect composition metadata without reading text through an ANSI API.
    /// Cancellation goes to the previous identity before changing the owner.
    fn event(&self, id: usize, event: &ImeEvent);
}

static CONTROL_MESSAGE: OnceLock<u32> = OnceLock::new();

pub(super) fn control_message() -> Result<u32> {
    let message = *CONTROL_MESSAGE
        .get_or_init(|| unsafe { RegisterWindowMessageW(w!("ShrimpmanOverlay.IME.Control")) });
    if message == 0 {
        Err(Error::new("RegisterWindowMessageW(IME) failed"))
    } else {
        Ok(message)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Overlay(Option<Id>),
    Host(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    owner: Owner,
    cursor: Rect,
}

#[derive(Default)]
struct State {
    requested: Option<Target>,
    block_host: bool,
    applied: Option<Target>,
    /// Retained while the broker owns native input or blocks host input.
    native: Option<NativeContext>,
    caret: Caret,
    owns_messages: bool,
    composing: bool,
    pending_high_surrogate: Option<u16>,
    ended: bool,
    interrupt: bool,
    queued: bool,
    syncing: bool,
    closing: bool,
}

#[derive(Default)]
pub(super) struct Ime {
    state: Mutex<State>,
    host: Option<Arc<dyn HostIme>>,
}

pub(super) struct Message {
    /// Host events are delivered synchronously; these events belong to egui.
    pub events: Vec<ImeEvent>,
    /// Let the system IME handle this message, bypassing the host procedure.
    pub default_proc: bool,
    pub lparam: LPARAM,
}

impl Ime {
    pub(super) fn new(host: Option<Arc<dyn HostIme>>) -> Self {
        Self {
            host,
            ..Self::default()
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn update(
        &self,
        hwnd: HWND,
        widget: Option<Id>,
        output: Option<IMEOutput>,
        pixels_per_point: f32,
        block_host: bool,
    ) {
        let output = output.filter(|_| block_host);
        let target = output.map(|output| Target {
            owner: Owner::Overlay(widget),
            cursor: output.cursor_rect * pixels_per_point,
        });
        let should_post = {
            let mut state = self.lock();
            if state.closing {
                return;
            }
            state.interrupt |= output.is_some_and(|output| output.should_interrupt_composition);
            let changed = state.requested != target
                || state.applied != target
                || state.block_host != block_host
                || (state.native.is_none() && (self.host.is_some() || block_host))
                || state.interrupt;
            state.requested = target;
            state.block_host = block_host;
            // Native focus can change during a game frame without a window
            // message. Coalesce one refresh with the latest overlay request.
            if (changed || self.host.is_some()) && !state.queued {
                state.queued = true;
                true
            } else {
                false
            }
        };
        if !should_post {
            return;
        }
        let posted = control_message().is_ok_and(|message| unsafe {
            PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0)).is_ok()
        });
        if !posted {
            self.lock().queued = false;
        }
    }

    pub(super) fn composing(&self) -> bool {
        let state = self.lock();
        state.owns_messages && state.composing
    }

    pub(super) fn sync(&self, hwnd: HWND) -> (Vec<ImeEvent>, bool) {
        self.synchronize(hwnd, false, true)
    }

    /// Nested IMM callbacks observe transitional ownership without repeating
    /// association, cancellation, or destruction of the context.
    pub(super) fn refresh(&self, hwnd: HWND) -> (Vec<ImeEvent>, bool) {
        self.synchronize(hwnd, false, false)
    }

    fn synchronize(
        &self,
        hwnd: HWND,
        force_none: bool,
        consume_message: bool,
    ) -> (Vec<ImeEvent>, bool) {
        {
            let mut state = self.lock();
            if consume_message {
                state.queued = false;
            }
            if state.syncing {
                return (Vec::new(), false);
            }
            state.syncing = true;
        }
        struct FinishSync<'a>(&'a Ime);
        impl Drop for FinishSync<'_> {
            fn drop(&mut self) {
                self.0.lock().syncing = false;
            }
        }
        let _finish = FinishSync(self);
        self.sync_inner(hwnd, force_none)
    }

    fn sync_inner(&self, hwnd: HWND, force_none: bool) -> (Vec<ImeEvent>, bool) {
        let focused = unsafe { GetFocus() } == hwnd;
        let (requested, block_host, previous, mut native, interrupt, closing) = {
            let mut state = self.lock();
            (
                state.requested,
                state.block_host,
                state.applied,
                state.native.clone(),
                std::mem::take(&mut state.interrupt),
                state.closing,
            )
        };
        let target = if force_none || closing || !focused {
            None
        } else if block_host {
            requested
        } else {
            self.host
                .as_ref()
                .and_then(|host| host.target())
                .map(|target| Target {
                    owner: Owner::Host(target.id),
                    cursor: target.cursor_rect,
                })
        };
        let release_context = closing || (self.host.is_none() && !block_host);
        let changed_owner =
            target.map(|target| target.owner) != previous.map(|target| target.owner);
        if changed_owner || interrupt {
            self.lock().pending_high_surrogate = None;
        }
        let mut events = Vec::new();
        let mut caret = self.lock().caret.clone();
        if let Some(previous) = previous
            && (changed_owner || interrupt)
        {
            {
                let mut state = self.lock();
                // Keep routing IME messages while cancelling, but detach their
                // recipient before IMM can synchronously deliver a final result.
                state.applied = None;
                state.composing = false;
            }
            if let Some(native) = &native {
                native.cancel();
            }
            self.emit(previous.owner, clear_preedit(), &mut events);
        }

        if !release_context {
            // A host adapter owns idle input too; blocking overlays suspend it.
            // Claim late IME messages before native association can reenter us.
            self.lock().owns_messages = true;
            if native.is_none() {
                native = NativeContext::new(hwnd);
            }
        }

        let Some(target) = target else {
            let restored = if release_context {
                let restored = native.as_mut().is_some_and(|native| native.restore(hwnd));
                if let Some(native) = native.take() {
                    native.destroy(hwnd);
                }
                restored
            } else {
                if let Some(native) = &native {
                    native.suspend(hwnd);
                }
                false
            };
            caret.restore();
            let mut state = self.lock();
            state.native = native;
            state.caret = caret;
            state.applied = None;
            state.owns_messages = !release_context;
            state.composing = false;
            state.pending_high_surrogate = None;
            return (events, restored);
        };

        let Some(mut native) = native else {
            return (events, false);
        };
        // Wine's macOS driver anchors native candidates through GUITHREADINFO,
        // so publish a system caret before activating the input context.
        caret.update(hwnd, target.cursor);
        let entering = !native.is_associated(hwnd);
        if entering {
            self.lock().owns_messages = true;
            if !native.associate(hwnd) {
                native.suspend(hwnd);
                caret.restore();
                let mut state = self.lock();
                state.native = Some(native);
                state.caret = caret;
                state.applied = None;
                return (events, false);
            }
        }
        {
            let mut state = self.lock();
            if entering || interrupt || changed_owner {
                state.ended = false;
            }
            state.native = Some(native.clone());
            state.caret = caret;
            state.applied = Some(target);
            state.owns_messages = true;
        }
        if entering || previous != Some(target) {
            native.set_cursor(target.cursor);
        }
        if entering {
            // Both editors draw preedit; the system IME draws candidates.
            unsafe {
                DefWindowProcW(
                    hwnd,
                    WM_IME_SETCONTEXT,
                    WPARAM(1),
                    LPARAM((ISC_SHOWUIALL & !ISC_SHOWUICOMPOSITIONWINDOW) as isize),
                );
            }
        }
        (events, false)
    }

    pub(super) fn stop(&self, hwnd: HWND, closing: bool) -> (Vec<ImeEvent>, bool) {
        {
            let mut state = self.lock();
            state.requested = None;
            state.closing |= closing;
        }
        self.synchronize(hwnd, true, true)
    }

    fn emit(&self, owner: Owner, event: ImeEvent, egui_events: &mut Vec<ImeEvent>) {
        match owner {
            Owner::Overlay(_) => egui_events.push(event),
            Owner::Host(id) => {
                if let Some(host) = &self.host {
                    host.event(id, &event);
                }
            }
        }
    }

    pub(super) fn handle_character(&self, code_unit: usize) -> bool {
        let (id, character) = {
            let mut state = self.lock();
            let Some(Target {
                owner: Owner::Host(id),
                ..
            }) = state.applied
            else {
                return false;
            };
            if state.composing {
                state.pending_high_surrogate = None;
                return true;
            }
            let character = decode_character(&mut state.pending_high_surrogate, code_unit);
            if character.is_some_and(char::is_control) {
                // Enter, Backspace and shortcuts retain their native key path.
                return false;
            }
            (id, character)
        };
        if let Some(character) = character
            && let Some(host) = &self.host
        {
            host.event(id, &ImeEvent::Commit(character.to_string()));
        }
        // Consume incomplete/malformed pairs too: forwarding either code unit
        // through an ANSI WindowProc would turn it into an unrelated byte.
        true
    }

    pub(super) fn handle_message(&self, message: u32, lparam: LPARAM) -> Option<Message> {
        let (native, owner, composing, ended) = {
            let mut state = self.lock();
            if !state.owns_messages {
                return None;
            }
            if matches!(
                message,
                WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION
            ) {
                state.pending_high_surrogate = None;
            }
            (
                state.native.clone(),
                state.applied.map(|target| target.owner),
                state.composing,
                state.ended,
            )
        };
        let mut reply = Message {
            events: Vec::new(),
            default_proc: false,
            lparam,
        };
        match message {
            WM_IME_SETCONTEXT => {
                reply.lparam.0 &= if owner.is_some() {
                    !(ISC_SHOWUICOMPOSITIONWINDOW as isize)
                } else {
                    !(ISC_SHOWUIALL as isize)
                };
                reply.default_proc = true;
            }
            WM_IME_STARTCOMPOSITION => {
                if owner.is_some() {
                    let mut state = self.lock();
                    state.composing = true;
                    state.ended = false;
                    reply.default_proc = true;
                }
            }
            WM_IME_COMPOSITION if !ended => {
                if let (Some(native), Some(owner)) = (native, owner) {
                    let flags = lparam.0 as u32;
                    if flags == 0 {
                        self.lock().composing = false;
                        self.emit(owner, clear_preedit(), &mut reply.events);
                    }
                    // A message may commit one clause and start another.
                    if flags & GCS_RESULTSTR.0 != 0
                        && let Some(text) = native.read_string(GCS_RESULTSTR)
                    {
                        self.lock().composing = false;
                        self.emit(owner, ImeEvent::Commit(text), &mut reply.events);
                    }
                    if flags & (GCS_COMPSTR.0 | GCS_COMPATTR.0 | GCS_COMPCLAUSE.0 | GCS_CURSORPOS.0)
                        != 0
                        && let Some(preedit) = native.preedit()
                    {
                        self.lock().composing =
                            matches!(&preedit, ImeEvent::Preedit { text, .. } if !text.is_empty());
                        self.emit(owner, preedit, &mut reply.events);
                    }
                }
            }
            WM_IME_ENDCOMPOSITION => {
                if let Some(owner) = owner {
                    // Hangul may expose its result at END before a late COMPOSITION.
                    if composing
                        && let Some(text) =
                            native.and_then(|native| native.read_string(GCS_RESULTSTR))
                        && !text.is_empty()
                    {
                        self.emit(owner, ImeEvent::Commit(text), &mut reply.events);
                    }
                    self.emit(owner, clear_preedit(), &mut reply.events);
                    reply.default_proc = true;
                }
                let mut state = self.lock();
                state.composing = false;
                state.ended = true;
            }
            // Do not let the default procedure emit duplicate character events.
            WM_IME_COMPOSITION | WM_IME_CHAR | WM_IME_REQUEST => {}
            WM_IME_NOTIFY
            | WM_IME_CONTROL
            | WM_IME_SELECT
            | WM_IME_COMPOSITIONFULL
            | WM_IME_KEYDOWN
            | WM_IME_KEYUP => {
                if owner.is_some() {
                    reply.default_proc = true;
                }
            }
            _ => return None,
        }
        Some(reply)
    }
}

fn clear_preedit() -> ImeEvent {
    ImeEvent::Preedit {
        text: String::new(),
        active_range_chars: None,
    }
}
