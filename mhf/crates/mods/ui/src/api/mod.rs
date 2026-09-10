//! Panels and borrowed UI operations backed by generated C vtables.

use mhf_mod_sdk::{
    Dependencies, Result, abi as api,
    error::status_result,
    interface::{Interface, InterfaceRef, bind},
};
use safer_ffi::prelude::{VirtualPtr, derive_ReprC, str};
use std::{
    ffi::c_void,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    rc::Rc,
};

pub const PROVIDER_ID: &str = "mhf.base";
pub const INTERFACE_ID: &str = "mhf.ui.v1";

/// A serialized UI-thread callback. `user` has the meaning established at
/// registration; `ui` is non-null, aligned, and borrowed only until this
/// invocation returns. The `'static` spelling erases the ABI lifetime without
/// extending the table, object, or underlying UI. The callback must not
/// retain or release the table/object, use it on another thread, unregister its
/// own panel, or unwind across the C boundary.
pub type RenderFn =
    unsafe extern "C" fn(user: *mut c_void, ui: *const UiTable<'static>) -> api::Status;

/// UI operations for one active render invocation. Implementations retain no
/// caller references and never unwind through a generated C entry point. Calls
/// are confined to the current UI thread and do not reenter the same UI.
/// C callers must provide valid UTF-8 with a non-null pointer even when empty,
/// valid exclusive output references, and initialized `bool` values where read.
#[derive_ReprC(dyn)]
pub trait UiApi {
    fn label(&self, text: str::Ref<'_>) -> api::Status;
    /// `OK` makes the returned button state valid.
    fn button(&self, text: str::Ref<'_>, out: &mut bool) -> api::Status;
    /// `value` is read as the current state and updated on `OK`.
    fn checkbox(&self, text: str::Ref<'_>, value: &mut bool) -> api::Status;
}

/// An immutable borrow for one render invocation, including when a C callback
/// receives `UiTable<'static>`. Consumers must not retain, move, overwrite, free,
/// byte-copy as an owner, or invoke `vtable.release_vptr`. The table, virtual
/// object, and underlying UI expire when the callback returns.
pub type UiTable<'ui> = VirtualPtr<dyn UiApi + 'ui>;

/// Panel registration on the host lifecycle thread. Implementations copy titles
/// they retain, serialize each panel's render calls on the UI thread, honor
/// synchronous unregister, and never unwind through a generated C entry point.
/// This interface is not available for concurrent lifecycle calls.
#[derive_ReprC(dyn)]
pub trait UiHostApi {
    /// Only `OK` makes the returned handle valid. C callers must provide valid
    /// UTF-8 with a non-null pointer even for an empty title, and a valid
    /// exclusive output reference.
    ///
    /// # Safety
    /// Keep the table, object, and provider code live. `render`, its DLL, and
    /// `user` state must be ready before registration and remain valid until
    /// successful synchronous unregister or completed provider shutdown. They
    /// must support serialized UI-thread calls without unwinding; `user` may be
    /// null only if `render` supports it. Failure must leave no registered or
    /// in-flight callback and retain neither callback nor state pointer.
    unsafe fn register_panel(
        &self,
        title: str::Ref<'_>,
        render: RenderFn,
        user: *mut c_void,
        out: &mut u64,
    ) -> api::Status;
    /// Call on the host lifecycle thread, never from this panel's render
    /// callback (which would wait for itself). `OK` removes the panel and waits
    /// for active callbacks; afterwards none may start. On failure, callbacks
    /// may remain active or start later: retain their state/code and propagate
    /// cleanup failure so the host keeps the consumer loaded.
    fn unregister_panel(&self, handle: u64) -> api::Status;
}

/// The provider owns this virtual object. Host lookup lends its immutable table
/// through consumer destruction; consumers must not move, free, overwrite,
/// byte-copy as an owner, or call its `vtable.release_vptr`. A pointer copy does
/// not keep the provider DLL loaded. All access must end before provider release.
pub type UiHostTable = VirtualPtr<dyn UiHostApi>;

pub enum UiHostInterface {}

// SAFETY: The provider publishes this table through consumer destruction and
// serializes each panel's callbacks. UiTable lives only for the render invocation.
// Successful unregister synchronously ends all uses of the callback and state.
unsafe impl Interface for UiHostInterface {
    type Table = UiHostTable;
    const PROVIDER: &'static str = PROVIDER_ID;
    const ID: &'static str = INTERFACE_ID;
}

#[repr(transparent)]
pub struct UiHost<'host> {
    binding: InterfaceRef<'host, UiHostInterface>,
}

impl<'host> UiHost<'host> {
    #[inline]
    pub fn bind(dependencies: Dependencies<'host>) -> Result<Self> {
        Ok(Self {
            binding: bind(dependencies)?,
        })
    }

    #[inline]
    pub fn panel<F>(&self, title: &str, render: F) -> Result<Panel<'host, F>>
    where
        F: FnMut(&Ui<'_>) + Send + 'static,
    {
        Panel::register(self.binding.table(), title, render)
    }
}

#[repr(transparent)]
pub struct Ui<'ui> {
    raw: &'ui UiTable<'static>,
    _thread: PhantomData<Rc<()>>,
}

impl<'ui> Ui<'ui> {
    #[inline]
    pub(crate) fn from_table<'provider: 'ui>(raw: &'ui UiTable<'provider>) -> Self {
        Self {
            // The C view erases the object's inner lifetime. This wrapper only
            // borrows the table for 'ui, cannot clone/release its virtual object,
            // and exposes no operation that extends the provider borrow.
            raw: unsafe { &*(raw as *const UiTable<'provider>).cast::<UiTable<'static>>() },
            _thread: PhantomData,
        }
    }

    #[inline]
    pub fn label(&self, text: &str) -> Result<()> {
        status_result(self.raw.label(text.into()), "UI label")
    }

    #[inline]
    pub fn button(&self, text: &str) -> Result<bool> {
        let mut clicked = false;
        status_result(self.raw.button(text.into(), &mut clicked), "UI button")?;
        Ok(clicked)
    }

    #[inline]
    pub fn checkbox(&self, text: &str, value: &mut bool) -> Result<()> {
        let mut output = *value;
        status_result(self.raw.checkbox(text.into(), &mut output), "UI checkbox")?;
        *value = output;
        Ok(())
    }
}

/// Call `close` from Mod::stop and propagate failure so the host keeps the mod
/// and its DLL resident. The callback uses one stable allocation and a
/// monomorphized C adapter; no trait object is involved.
///
/// Drop also unregisters. If it fails, callback storage is retained because the
/// provider may still invoke it. Provider shutdown must finish all callbacks
/// before any consumer DLL is unloaded.
pub struct Panel<'host, F> {
    table: &'host UiHostTable,
    handle: u64,
    // An owning raw pointer avoids extending Box's unique borrow over render
    // calls. Reconstruct the Box only once the provider has stopped using it.
    callback: Option<NonNull<F>>,
    _thread: PhantomData<Rc<()>>,
}

impl<'host, F> Panel<'host, F> {
    fn register(table: &'host UiHostTable, title: &str, render: F) -> Result<Self>
    where
        F: FnMut(&Ui<'_>) + Send + 'static,
    {
        let callback = NonNull::new(Box::into_raw(Box::new(render))).unwrap();
        let mut handle = 0;
        let status = unsafe {
            table.register_panel(
                title.into(),
                render_panel::<F>,
                callback.as_ptr().cast(),
                &mut handle,
            )
        };
        // Failed registration must retain neither callback nor user pointer.
        if let Err(error) = status_result(status, "register UI panel") {
            // SAFETY: Failed registration retains neither the pointer nor an
            // in-flight callback. This is the allocation's only owner.
            unsafe { drop(Box::from_raw(callback.as_ptr())) };
            return Err(error);
        }
        Ok(Self {
            table,
            handle,
            callback: Some(callback),
            _thread: PhantomData,
        })
    }

    /// Retryable: a failed unregister keeps the callback and handle intact.
    #[inline]
    pub fn close(&mut self) -> Result<()> {
        if self.callback.is_none() {
            return Ok(());
        }
        status_result(
            self.table.unregister_panel(self.handle),
            "unregister UI panel",
        )?;
        if let Some(callback) = self.callback.take() {
            // SAFETY: Successful synchronous unregister ended every callback.
            // No Box has owned this allocation since it was published.
            unsafe { drop(Box::from_raw(callback.as_ptr())) };
        }
        Ok(())
    }
}

impl<F> Drop for Panel<'_, F> {
    fn drop(&mut self) {
        // A failed close intentionally leaves the allocation resident: dropping
        // NonNull does not free storage the provider may still call into.
        let _ = self.close();
    }
}

unsafe extern "C" fn render_panel<F>(user: *mut c_void, raw: *const UiTable<'static>) -> api::Status
where
    F: FnMut(&Ui<'_>) + Send + 'static,
{
    let render = unsafe { &mut *user.cast::<F>() };
    let ui = Ui::from_table(unsafe { &*raw });
    match catch_unwind(AssertUnwindSafe(|| render(&ui))) {
        Ok(()) => api::OK,
        Err(_) => api::ERROR,
    }
}

#[cfg(feature = "headers")]
pub fn define_header(definer: &mut dyn mhf_mod_sdk::abi::headers::Definer) -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    h::alias::<UiTable<'static>>(definer, "UiTable")?;
    h::alias::<RenderFn>(definer, "RenderFn")?;
    h::alias::<UiHostTable>(definer, "UiHostTable")?;
    h::string(definer, "MHF_UI_PROVIDER", PROVIDER_ID)?;
    h::string(definer, "MHF_UI_INTERFACE", INTERFACE_ID)?;
    Ok(())
}

#[cfg(test)]
mod tests;
