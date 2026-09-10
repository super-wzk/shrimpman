//! UI provider implements the same traits exposed to consumers.

use crate::{
    Overlay, OverlayRegistration, OverlayRegistry, RenderFn, UiApi, UiHostApi, UiHostTable,
    UiTable, egui,
};
use mhf_mod_sdk::abi as api;
use safer_ffi::prelude::str;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

fn guarded(operation: impl FnOnce() -> api::Status) -> api::Status {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or(api::ERROR)
}

pub(crate) struct UiService {
    pub(crate) api: UiHostTable,
}
struct UiHostState {
    registry: OverlayRegistry,
    registrations: Mutex<BTreeMap<u64, OverlayRegistration>>,
    next: AtomicU64,
}
impl UiService {
    pub(crate) fn new(registry: OverlayRegistry) -> Self {
        Self {
            api: Box::new(UiHostState {
                registry,
                registrations: Mutex::new(BTreeMap::new()),
                next: AtomicU64::new(1),
            })
            .into(),
        }
    }
}

struct Panel {
    title: String,
    id: u64,
    render: RenderFn,
    user: *mut c_void,
}
// The ABI requires callback state usable on the rendering thread, retained
// through synchronous unregister / Mod stop.
unsafe impl Send for Panel {}
impl Overlay for Panel {
    fn initialize(&mut self, context: &egui::Context) {
        mhf_font::install(context);
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Window::new(&self.title)
            .id(egui::Id::new(("mod-panel", self.id)))
            .show(ui.ctx(), |ui| {
                let status = {
                    let state = UiState {
                        ui: RefCell::new(&mut *ui),
                    };
                    let table: UiTable<'_> = (&state).into();
                    // C cannot express this frame's lifetime. The pointer is
                    // borrowed only for render; both the table and state stay
                    // alive until it returns, and the SDK's Ui cannot escape.
                    unsafe { (self.render)(self.user, (&table as *const UiTable<'_>).cast()) }
                };
                if status != api::OK {
                    ui.label(format!("Mod 界面调用失败：{status}"));
                }
            });
    }
}

impl UiHostApi for UiHostState {
    unsafe fn register_panel(
        &self,
        title: str::Ref<'_>,
        render: RenderFn,
        user: *mut c_void,
        out: &mut u64,
    ) -> api::Status {
        guarded(|| {
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let registration = self.registry.register(Box::new(Panel {
                title: title.as_str().to_owned(),
                id,
                render,
                user,
            }));
            self.registrations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, registration);
            *out = id;
            api::OK
        })
    }
    fn unregister_panel(&self, id: u64) -> api::Status {
        guarded(|| {
            let registration = self
                .registrations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            if let Some(mut registration) = registration {
                registration.unregister();
            }
            api::OK
        })
    }
}
struct UiState<'a> {
    ui: RefCell<&'a mut egui::Ui>,
}
impl UiApi for UiState<'_> {
    fn label(&self, text: str::Ref<'_>) -> api::Status {
        guarded(|| {
            self.ui.borrow_mut().label(text.as_str());
            api::OK
        })
    }
    fn button(&self, text: str::Ref<'_>, out: &mut bool) -> api::Status {
        guarded(|| {
            *out = self.ui.borrow_mut().button(text.as_str()).clicked();
            api::OK
        })
    }
    fn checkbox(&self, text: str::Ref<'_>, value: &mut bool) -> api::Status {
        guarded(|| {
            self.ui.borrow_mut().checkbox(value, text.as_str());
            api::OK
        })
    }
}
