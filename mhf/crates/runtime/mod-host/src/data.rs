//! The public `mhf.data.v1` interface, automatically provided by data packages.
//! Its independent C definition is in `include/mhf_data.h`.

use crate::{Context, Module, Result, api, context::copy_bytes};
use std::{
    ffi::c_void,
    fs,
    path::{Component, Path, PathBuf},
};

pub const INTERFACE_ID: &str = "mhf.data.v1";

/// Read-only package resources. The table and context remain valid through the
/// consumer's destroy. Output buffers are supplied and owned by the caller.
#[safer_ffi::derive_ReprC]
#[repr(C)]
pub struct DataV1 {
    pub context: *mut c_void,
    pub resource_root: api::ReadTextFn,
    pub read_file: unsafe extern "C" fn(
        context: *mut c_void,
        relative_path: api::Str,
        buffer: *mut u8,
        capacity: u32,
        required: *mut u32,
    ) -> api::Status,
}

pub(crate) struct DataMod {
    table: DataV1,
    root: PathBuf,
}

impl DataMod {
    pub(crate) fn new(root: PathBuf) -> Box<Self> {
        let mut data = Box::new(Self {
            table: DataV1 {
                context: std::ptr::null_mut(),
                resource_root,
                read_file,
            },
            root,
        });
        data.table.context = (&mut *data as *mut Self).cast();
        data
    }
}

impl Module for DataMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        unsafe { context.register(INTERFACE_ID, (&self.table as *const DataV1).cast()) }
    }
}

unsafe extern "C" fn resource_root(
    pointer: *mut c_void,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    let data = unsafe { &*pointer.cast::<DataMod>() };
    unsafe {
        copy_bytes(
            data.root.to_string_lossy().as_bytes(),
            buffer,
            capacity,
            required,
        )
    }
}

unsafe extern "C" fn read_file(
    pointer: *mut c_void,
    relative: api::Str,
    buffer: *mut u8,
    capacity: u32,
    required: *mut u32,
) -> api::Status {
    unsafe {
        required.write(0);
    }
    let data = unsafe { &*pointer.cast::<DataMod>() };
    let relative = unsafe { relative.as_str() };
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains(['\\', ':'])
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return api::ERROR;
    }
    let path = match data.root.join(path).canonicalize() {
        Ok(path) if path.starts_with(&data.root) => path,
        Ok(_) => return api::ERROR,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return api::NOT_FOUND,
        Err(_) => return api::ERROR,
    };
    match fs::read(path) {
        Ok(bytes) => unsafe { copy_bytes(&bytes, buffer, capacity, required) },
        Err(_) => api::ERROR,
    }
}

#[cfg(feature = "headers")]
pub fn generate_header(path: &std::path::Path) -> std::io::Result<()> {
    use mhf_mod_api::headers as h;
    h::write(
        path,
        "MHF_DATA_H",
        Some("mhf_mod.h"),
        h::definitions()?,
        |d| {
            h::alias::<DataV1>(d, "MhfDataV1")?;
            h::string(d, "MHF_DATA_INTERFACE_ID", INTERFACE_ID)
        },
    )
}
