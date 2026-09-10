use crate::{Context, ModHost, Result, api, with_owner};
use api::game::{GlobalData32, LaunchParams32};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

type Callback = dyn FnMut(&mut LaunchParams32, &mut GlobalData32) -> Result<bool>;

/// A built-in launch callback and its stable C interface. Registration retains
/// the callback in the Mod's context through destruction of its consumers.
pub struct LaunchProvider {
    state: Rc<State>,
    interface: &'static str,
}

pub(crate) struct State {
    api: api::LaunchApiV1,
    callback: RefCell<Box<Callback>>,
    host: Cell<*const api::HostV2>,
}

impl LaunchProvider {
    pub fn new(
        callback: impl FnMut(&mut LaunchParams32, &mut GlobalData32) -> Result<bool> + 'static,
    ) -> Self {
        Self::with_interface(callback, api::LAUNCH_INTERFACE_ID)
    }

    /// Used only when no selected Mod publishes an ordinary launch provider.
    pub fn fallback(
        callback: impl FnMut(&mut LaunchParams32, &mut GlobalData32) -> Result<bool> + 'static,
    ) -> Self {
        Self::with_interface(callback, api::FALLBACK_LAUNCH_INTERFACE_ID)
    }

    fn with_interface(
        callback: impl FnMut(&mut LaunchParams32, &mut GlobalData32) -> Result<bool> + 'static,
        interface: &'static str,
    ) -> Self {
        let state = Rc::new_cyclic(|state: &std::rc::Weak<State>| State {
            api: api::LaunchApiV1 {
                context: state.as_ptr().cast_mut().cast(),
                run,
            },
            callback: RefCell::new(Box::new(callback)),
            host: Cell::new(std::ptr::null()),
        });
        Self { state, interface }
    }

    /// Publish from the provider's prepare callback. One registration per handle.
    pub fn register(&self, context: &Context) -> Result<()> {
        if !self.state.host.get().is_null() {
            return Err("launch provider is already registered".into());
        }
        // The context retains the allocation containing the table and callback.
        // Publication remains transactional with the provider's prepare phase.
        unsafe {
            context.register(
                self.interface,
                (&self.state.api as *const api::LaunchApiV1).cast(),
            )?;
        }
        self.state.host.set(context.api());
        context
            .launch_providers
            .borrow_mut()
            .push(self.state.clone());
        Ok(())
    }
}

unsafe extern "C" fn run(context: *mut c_void, target: *mut api::LaunchTargetV1) -> api::Status {
    let state = unsafe { &*context.cast::<State>() };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let target = unsafe { target.as_ref() }.ok_or("launch target is null")?;
        let params = unsafe { target.params.as_mut() }.ok_or("launch parameters are null")?;
        let global = unsafe { target.global.as_mut() }.ok_or("launch global data is null")?;
        (state.callback.borrow_mut())(params, global)
    }));
    let message = match result {
        Ok(Ok(true)) => return api::OK,
        Ok(Ok(false)) => return api::CANCELLED,
        Ok(Err(error)) => error,
        Err(_) => "launch provider panicked".into(),
    };
    if let Some(host) = unsafe { state.host.get().as_ref() } {
        unsafe { (host.log)(host.context, api::LOG_ERROR, api::Str::new(&message)) };
    }
    api::ERROR
}

impl ModHost {
    /// Invoke a launch provider once, after prepare and before check. Fallback
    /// providers participate only when no ordinary launch provider is published.
    /// The effective tier must have exactly one provider. Cancellation enters
    /// ordinary shutdown without a startup failure.
    ///
    /// # Safety
    /// Both target pointers must address initialized, exclusive launch storage
    /// for the whole callback. The game must not be loaded or running yet.
    pub unsafe fn launch(&mut self, target: &mut api::LaunchTargetV1) -> Result<bool> {
        if self.startup_failed
            || self.shutting_down
            || self.launch_finished
            || self.next_phase != api::PHASE_CHECK
        {
            return Err("cannot launch in the current Mod lifecycle state".into());
        }
        self.launch_finished = true;
        let mut providers = self.shared.providers(api::LAUNCH_INTERFACE_ID);
        if providers.is_empty() {
            providers = self.shared.providers(api::FALLBACK_LAUNCH_INTERFACE_ID);
        }
        let [(id, pointer)] = providers.as_slice() else {
            self.startup_failed = true;
            return Err(if providers.is_empty() {
                "no Mod provides the launch interface".into()
            } else {
                format!(
                    "multiple Mods provide the launch interface: {}",
                    providers
                        .iter()
                        .map(|(id, _)| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
        };
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.status.id == *id)
            .expect("published provider belongs to this host");
        entry.context.clear_error();
        let table = unsafe { &*(*pointer as *const api::LaunchApiV1) };
        let status = with_owner(id, || unsafe { (table.run)(table.context, target) });
        match status {
            api::OK => {
                entry.status.phase = "launch_ready";
                Ok(true)
            }
            api::CANCELLED => {
                entry.status.phase = "cancelled";
                self.shutting_down = true;
                Ok(false)
            }
            _ => {
                let error = format!("status {status}: {}", entry.context.error());
                entry.status.phase = "failed";
                entry.status.error = Some(error.clone());
                self.startup_failed = true;
                Err(format!("{id}: launch: {error}"))
            }
        }
    }
}
