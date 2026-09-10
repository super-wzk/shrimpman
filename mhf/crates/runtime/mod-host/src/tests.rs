use super::*;
use mhf_mod_package::{Manifest, Version};
use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

type Events = Rc<RefCell<Vec<String>>>;
static EARLY: u32 = 17;
static LATE: u32 = 23;

fn candidate(id: &str, dependencies: &[&str]) -> Candidate {
    Candidate::builtin(Manifest {
        schema: 1,
        id: id.into(),
        name: id.into(),
        version: Version::new(1, 0, 0),
        kind: Kind::Native,
        entry: None,
        dependencies: dependencies
            .iter()
            .map(|id| ((*id).into(), "^1".parse().unwrap()))
            .collect(),
    })
    .unwrap()
}

struct Probe {
    id: String,
    events: Events,
    failure: Rc<Cell<Option<&'static str>>>,
}

impl Probe {
    fn stage(&self, stage: &'static str) -> Result<()> {
        self.events
            .borrow_mut()
            .push(format!("{}:{stage}", self.id));
        if self.failure.get() == Some(stage) {
            Err(format!("{stage} failed"))
        } else {
            Ok(())
        }
    }
}

impl Module for Probe {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        unsafe { context.register("early", (&EARLY as *const u32).cast()) }?;
        self.stage("prepare")
    }
    fn check(&mut self, _context: &Context) -> Result<()> {
        self.stage("check")
    }
    fn attach(&mut self, context: &Context) -> Result<()> {
        unsafe { context.register("late", (&LATE as *const u32).cast()) }?;
        self.stage("attach")
    }
    fn stop(&mut self, _context: &Context) -> Result<()> {
        self.stage("stop")
    }
    fn detach(&mut self, _context: &Context) -> Result<()> {
        self.stage("detach")
    }
    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        self.stage("release")
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.events
            .borrow_mut()
            .push(format!("{}:destroy", self.id));
    }
}

fn host(
    candidates: Vec<Candidate>,
    failures: &BTreeMap<String, Rc<Cell<Option<&'static str>>>>,
    events: &Events,
) -> ModHost {
    ModHost::load(
        Resolved { mods: candidates },
        &BTreeMap::new(),
        |candidate| {
            Ok(Box::new(Probe {
                id: candidate.manifest.id.clone(),
                events: events.clone(),
                failure: failures
                    .get(&candidate.manifest.id)
                    .cloned()
                    .unwrap_or_default(),
            }))
        },
    )
    .unwrap()
}

fn attached(host: &mut ModHost) {
    host.prepare().unwrap();
    host.check().unwrap();
    host.attach().unwrap();
}

struct LaunchProbe {
    id: String,
    provider: Option<LaunchProvider>,
    events: Events,
}

impl Module for LaunchProbe {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        // The context must retain the stable table even when the helper itself
        // is dropped after registration.
        self.provider.take().unwrap().register(context)
    }

    fn stop(&mut self, _: &Context) -> Result<()> {
        self.events.borrow_mut().push(format!("{}:stop", self.id));
        Ok(())
    }

    fn detach(&mut self, _: &Context) -> Result<()> {
        self.events.borrow_mut().push(format!("{}:detach", self.id));
        Ok(())
    }
}

impl Drop for LaunchProbe {
    fn drop(&mut self) {
        self.events
            .borrow_mut()
            .push(format!("{}:destroy", self.id));
    }
}

fn launch_host(
    ids: &[&str],
    fallbacks: &[&str],
    outcome: &'static str,
    events: &Events,
) -> ModHost {
    ModHost::load(
        Resolved {
            mods: ids.iter().map(|id| candidate(id, &[])).collect(),
        },
        &BTreeMap::new(),
        |candidate| {
            let id = candidate.manifest.id.clone();
            let events = events.clone();
            let callback = {
                let id = id.clone();
                let events = events.clone();
                move |params: &mut api::game::LaunchParams32,
                      global: &mut api::game::GlobalData32| {
                    events.borrow_mut().push(format!("{id}:launch"));
                    match outcome {
                        "cancel" => Ok(false),
                        "error" => Err("launch configuration failed".into()),
                        "panic" => panic!("synthetic launch failure"),
                        _ => {
                            params.selected_character_id_1 = 7;
                            global.festa_id = 11;
                            Ok(true)
                        }
                    }
                }
            };
            let provider = if fallbacks.contains(&id.as_str()) {
                LaunchProvider::fallback(callback)
            } else {
                LaunchProvider::new(callback)
            };
            Ok(Box::new(LaunchProbe {
                id,
                provider: Some(provider),
                events,
            }))
        },
    )
    .unwrap()
}

