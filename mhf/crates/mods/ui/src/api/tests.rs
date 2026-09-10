use super::*;
use std::{
    cell::RefCell,
    ffi::c_void,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct DropCounter(Arc<AtomicUsize>);
impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
#[derive(Default)]
struct State {
    fail_register: bool,
    fail_unregister: bool,
    unregister_count: usize,
    render: Option<RenderFn>,
    user: *mut c_void,
}
impl UiHostApi for RefCell<State> {
    unsafe fn register_panel(
        &self,
        _: str::Ref<'_>,
        render: RenderFn,
        user: *mut c_void,
        out: &mut u64,
    ) -> api::Status {
        let mut state = self.borrow_mut();
        if state.fail_register {
            return api::ERROR;
        }
        state.render = Some(render);
        state.user = user;
        *out = 1;
        api::OK
    }
    fn unregister_panel(&self, _: u64) -> api::Status {
        let mut state = self.borrow_mut();
        state.unregister_count += 1;
        if state.fail_unregister {
            return api::ERROR;
        }
        state.render = None;
        state.user = std::ptr::null_mut();
        api::OK
    }
}
fn mock(fail_register: bool, fail_unregister: bool) -> (Rc<RefCell<State>>, UiHostTable) {
    let state = Rc::new(RefCell::new(State {
        fail_register,
        fail_unregister,
        ..Default::default()
    }));
    let table = state.clone().into();
    (state, table)
}
fn panel(
    table: &UiHostTable,
    dropped: Arc<AtomicUsize>,
) -> Panel<'_, impl FnMut(&Ui<'_>) + Send + 'static> {
    let marker = DropCounter(dropped);
    Panel::register(table, "test panel", move |_: &Ui<'_>| {
        std::hint::black_box(&marker);
    })
    .unwrap()
}
fn render(state: &RefCell<State>, ui: &UiTable<'_>) -> api::Status {
    let (render, user) = {
        let state = state.borrow();
        (state.render.unwrap(), state.user)
    };
    // As in the host, erase only the inner ABI lifetime for this invocation.
    unsafe { render(user, (ui as *const UiTable<'_>).cast()) }
}
struct MockUi(Option<Arc<AtomicUsize>>);
impl Drop for MockUi {
    fn drop(&mut self) {
        if let Some(drops) = &self.0 {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}
impl UiApi for MockUi {
    fn label(&self, _: str::Ref<'_>) -> api::Status {
        api::OK
    }
    fn button(&self, _: str::Ref<'_>, out: &mut bool) -> api::Status {
        *out = true;
        api::OK
    }
    fn checkbox(&self, _: str::Ref<'_>, value: &mut bool) -> api::Status {
        *value = !*value;
        api::OK
    }
}
fn ui_table() -> UiTable<'static> {
    Box::new(MockUi(None)).into()
}

#[test]
fn failed_close_retains_state_and_can_be_retried() {
    let (state, table) = mock(false, true);
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut panel = panel(&table, dropped.clone());
    assert!(panel.close().is_err());
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    assert_eq!(render(&state, &ui_table()), api::OK);
    state.borrow_mut().fail_unregister = false;
    panel.close().unwrap();
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert!(state.borrow().render.is_none());
    drop(panel);
    assert_eq!(state.borrow().unregister_count, 2);
}
#[test]
fn failed_drop_preserves_callback_until_provider_stops() {
    let (state, table) = mock(false, true);
    let dropped = Arc::new(AtomicUsize::new(0));
    let panel = panel(&table, dropped.clone());
    let callback = panel.callback.unwrap().as_ptr();
    drop(panel);
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    assert_eq!(render(&state, &ui_table()), api::OK);
    // The provider is stopped before releasing the intentionally retained box.
    state.borrow_mut().render = None;
    state.borrow_mut().user = std::ptr::null_mut();
    unsafe { drop(Box::from_raw(callback)) };
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
}
#[test]
fn failed_registration_releases_its_single_callback_allocation() {
    let (state, table) = mock(true, false);
    let dropped = Arc::new(AtomicUsize::new(0));
    let marker = DropCounter(dropped.clone());
    let result = Panel::register(&table, "failed panel", move |_: &Ui<'_>| {
        std::hint::black_box(&marker);
    });
    assert!(result.is_err());
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert!(state.borrow().render.is_none());
    assert_eq!(state.borrow().unregister_count, 0);
}
#[test]
fn callbacks_borrow_the_frame_and_contain_panics() {
    let (state, table) = mock(false, false);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut count = 0;
    let _panel = Panel::register(&table, "callback", move |ui: &Ui<'_>| {
        count += 1;
        observed.store(count, Ordering::Relaxed);
        ui.label("中文").unwrap();
        assert!(ui.button("button").unwrap());
        let mut checked = false;
        ui.checkbox("checkbox", &mut checked).unwrap();
        assert!(checked);
        if count == 2 {
            panic!("synthetic renderer failure");
        }
    })
    .unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let frame = MockUi(Some(dropped.clone()));
    let ui: UiTable<'_> = (&frame).into();
    assert_eq!(render(&state, &ui), api::OK);
    assert_eq!(render(&state, &ui), api::ERROR);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    drop(ui);
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    drop(frame);
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
}
