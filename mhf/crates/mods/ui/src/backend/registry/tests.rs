use super::*;

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    fn push(&self, name: &str, event: &str) {
        self.0.lock().unwrap().push(format!("{name}:{event}"));
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct TracedOverlay {
    name: &'static str,
    events: Events,
    policy: InputPolicy,
}

impl Overlay for TracedOverlay {
    fn initialize(&mut self, _: &egui::Context) {
        self.events.push(self.name, "initialize");
    }

    fn ui(&mut self, _: &mut egui::Ui) {
        self.events.push(self.name, "ui");
    }

    fn input_policy(&self, _: &egui::Context) -> InputPolicy {
        self.policy
    }

    fn platform_output(&mut self, _: &egui::Context, _: &egui::PlatformOutput) {
        self.events.push(self.name, "output");
    }
}

impl Drop for TracedOverlay {
    fn drop(&mut self) {
        self.events.push(self.name, "drop");
    }
}

fn traced(name: &'static str, events: &Events, policy: InputPolicy) -> Box<dyn Overlay> {
    Box::new(TracedOverlay {
        name,
        events: events.clone(),
        policy,
    })
}

fn frame(registry: &mut OverlayRegistry, context: &egui::Context) {
    let mut output = context.run_ui(egui::RawInput::default(), |ui| registry.ui(ui));
    registry.platform_output(context, &output.platform_output);
    output.textures_delta.clear();
}

#[test]
fn contributions_initialize_and_render_in_registration_order() {
    let events = Events::default();
    let context = egui::Context::default();
    let mut registry = OverlayRegistry::default();
    let _first = registry.register(traced("first", &events, InputPolicy::default()));
    registry.initialize(&context);
    assert_eq!(events.take(), ["first:initialize"]);

    frame(&mut registry, &context);
    assert_eq!(events.take(), ["first:ui", "first:output"]);

    let _second = registry.register(traced("second", &events, InputPolicy::default()));
    assert!(events.take().is_empty());
    frame(&mut registry, &context);
    assert_eq!(
        events.take(),
        [
            "first:ui",
            "second:initialize",
            "second:ui",
            "first:output",
            "second:output",
        ]
    );
    frame(&mut registry, &context);
    assert_eq!(
        events.take(),
        ["first:ui", "second:ui", "first:output", "second:output"]
    );
}

#[test]
fn capture_uses_the_strongest_request_per_channel_and_clears_on_removal() {
    use InputCapture::{Auto, Block, PassThrough};

    let context = egui::Context::default();
    let events = Events::default();
    let mut registry = OverlayRegistry::default();
    let pass_through = InputPolicy {
        pointer: PassThrough,
        keyboard: PassThrough,
    };
    assert_eq!(registry.input_policy(&context), pass_through);
    let first = registry.register(traced(
        "first",
        &events,
        InputPolicy {
            pointer: Block,
            keyboard: PassThrough,
        },
    ));
    let second = registry.register(traced(
        "second",
        &events,
        InputPolicy {
            pointer: Auto,
            keyboard: Auto,
        },
    ));
    registry.initialize(&context);
    assert_eq!(
        registry.input_policy(&context),
        InputPolicy {
            pointer: Block,
            keyboard: Auto,
        }
    );
    drop(first);
    assert_eq!(registry.input_policy(&context), InputPolicy::default());
    drop(second);
    assert_eq!(registry.input_policy(&context), pass_through);
}

#[test]
fn unregister_releases_state_and_invalidates_existing_render_snapshots() {
    let context = egui::Context::default();
    let events = Events::default();
    let mut registry = OverlayRegistry::default();
    let mut registration = registry.register(traced("removed", &events, InputPolicy::default()));
    let snapshot = registry.snapshot();
    registration.unregister();
    registration.unregister();
    assert_eq!(events.take(), ["removed:drop"]);
    assert!(snapshot[0].lock().unwrap().is_none());

    registry.initialize(&context);
    frame(&mut registry, &context);
    assert!(events.take().is_empty());
}

#[test]
fn callbacks_can_register_contributions_for_the_next_frame() {
    let context = egui::Context::default();
    let events = Events::default();
    let mut registry = OverlayRegistry::default();
    let added = Arc::new(Mutex::new(None));
    let registration_slot = Arc::clone(&added);
    let shared_registry = registry.clone();
    let callback_events = events.clone();
    let _first = registry.register(Box::new(move |_: &mut egui::Ui| {
        let mut added = registration_slot.lock().unwrap();
        if added.is_none() {
            *added = Some(shared_registry.register(traced(
                "added",
                &callback_events,
                InputPolicy::default(),
            )));
        }
    }));
    registry.initialize(&context);
    frame(&mut registry, &context);
    assert!(events.take().is_empty());
    frame(&mut registry, &context);
    assert_eq!(
        events.take(),
        ["added:initialize", "added:ui", "added:output"]
    );
    // Release the callback's registration before its registry owner.
    added.lock().unwrap().take();
}

#[test]
fn unregister_waits_for_an_in_progress_callback() {
    use std::{sync::mpsc, thread, time::Duration};

    let mut registry = OverlayRegistry::default();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut registration = registry.register(Box::new(move |_: &mut egui::Ui| {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    }));
    let render = thread::spawn(move || frame(&mut registry, &egui::Context::default()));
    entered_rx.recv().unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let (removed_tx, removed_rx) = mpsc::channel();
    let remove = thread::spawn(move || {
        started_tx.send(()).unwrap();
        registration.unregister();
        removed_tx.send(()).unwrap();
    });
    started_rx.recv().unwrap();
    let while_rendering = removed_rx.recv_timeout(Duration::from_millis(50));
    release_tx.send(()).unwrap();
    render.join().unwrap();
    remove.join().unwrap();
    assert!(matches!(
        while_rendering,
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    removed_rx.recv().unwrap();
}