#[test]
fn launch_requires_one_published_provider_before_invoking_any_callback() {
    let mut params = api::game::LaunchParams32::default();
    let mut global = api::game::GlobalData32::default();
    let mut target = api::LaunchTargetV1 {
        params: &mut params,
        global: &mut global,
    };
    let events = Events::default();
    let mut missing = host(vec![candidate("ordinary", &[])], &BTreeMap::new(), &events);
    missing.prepare().unwrap();
    assert!(
        unsafe { missing.launch(&mut target) }
            .unwrap_err()
            .contains("no Mod provides")
    );
    assert!(missing.check().is_err());
    drop(missing);
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| event == "ordinary:destroy")
    );

    events.borrow_mut().clear();
    let mut multiple = launch_host(&["first", "second"], &[], "ready", &events);
    multiple.prepare().unwrap();
    let error = unsafe { multiple.launch(&mut target) }.unwrap_err();
    assert!(error.contains("multiple Mods"), "{error}");
    assert!(error.contains("first, second"), "{error}");
    assert!(events.borrow().is_empty());
    assert!(multiple.check().is_err());
    drop(multiple);
    assert!(events.borrow().iter().any(|event| event == "first:destroy"));
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| event == "second:destroy")
    );
}

#[test]
fn launch_prepares_host_storage_once_and_only_between_prepare_and_check() {
    let mut params = api::game::LaunchParams32::default();
    let mut global = api::game::GlobalData32::default();
    let mut target = api::LaunchTargetV1 {
        params: &mut params,
        global: &mut global,
    };
    let events = Events::default();
    let mut host = launch_host(&["launch"], &[], "ready", &events);
    assert!(unsafe { host.launch(&mut target) }.is_err());
    host.prepare().unwrap();
    assert!(unsafe { host.launch(&mut target) }.unwrap());
    assert_eq!(params.selected_character_id_1, 7);
    assert_eq!(global.festa_id, 11);
    assert!(unsafe { host.launch(&mut target) }.is_err());
    host.check().unwrap();
    host.attach().unwrap();
    drop(host);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| *event == "launch:launch")
            .count(),
        1
    );

    events.borrow_mut().clear();
    let mut late = launch_host(&["launch"], &[], "ready", &events);
    late.prepare().unwrap();
    late.check().unwrap();
    assert!(unsafe { late.launch(&mut target) }.is_err());
    assert!(events.borrow().is_empty());
}

#[test]
fn launch_overrides_fallbacks_independently_of_prepare_order() {
    for (ids, fallbacks, selected) in [
        (&["default"][..], &["default"][..], "default"),
        (
            &["default", "replacement"][..],
            &["default"][..],
            "replacement",
        ),
        (
            &["replacement", "default"][..],
            &["default"][..],
            "replacement",
        ),
        (
            &["first-default", "replacement", "second-default"][..],
            &["first-default", "second-default"][..],
            "replacement",
        ),
    ] {
        let mut params = api::game::LaunchParams32::default();
        let mut global = api::game::GlobalData32::default();
        let mut target = api::LaunchTargetV1 {
            params: &mut params,
            global: &mut global,
        };
        let events = Events::default();
        let mut host = launch_host(ids, fallbacks, "ready", &events);
        host.prepare().unwrap();
        assert!(unsafe { host.launch(&mut target) }.unwrap());
        assert_eq!(*events.borrow(), [format!("{selected}:launch")]);
    }
}

#[test]
fn launch_rejects_ambiguity_within_the_effective_tier() {
    for (ids, fallbacks) in [
        (&["default", "first", "second"][..], &["default"][..]),
        (&["first", "second"][..], &["first", "second"][..]),
    ] {
        let mut params = api::game::LaunchParams32::default();
        let mut global = api::game::GlobalData32::default();
        let mut target = api::LaunchTargetV1 {
            params: &mut params,
            global: &mut global,
        };
        let events = Events::default();
        let mut host = launch_host(ids, fallbacks, "ready", &events);
        host.prepare().unwrap();
        let error = unsafe { host.launch(&mut target) }.unwrap_err();
        assert!(error.contains("first, second"), "{error}");
        assert!(events.borrow().is_empty());
        assert!(host.check().is_err());
    }
}

