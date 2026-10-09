use super::WindowState;
use std::{
    cell::RefCell,
    sync::{Arc, atomic::Ordering},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{
        GA_ROOTOWNER, GetAncestor, GetClassNameW, GetLastActivePopup, IDCANCEL, KillTimer,
        PostMessageW, SetTimer, WM_COMMAND,
    },
};

pub(super) fn save_recording(
    parent: &eframe::Frame,
    owner: HWND,
    json: &str,
    window: &Arc<WindowState>,
) -> Result<bool, String> {
    // Keep the dialog on the desktop UI thread and owned by its window, so it
    // never blocks the game window or holds a DebugControl lock.
    let _cancel_timer = CancelTimer::new(owner, Arc::clone(window))?;
    let Some(path) = rfd::FileDialog::new()
        .set_parent(parent)
        .set_title("保存 AI 录制")
        .add_filter("AI 录制 (*.json)", &["json"])
        .set_file_name("mhf-ai-recording.json")
        .save_file()
    else {
        return Ok(false);
    };
    if window.stopping.load(Ordering::Acquire) || !window.visible.load(Ordering::Acquire) {
        return Ok(false);
    }
    std::fs::write(path, json).map_err(|error| format!("保存录制失败：{error}"))?;
    Ok(true)
}

thread_local! {
    static ACTIVE_DIALOG: RefCell<Option<(HWND, Arc<WindowState>)>> = const { RefCell::new(None) };
}

struct CancelTimer(usize);

impl CancelTimer {
    fn new(owner: HWND, window: Arc<WindowState>) -> Result<Self, String> {
        // rfd exposes no cancellation handle. Its modal message loop still
        // dispatches this UI-thread timer, allowing F7 and shutdown to close
        // only the native dialog owned by this desktop window.
        let timer = unsafe { SetTimer(None, 0, 33, Some(cancel_hidden_dialog)) };
        if timer == 0 {
            return Err(format!(
                "无法监控保存对话框：{}",
                windows::core::Error::from_thread()
            ));
        }
        ACTIVE_DIALOG.with_borrow_mut(|active| *active = Some((owner, window)));
        Ok(Self(timer))
    }
}

impl Drop for CancelTimer {
    fn drop(&mut self) {
        let _ = unsafe { KillTimer(None, self.0) };
        ACTIVE_DIALOG.with_borrow_mut(|active| *active = None);
    }
}

unsafe extern "system" fn cancel_hidden_dialog(
    _window: HWND,
    _message: u32,
    _id: usize,
    _time: u32,
) {
    let owner = ACTIVE_DIALOG.with_borrow(|active| {
        active.as_ref().and_then(|(owner, window)| {
            (window.stopping.load(Ordering::Acquire) || !window.visible.load(Ordering::Acquire))
                .then_some(*owner)
        })
    });
    if let Some(owner) = owner {
        let mut class = [0_u16; 16];
        unsafe {
            let popup = GetLastActivePopup(owner);
            let length = GetClassNameW(popup, &mut class) as usize;
            if popup != owner
                && class[..length] == *windows::core::w!("#32770").as_wide()
                && GetAncestor(popup, GA_ROOTOWNER) == owner
            {
                // IDCANCEL follows the same path as the dialog's Cancel button.
                let _ = PostMessageW(
                    Some(popup),
                    WM_COMMAND,
                    WPARAM(IDCANCEL.0 as usize),
                    LPARAM(0),
                );
            }
        }
    }
}
