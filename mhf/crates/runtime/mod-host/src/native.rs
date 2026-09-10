use crate::{Context, Module, Result};
use std::path::Path;

#[cfg(any(windows, test))]
use {mhf_mod_api as api, std::ffi::c_void};

/// The library guard is dropped after destroy returns. Keeping the adapter
/// independent of the OS loader also lets lifecycle contracts be tested on CI.
#[cfg(any(windows, test))]
struct Native<L> {
    instance: *mut c_void,
    functions: &'static api::ModV2,
    _library: L,
}

#[cfg(any(windows, test))]
impl<L> Native<L> {
    fn call(&mut self, callback: Option<api::LifecycleFn>, context: &Context) -> Result<()> {
        context.clear_error();
        if let Some(callback) = callback {
            let status = unsafe { callback(self.instance) };
            if status != api::OK {
                return Err(format!("status {status}: {}", context.error()));
            }
        }
        Ok(())
    }
}

#[cfg(any(windows, test))]
impl<L> Module for Native<L> {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        self.call(self.functions.prepare, context)
    }
    fn check(&mut self, context: &Context) -> Result<()> {
        self.call(self.functions.check, context)
    }
    fn attach(&mut self, context: &Context) -> Result<()> {
        self.call(self.functions.attach, context)
    }
    fn stop(&mut self, context: &Context) -> Result<()> {
        self.call(self.functions.stop, context)
    }
    fn detach(&mut self, context: &Context) -> Result<()> {
        self.call(self.functions.detach, context)
    }
}

#[cfg(any(windows, test))]
impl<L> Drop for Native<L> {
    fn drop(&mut self) {
        unsafe {
            (self.functions.destroy)(self.instance);
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) unsafe fn from_table<L: 'static>(
    functions: &'static api::ModV2,
    context: &Context,
    library: L,
) -> Result<Box<dyn Module>> {
    context.clear_error();
    let mut instance = std::ptr::null_mut();
    let status = unsafe { (functions.create)(context.api(), &mut instance) };
    if status != api::OK {
        return Err(format!(
            "create failed with status {status}: {}",
            context.error()
        ));
    }
    Ok(Box::new(Native {
        instance,
        functions,
        _library: library,
    }))
}

#[cfg(windows)]
pub(crate) fn load(path: &Path, context: &Context) -> Result<Box<dyn Module>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{FreeLibrary, HMODULE},
            System::LibraryLoader::{
                GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
                LoadLibraryExW,
            },
        },
        core::{PCSTR, PCWSTR},
    };

    struct Library(HMODULE);
    impl Drop for Library {
        fn drop(&mut self) {
            unsafe {
                let _ = FreeLibrary(self.0);
            }
        }
    }

    let path = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let library = Library(
        unsafe {
            LoadLibraryExW(
                PCWSTR(wide.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }
        .map_err(|error| format!("failed to load {}: {error}", path.display()))?,
    );
    let address = unsafe { GetProcAddress(library.0, PCSTR(api::MOD_QUERY_SYMBOL.as_ptr())) }
        .ok_or_else(|| format!("{} does not export mhf_mod_query_v2", path.display()))?;
    let query: api::ModQueryV2 = unsafe { std::mem::transmute(address) };
    let functions = unsafe { query().as_ref() }.ok_or("Mod query returned no interface")?;
    unsafe { from_table(functions, context, library) }
}

#[cfg(not(windows))]
pub(crate) fn load(path: &Path, _context: &Context) -> Result<Box<dyn Module>> {
    Err(format!("native Mod {} requires Windows", path.display()))
}