#[test]
fn cancelled_launch_shuts_down_normally_without_running_or_retaining_mods() {
    let mut params = api::game::LaunchParams32::default();
    let mut global = api::game::GlobalData32::default();
    let mut target = api::LaunchTargetV1 {
        params: &mut params,
        global: &mut global,
    };
    let events = Events::default();
    let mut host = launch_host(&["launch"], &[], "cancel", &events);
    host.prepare().unwrap();
    assert!(!unsafe { host.launch(&mut target) }.unwrap());
    assert_eq!(host.statuses()[0].phase, "cancelled");
    assert!(host.statuses()[0].error.is_none());
    assert!(host.check().is_err());
    assert!(unsafe { host.launch(&mut target) }.is_err());
    drop(host);
    assert_eq!(
        *events.borrow(),
        [
            "launch:launch",
            "launch:stop",
            "launch:detach",
            "launch:destroy"
        ]
    );
}

#[test]
fn launch_errors_and_panics_report_provider_details_and_still_clean_up() {
    for (outcome, message) in [
        ("error", "launch configuration failed"),
        ("panic", "launch provider panicked"),
    ] {
        let mut params = api::game::LaunchParams32::default();
        let mut global = api::game::GlobalData32::default();
        let mut target = api::LaunchTargetV1 {
            params: &mut params,
            global: &mut global,
        };
        let events = Events::default();
        let mut host = launch_host(&["launch"], &[], outcome, &events);
        host.prepare().unwrap();
        let error = unsafe { host.launch(&mut target) }.unwrap_err();
        assert!(error.contains(message), "{error}");
        assert_eq!(host.statuses()[0].phase, "failed");
        assert!(host.check().is_err());
        drop(host);
        assert_eq!(
            *events.borrow(),
            [
                "launch:launch",
                "launch:stop",
                "launch:detach",
                "launch:destroy"
            ]
        );
    }
}

#[test]
fn publishes_interfaces_transactionally_and_only_to_declared_consumers() {
    let events = Events::default();
    let failure = Rc::new(Cell::new(Some("attach")));
    let mut host = host(
        vec![
            candidate("provider", &[]),
            candidate("consumer", &["provider"]),
            candidate("other", &[]),
        ],
        &BTreeMap::from([("provider".into(), failure)]),
        &events,
    );
    assert!(
        unsafe {
            host.entries[0]
                .context
                .register("outside", (&EARLY as *const u32).cast())
        }
        .is_err()
    );
    host.prepare().unwrap();
    assert_eq!(
        host.entries[0]
            .context
            .interface("provider", "early")
            .unwrap(),
        (&EARLY as *const u32).cast()
    );
    assert_eq!(
        unsafe {
            *host.entries[1]
                .context
                .interface("provider", "early")
                .unwrap()
                .cast::<u32>()
        },
        EARLY
    );
    assert!(
        host.entries[2]
            .context
            .interface("provider", "early")
            .is_err()
    );
    host.check().unwrap();
    assert!(host.attach().is_err());
    assert!(
        host.entries[1]
            .context
            .interface("provider", "late")
            .is_err()
    );
    assert!(
        host.entries[1]
            .context
            .interface("provider", "early")
            .is_ok()
    );
    assert!(host.attach().is_err());
    drop(host);
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| event == "provider:destroy")
    );
}

#[test]
fn failed_prepare_never_exposes_its_interfaces_or_runs_unstarted_cleanup() {
    let events = Events::default();
    let mut host = host(
        vec![
            candidate("provider", &[]),
            candidate("consumer", &["provider"]),
        ],
        &BTreeMap::from([("provider".into(), Rc::new(Cell::new(Some("prepare"))))]),
        &events,
    );
    assert!(host.prepare().is_err());
    assert!(
        host.entries[1]
            .context
            .interface("provider", "early")
            .is_err()
    );
    drop(host);
    let events = events.borrow();
    assert!(!events.iter().any(|event| event == "consumer:stop"));
    assert!(events.iter().any(|event| event == "provider:detach"));
    assert!(
        events
            .iter()
            .position(|event| event == "consumer:destroy")
            .unwrap()
            < events
                .iter()
                .position(|event| event == "provider:destroy")
                .unwrap()
    );
}

