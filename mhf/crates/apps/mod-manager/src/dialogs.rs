#[cfg(windows)]
pub(crate) use native::{export_archive, import_archive};

#[cfg(not(windows))]
pub(crate) use unsupported::{export_archive, import_archive};

#[cfg(windows)]
mod native {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        Win32::{
            Foundation::ERROR_CANCELLED,
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
                FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_STRICTFILETYPES, FileOpenDialog,
                FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog, SIGDN_FILESYSPATH,
            },
        },
        core::{HRESULT, PCWSTR, w},
    };

    pub(crate) fn import_archive() -> Result<Option<PathBuf>, String> {
        open_file(w!("导入 Mod 整合包"), w!("ZIP 整合包"), w!("*.zip"))
            .map_err(|error| format!("无法选择整合包：{error}"))
    }

    pub(crate) fn export_archive() -> Result<Option<PathBuf>, String> {
        let pick = || -> windows::core::Result<Option<PathBuf>> {
            let _apartment = ComApartment::new()?;
            // Archives use create_new; existing exports must keep their contents.
            unsafe {
                let dialog: IFileSaveDialog =
                    CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
                dialog.SetTitle(w!("导出 Mod 整合包（请使用新文件名）"))?;
                dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                    pszName: w!("ZIP 整合包"),
                    pszSpec: w!("*.zip"),
                }])?;
                dialog.SetDefaultExtension(w!("zip"))?;
                dialog.SetFileName(w!("mods.zip"))?;
                dialog.SetOptions(
                    (dialog.GetOptions()? & !FOS_OVERWRITEPROMPT)
                        | FOS_FORCEFILESYSTEM
                        | FOS_NOCHANGEDIR
                        | FOS_PATHMUSTEXIST
                        | FOS_STRICTFILETYPES,
                )?;
                selected_path(&dialog)
            }
        };
        pick().map_err(|error| format!("无法选择导出路径：{error}"))
    }

    fn open_file(
        title: PCWSTR,
        filter_name: PCWSTR,
        pattern: PCWSTR,
    ) -> windows::core::Result<Option<PathBuf>> {
        let _apartment = ComApartment::new()?;
        // Callers provide static UTF-16 strings, valid for the modal dialog's lifetime.
        unsafe {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetTitle(title)?;
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: filter_name,
                pszSpec: pattern,
            }])?;
            dialog.SetOptions(
                dialog.GetOptions()?
                    | FOS_FORCEFILESYSTEM
                    | FOS_NOCHANGEDIR
                    | FOS_FILEMUSTEXIST
                    | FOS_PATHMUSTEXIST
                    | FOS_STRICTFILETYPES,
            )?;
            selected_path(&dialog)
        }
    }

    fn selected_path(dialog: &IFileDialog) -> windows::core::Result<Option<PathBuf>> {
        // Only filesystem results are requested; copy their UTF-16 path before freeing it.
        unsafe {
            match dialog.Show(None) {
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

    struct ComApartment;

    impl ComApartment {
        fn new() -> windows::core::Result<Self> {
            // Both S_OK and S_FALSE require a matching CoUninitialize on this thread.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            // This guard stays in its creating function, on the GUI thread.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(not(windows))]
mod unsupported {
    use std::path::PathBuf;

    pub(crate) fn import_archive() -> Result<Option<PathBuf>, String> {
        Err("当前平台不支持原生文件选择器，请输入整合包路径。".to_owned())
    }

    pub(crate) fn export_archive() -> Result<Option<PathBuf>, String> {
        Err("当前平台不支持原生文件选择器，请输入导出路径。".to_owned())
    }
}
