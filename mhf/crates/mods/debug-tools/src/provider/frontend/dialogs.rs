use super::WindowState;
use std::{
    cell::RefCell,
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::{
            Shell::{
                Common::COMDLG_FILTERSPEC, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
                FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_STRICTFILETYPES, FileSaveDialog,
                IFileSaveDialog, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::{KillTimer, SetTimer},
        },
    },
    core::{HRESULT, w},
};

pub(super) fn save_recording(
    owner: HWND,
    json: &str,
    window: Arc<WindowState>,
) -> Result<bool, String> {
    let path = save_path(owner, window).map_err(|error| format!("无法选择保存路径：{error}"))?;
    write_recording(path, json)
}

fn write_recording(path: Option<PathBuf>, json: &str) -> Result<bool, String> {
    let Some(path) = path else {
        return Ok(false);
    };
    std::fs::write(path, json).map_err(|error| format!("保存录制失败：{error}"))?;
    Ok(true)
}

fn save_path(owner: HWND, window: Arc<WindowState>) -> windows::core::Result<Option<PathBuf>> {
    let _apartment = ComApartment::new()?;
    // This modal dialog belongs to the independent desktop UI thread. It never
    // owns the game window or holds a DebugControl lock while the user chooses.
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetTitle(w!("保存 AI 录制"))?;
        dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("AI 录制 (*.json)"),
            pszSpec: w!("*.json"),
        }])?;
        dialog.SetDefaultExtension(w!("json"))?;
        dialog.SetFileName(w!("mhf-ai-recording.json"))?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_OVERWRITEPROMPT
                | FOS_FORCEFILESYSTEM
                | FOS_NOCHANGEDIR
                | FOS_PATHMUSTEXIST
                | FOS_STRICTFILETYPES,
        )?;
        let _cancel_timer = CancelTimer::new(dialog.clone(), window)?;
        match dialog.Show(Some(owner)) {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        let item = dialog.GetResult()?;
        let raw_path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(OsString::from_wide(raw_path.as_wide()));
        CoTaskMemFree(Some(raw_path.0.cast()));
        Ok(Some(path))
    }
}

thread_local! {
    static ACTIVE_DIALOG: RefCell<Option<(IFileSaveDialog, Arc<WindowState>)>> = const { RefCell::new(None) };
}

struct CancelTimer(usize);

impl CancelTimer {
    fn new(dialog: IFileSaveDialog, window: Arc<WindowState>) -> windows::core::Result<Self> {
        // The modal message loop continues dispatching this UI-thread timer,
        // so F7 and shutdown can cancel through the COM dialog's own API.
        let timer = unsafe { SetTimer(None, 0, 33, Some(cancel_hidden_dialog)) };
        if timer == 0 {
            return Err(windows::core::Error::from_thread());
        }
        ACTIVE_DIALOG.with_borrow_mut(|active| *active = Some((dialog, window)));
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
    let dialog = ACTIVE_DIALOG.with_borrow(|active| {
        active.as_ref().and_then(|(dialog, window)| {
            (window.stopping.load(Ordering::Acquire) || !window.visible.load(Ordering::Acquire))
                .then(|| dialog.clone())
        })
    });
    if let Some(dialog) = dialog {
        // Release the TLS borrow before invoking COM, which can dispatch messages.
        let _ = unsafe { dialog.Close(HRESULT::from_win32(ERROR_CANCELLED.0)) };
    }
}

struct ComApartment;

impl ComApartment {
    fn new() -> windows::core::Result<Self> {
        // Both S_OK and S_FALSE need a matching CoUninitialize on this thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_recording_path_writes_the_json_and_reports_io_errors() {
        let path = std::env::temp_dir().join(format!("mhf-录制-{}.json", std::process::id()));
        let recording = mhf_ai_debug::Recording::empty(mhf_ai_debug::Snapshot::default());
        let json = recording.to_json().unwrap();
        std::fs::write(&path, "previous contents longer than the recording").unwrap();
        assert!(write_recording(Some(path.clone()), &json).unwrap());
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved, json);
        mhf_ai_debug::ReplaySession::new(mhf_ai_debug::Recording::from_json(&saved).unwrap())
            .unwrap();
        let error = write_recording(Some(path.join("unavailable.json")), &json).unwrap_err();
        assert!(error.starts_with("保存录制失败："));
        std::fs::remove_file(path).unwrap();
    }
}