#[test]
fn stop_failure_keeps_transitive_providers_operational_and_can_be_retried() {
    let events = Events::default();
    let failure = Rc::new(Cell::new(Some("stop")));
    let mut host = host(
        vec![
            candidate("base", &[]),
            candidate("provider", &["base"]),
            candidate("consumer", &["provider"]),
            candidate("unrelated", &[]),
        ],
        &BTreeMap::from([("consumer".into(), failure.clone())]),
        &events,
    );
    attached(&mut host);
    events.borrow_mut().clear();
    assert!(host.stop().is_err());
    assert_eq!(*events.borrow(), ["unrelated:stop", "consumer:stop"]);
    failure.set(None);
    unsafe { host.detach(&[]) }.unwrap();
    unsafe { host.prepare_release() }.unwrap();
    drop(host);
    let events = events.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|event| *event == "unrelated:stop")
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .position(|event| event == "consumer:destroy")
            .unwrap()
            < events
                .iter()
                .position(|event| event == "provider:destroy")
                .unwrap()
    );
}

#[test]
fn detach_failure_retains_providers_and_drop_does_not_retry_or_destroy() {
    let events = Events::default();
    let mut host = host(
        vec![
            candidate("provider", &[]),
            candidate("consumer", &["provider"]),
            candidate("unrelated", &[]),
        ],
        &BTreeMap::from([("consumer".into(), Rc::new(Cell::new(Some("detach"))))]),
        &events,
    );
    attached(&mut host);
    events.borrow_mut().clear();
    assert!(unsafe { host.detach(&["provider", "consumer"]) }.is_err());
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| event == "provider:detach")
    );
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| event == "unrelated:detach")
    );
    let before = events.borrow().clone();
    drop(host);
    assert_eq!(*events.borrow(), before);
}

#[test]
fn a_stop_failure_still_allows_unrelated_detachment() {
    let events = Events::default();
    let mut host = host(
        vec![
            candidate("provider", &[]),
            candidate("consumer", &["provider"]),
            candidate("unrelated", &[]),
        ],
        &BTreeMap::from([("consumer".into(), Rc::new(Cell::new(Some("stop"))))]),
        &events,
    );
    attached(&mut host);
    events.borrow_mut().clear();
    assert!(unsafe { host.detach(&[]) }.is_err());
    assert_eq!(
        *events.borrow(),
        ["unrelated:stop", "consumer:stop", "unrelated:detach"]
    );
    drop(host);
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| event.ends_with(":destroy"))
    );
}

#[test]
fn callback_can_query_registry_before_its_own_publication_commits() {
    struct Publishing(Rc<Cell<*const Context>>);
    impl Module for Publishing {
        fn attach(&mut self, context: &Context) -> Result<()> {
            unsafe { context.register("value", (&EARLY as *const u32).cast()) }?;
            assert!(context.interface("provider", "value").is_err());
            let observer = unsafe { &*self.0.get() };
            assert!(observer.interface("provider", "value").is_err());
            Ok(())
        }
    }
    struct Empty;
    impl Module for Empty {}
    let observer = Rc::new(Cell::new(std::ptr::null()));
    let mut host = ModHost::load(
        Resolved {
            mods: vec![
                candidate("provider", &[]),
                candidate("consumer", &["provider"]),
            ],
        },
        &BTreeMap::new(),
        |candidate| {
            if candidate.manifest.id == "provider" {
                Ok(Box::new(Publishing(observer.clone())) as Box<dyn Module>)
            } else {
                Ok(Box::new(Empty) as Box<dyn Module>)
            }
        },
    )
    .unwrap();
    observer.set(&*host.entries[1].context);
    attached(&mut host);
    assert!(
        host.entries[1]
            .context
            .interface("provider", "value")
            .is_ok()
    );
}

