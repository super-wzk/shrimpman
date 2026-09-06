use egui::Rect;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateCaret, DestroyCaret, GUITHREADINFO, GetGUIThreadInfo, SetCaretPos,
};

/// Wine's macOS driver positions candidates from the thread's Win32 caret.
/// All native operations run on the window thread; the UI draws its own caret.
#[derive(Default, Clone)]
pub(super) struct Caret {
    active: Option<ActiveCaret>,
}

#[derive(Clone)]
struct ActiveCaret {
    hwnd: usize,
    expected: RECT,
    /// Some means we borrowed a host caret and must preserve its shape.
    original_position: Option<POINT>,
}

impl ActiveCaret {
    fn matches(&self, info: &GUITHREADINFO) -> bool {
        info.hwndCaret.0 as usize == self.hwnd && info.rcCaret == self.expected
    }

    fn size(&self) -> (i32, i32) {
        (
            self.expected.right.saturating_sub(self.expected.left),
            self.expected.bottom.saturating_sub(self.expected.top),
        )
    }
}

impl Caret {
    pub(super) fn update(&mut self, hwnd: HWND, rect: Rect) {
        if !rect.is_finite() {
            return;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.hwnd != hwnd.0 as usize)
        {
            self.restore();
            if self.active.is_some() {
                return;
            }
        }
        let Some(info) = thread_info() else {
            return;
        };
        if self
            .active
            .as_ref()
            .is_some_and(|active| !active.matches(&info))
        {
            // The host moved, replaced, or destroyed the previous caret.
            self.active = None;
        }

        let x = rect.min.x.floor() as i32;
        let y = rect.min.y.floor() as i32;
        let width = (rect.max.x.ceil() as i32).saturating_sub(x).max(1);
        let height = (rect.max.y.ceil() as i32).saturating_sub(y).max(1);
        let create = if let Some(active) = &self.active {
            active.original_position.is_none() && active.size() != (width, height)
        } else if info.hwndCaret.is_invalid() {
            true
        } else if info.hwndCaret == hwnd {
            self.active = Some(ActiveCaret {
                hwnd: hwnd.0 as usize,
                expected: info.rcCaret,
                original_position: Some(POINT {
                    x: info.rcCaret.left,
                    y: info.rcCaret.top,
                }),
            });
            false
        } else {
            // CreateCaret would destroy another window's caret on this queue.
            return;
        };

        if create {
            if unsafe { CreateCaret(hwnd, None, width, height) }.is_err() {
                return;
            }
            // CreateCaret starts hidden. Never call ShowCaret for this helper.
            let expected = thread_info().filter(|info| info.hwndCaret == hwnd).map_or(
                RECT {
                    right: width,
                    bottom: height,
                    ..RECT::default()
                },
                |info| info.rcCaret,
            );
            self.active = Some(ActiveCaret {
                hwnd: hwnd.0 as usize,
                expected,
                original_position: None,
            });
        }

        let Some(active) = &mut self.active else {
            return;
        };
        if active.expected.left == x && active.expected.top == y {
            return;
        }
        if unsafe { SetCaretPos(x, y) }.is_ok() {
            let (width, height) = active.size();
            active.expected = RECT {
                left: x,
                top: y,
                right: x.saturating_add(width),
                bottom: y.saturating_add(height),
            };
        }
    }

    pub(super) fn restore(&mut self) {
        let Some(active) = &self.active else {
            return;
        };
        let Some(info) = thread_info() else {
            return;
        };
        if !active.matches(&info) {
            self.active = None;
            return;
        }
        let restored = if let Some(position) = active.original_position {
            unsafe { SetCaretPos(position.x, position.y) }
        } else {
            unsafe { DestroyCaret() }
        };
        if restored.is_ok() {
            self.active = None;
        }
    }
}

fn thread_info() -> Option<GUITHREADINFO> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..GUITHREADINFO::default()
    };
    // Zero selects the foreground thread, which can differ during activation.
    unsafe { GetGUIThreadInfo(GetCurrentThreadId(), &mut info) }.ok()?;
    Some(info)
}
