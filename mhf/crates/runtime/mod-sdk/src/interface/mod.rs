use crate::{Dependencies, Result};
use crate::{abi as api, host::check};

/// Provider-owned mapping from a public interface ID to its fixed table layout.
///
/// # Safety
/// ID must identify exactly Table as implemented by PROVIDER. Successful binding
/// must provide an immutable reference valid through consumer destruction. The
/// provider wrapper must honor its callback, ownership and threading contracts.
pub unsafe trait Interface {
    type Table: safer_ffi::layout::ReprC;
    const PROVIDER: &'static str;
    const ID: &'static str;
}

#[inline]
pub fn bind<'host, I: Interface>(
    dependencies: Dependencies<'host>,
) -> Result<InterfaceRef<'host, I>> {
    let host = dependencies.host;
    let mut table = std::ptr::null();
    let status = unsafe {
        (host.raw.dependency)(
            host.raw.context,
            api::Str::new(I::PROVIDER),
            api::Str::new(I::ID),
            &mut table,
        )
    };
    check(host, status)?;
    Ok(InterfaceRef {
        table: unsafe { &*table.cast::<I::Table>() },
    })
}

/// A borrowed provider binding. Its Send/Sync behavior follows the table itself,
/// including the generated virtual trait's declared threading bounds.
/// Dropping this reference never releases the table or its virtual object; the
/// provider retains ownership through consumer destruction.
#[repr(transparent)]
pub struct InterfaceRef<'host, I: Interface> {
    table: &'host I::Table,
}

impl<'host, I: Interface> InterfaceRef<'host, I> {
    #[inline]
    pub fn table(&self) -> &'host I::Table {
        self.table
    }
}