#[test]
fn consumer_can_use_public_dependency_table_during_destroy() {
    struct Consumer {
        host: *const api::HostV2,
        events: Events,
    }
    impl Module for Consumer {
        fn prepare(&mut self, context: &Context) -> Result<()> {
            self.host = context.api();
            Ok(())
        }
    }
    impl Drop for Consumer {
        fn drop(&mut self) {
            let host = unsafe { &*self.host };
            let mut table = std::ptr::null();
            let status = unsafe {
                (host.dependency)(
                    host.context,
                    api::Str::new("provider"),
                    api::Str::new("early"),
                    &mut table,
                )
            };
            assert_eq!(status, api::OK);
            assert_eq!(unsafe { *table.cast::<u32>() }, EARLY);
            assert!(
                !self
                    .events
                    .borrow()
                    .iter()
                    .any(|event| event == "provider:destroy")
            );
            self.events.borrow_mut().push("consumer:destroy".into());
        }
    }
    let events = Events::default();
    let mut host = ModHost::load(
        Resolved {
            mods: vec![
                candidate("provider", &[]),
                candidate("consumer", &["provider"]),
            ],
        },
        &BTreeMap::new(),
        |candidate| {
            if candidate.manifest.id == "provider" {
                Ok(Box::new(Probe {
                    id: "provider".into(),
                    events: events.clone(),
                    failure: Rc::default(),
                }) as Box<dyn Module>)
            } else {
                Ok(Box::new(Consumer {
                    host: std::ptr::null(),
                    events: events.clone(),
                }) as Box<dyn Module>)
            }
        },
    )
    .unwrap();
    attached(&mut host);
    unsafe { host.detach(&[]) }.unwrap();
    unsafe { host.prepare_release() }.unwrap();
    drop(host);
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| event == "provider:destroy")
    );
}

#[test]
fn dependency_order_overrides_builtin_preference_and_release_precedes_destroy() {
    let events = Events::default();
    let mut host = host(
        vec![
            candidate("provider", &[]),
            candidate("consumer", &["provider"]),
        ],
        &BTreeMap::new(),
        &events,
    );
    attached(&mut host);
    events.borrow_mut().clear();
    unsafe { host.detach(&["provider", "consumer"]) }.unwrap();
    unsafe { host.prepare_release() }.unwrap();
    events.borrow_mut().push("game:release".into());
    drop(host);
    assert_eq!(
        *events.borrow(),
        [
            "consumer:stop",
            "provider:stop",
            "consumer:detach",
            "provider:detach",
            "consumer:release",
            "provider:release",
            "game:release",
            "consumer:destroy",
            "provider:destroy"
        ]
    );
}

#[test]
fn release_failure_keeps_buffers_and_providers_resident() {
    let events = Events::default();
    let mut host = host(
        vec![
            candidate("provider", &[]),
            candidate("consumer", &["provider"]),
        ],
        &BTreeMap::from([("consumer".into(), Rc::new(Cell::new(Some("release"))))]),
        &events,
    );
    attached(&mut host);
    unsafe { host.detach(&[]) }.unwrap();
    events.borrow_mut().clear();
    assert!(unsafe { host.prepare_release() }.is_err());
    drop(host);
    assert_eq!(*events.borrow(), ["consumer:release"]);
}

#[test]
fn stopping_mods_does_not_claim_that_the_game_has_stopped() {
    let events = Events::default();
    let mut host = host(vec![candidate("a", &[])], &BTreeMap::new(), &events);
    attached(&mut host);
    host.running();
    host.stop().unwrap();
    events.borrow_mut().clear();
    drop(host);
    assert!(events.borrow().is_empty());
}

#[test]
fn public_host_table_copies_config_and_preserves_last_error() {
    let shared = Arc::new(Shared::new());
    let context = Context::new(&candidate("a", &[]), "speed = 2\n".into(), shared.clone());
    let host = context.api();
    let mut required = 0;
    assert_eq!(
        unsafe { (host.config)(host.context, std::ptr::null_mut(), 0, &mut required) },
        api::BUFFER_TOO_SMALL
    );
    assert_eq!(required, 10);
    let mut bytes = [0xAA; 10];
    assert_eq!(
        unsafe { (host.config)(host.context, bytes.as_mut_ptr(), 3, &mut required) },
        api::BUFFER_TOO_SMALL
    );
    assert_eq!(bytes, [0xAA; 10]);
    assert_eq!(
        unsafe { (host.config)(host.context, bytes.as_mut_ptr(), 10, &mut required) },
        api::OK
    );
    assert_eq!(&bytes, b"speed = 2\n");
    let mut table = std::ptr::null();
    assert_eq!(
        unsafe {
            (host.dependency)(
                host.context,
                api::Str::new("missing"),
                api::Str::new("v1"),
                &mut table,
            )
        },
        api::NOT_FOUND
    );
    let error = context.error();
    assert_eq!(
        unsafe { (host.last_error)(host.context, std::ptr::null_mut(), 0, &mut required) },
        api::BUFFER_TOO_SMALL
    );
    assert_eq!(context.error(), error);
    shared.set_game(123_usize as *mut c_void);
    let mut game = context.game();
    assert_eq!(
        unsafe { (host.game_info)(host.context, &mut game) },
        api::OK
    );
    assert_eq!(game.module_base as usize, 123);
}

thread_local! { static NATIVE_EVENTS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) }; }

struct NativeInstance(*const api::HostV2);
unsafe extern "C" fn create_native(host: *const api::HostV2, out: *mut *mut c_void) -> api::Status {
    NATIVE_EVENTS.with(|events| events.borrow_mut().push("create"));
    unsafe {
        out.write(Box::into_raw(Box::new(NativeInstance(host))).cast());
    }
    api::OK
}
unsafe extern "C" fn native_failure(instance: *mut c_void) -> api::Status {
    let instance = unsafe { &*instance.cast::<NativeInstance>() };
    let host = unsafe { &*instance.0 };
    unsafe {
        (host.log)(
            host.context,
            api::LOG_ERROR,
            api::Str::new("native failure"),
        );
    }
    api::ERROR
}
unsafe extern "C" fn destroy_native(instance: *mut c_void) {
    drop(unsafe { Box::from_raw(instance.cast::<NativeInstance>()) });
    NATIVE_EVENTS.with(|events| events.borrow_mut().push("destroy"));
}
struct Library;
impl Drop for Library {
    fn drop(&mut self) {
        NATIVE_EVENTS.with(|events| events.borrow_mut().push("unload"));
    }
}
static NATIVE: api::ModV2 = api::ModV2 {
    create: create_native,
    prepare: Some(native_failure),
    check: None,
    attach: None,
    stop: None,
    detach: None,
    destroy: destroy_native,
};

#[test]
fn native_c_adapter_reports_callback_errors_and_destroys_before_unloading() {
    NATIVE_EVENTS.with(|events| events.borrow_mut().clear());
    let context = Context::new(
        &candidate("native", &[]),
        String::new(),
        Arc::new(Shared::new()),
    );
    let mut native = unsafe { native::from_table(&NATIVE, &context, Library) }.unwrap();
    assert!(
        native
            .prepare(&context)
            .unwrap_err()
            .contains("native failure")
    );
    native.attach(&context).unwrap(); // Optional callback is a no-op.
    drop(native);
    NATIVE_EVENTS.with(|events| assert_eq!(*events.borrow(), ["create", "destroy", "unload"]));
}

#[test]
fn data_package_is_consumable_without_a_dll_or_sdk() {
    let temp = Temp::new();
    fs::write(temp.0.join("words.txt"), "你好").unwrap();
    let mut data = candidate("dictionary", &[]);
    data.manifest.kind = Kind::Data;
    data.source = Source::Directory(temp.0.clone());
    let mut host = host(
        vec![data, candidate("consumer", &["dictionary"])],
        &BTreeMap::new(),
        &Events::default(),
    );
    host.prepare().unwrap();
    let table = unsafe {
        &*host.entries[1]
            .context
            .interface("dictionary", data::INTERFACE_ID)
            .unwrap()
            .cast::<data::DataV1>()
    };
    let mut required = 0;
    assert_eq!(
        unsafe {
            (table.read_file)(
                table.context,
                api::Str::new("words.txt"),
                std::ptr::null_mut(),
                0,
                &mut required,
            )
        },
        api::BUFFER_TOO_SMALL
    );
    assert_eq!(required, 6);
    let mut bytes = [0; 6];
    assert_eq!(
        unsafe {
            (table.read_file)(
                table.context,
                api::Str::new("words.txt"),
                bytes.as_mut_ptr(),
                6,
                &mut required,
            )
        },
        api::OK
    );
    assert_eq!(&bytes, "你好".as_bytes());
    for path in ["../outside", "/outside", "..\\outside", "C:/outside"] {
        assert_eq!(
            unsafe {
                (table.read_file)(
                    table.context,
                    api::Str::new(path),
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            api::ERROR
        );
    }
    assert_eq!(
        unsafe {
            (table.read_file)(
                table.context,
                api::Str::new("missing"),
                std::ptr::null_mut(),
                0,
                &mut required,
            )
        },
        api::NOT_FOUND
    );
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "mhf-mod-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
