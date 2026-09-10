mod events;

use egui::{Context, Event, Id, Key, Modifiers, RawInput, Rect, Response, Ui, pos2, vec2};
use egui_hunter::{
    Button, Direction, EngagementPlugin, FocusEngagement, GamepadState, InputDevice,
    NavigationInput, Panel, TextField, consume_escape,
};

fn region_a() -> Id {
    Id::new("region-a")
}
fn region_b() -> Id {
    Id::new("region-b")
}
fn action_a() -> Id {
    Id::new("action-a")
}
fn action_b() -> Id {
    Id::new("action-b")
}
fn editor() -> Id {
    Id::new("editor")
}
fn outside() -> Id {
    Id::new("outside")
}

#[derive(Clone, Copy, Default)]
enum Content {
    #[default]
    Form,
    Single,
    Slider,
}
struct Model {
    render_scope: bool,
    empty: bool,
    enabled: bool,
    show_b: bool,
    content: Content,
    value: String,
    slider: f32,
    clicks: usize,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            render_scope: true,
            empty: false,
            enabled: true,
            show_b: true,
            content: Content::Form,
            value: "hunter".into(),
            slider: 0.5,
            clicks: 0,
        }
    }
}
struct Scene {
    frames: Vec<Response>,
    controls: Vec<Response>,
    header: Response,
    back: bool,
    tab_available: bool,
}
impl Model {
    fn show(&mut self, ui: &mut Ui, initial: bool) -> Scene {
        let mut engagement = FocusEngagement::new(Id::new("page"));
        if self.render_scope {
            engagement.begin(ui, initial.then_some(region_a()));
        }
        let header = ui.add(Button::new("Outside header").id(outside()));
        self.clicks += usize::from(header.clicked());
        let mut frames = Vec::new();
        let mut all = Vec::new();
        if self.render_scope && !self.empty {
            let first = engagement.show(ui, region_a(), |ui, controls| {
                Panel::new("First").show(ui, |ui| {
                    if matches!(self.content, Content::Slider) {
                        let slider = ui
                            .add(egui::Slider::new(&mut self.slider, 0.0..=1.0).show_value(false));
                        controls.push(slider);
                    } else {
                        let action = ui
                            .add_enabled(self.enabled, Button::new("First action").id(action_a()));
                        self.clicks += usize::from(action.clicked());
                        controls.push(action);
                        if matches!(self.content, Content::Form) {
                            if self.show_b {
                                let action = ui.add(Button::new("Second action").id(action_b()));
                                self.clicks += usize::from(action.clicked());
                                controls.push(action);
                            }
                            controls.push(ui.add(TextField::new(editor(), &mut self.value)));
                        }
                    }
                });
                all.extend(controls.iter().cloned());
            });
            frames.push(first.response);
            let second = engagement.show(ui, region_b(), |ui, controls| {
                Panel::new("Second").show(ui, |ui| {
                    let action = ui.add(Button::new("Other region").id(Id::new("other")));
                    self.clicks += usize::from(action.clicked());
                    controls.push(action);
                });
                all.extend(controls.iter().cloned());
            });
            frames.push(second.response);
        }
        if self.render_scope {
            engagement.navigate(ui);
        }
        Scene {
            frames,
            controls: all,
            header,
            tab_available: ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            ..
                        }
                    )
                })
            }),
            back: consume_escape(ui.ctx()),
        }
    }
}
struct Host {
    ctx: Context,
    adapter: NavigationInput,
    model: Model,
    first: bool,
    time: f64,
}
impl Default for Host {
    fn default() -> Self {
        let ctx = Context::default();
        ctx.add_plugin(EngagementPlugin::default());
        Self {
            ctx,
            adapter: NavigationInput::default(),
            model: Model::default(),
            first: true,
            time: 0.0,
        }
    }
}
impl Host {
    fn step(&mut self, events: Vec<Event>, pad: GamepadState) -> Scene {
        self.time += 0.016;
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(700.0, 1000.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.adapter.apply(&self.ctx, &mut raw, pad);
        let mut scene = None;
        self.ctx
            .run_ui(raw, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| scene = Some(self.model.show(ui, self.first)));
            })
            .drop_without_applying_deltas();
        self.first = false;
        scene.unwrap()
    }
    fn idle(&mut self) -> Scene {
        self.step(vec![], GamepadState::default())
    }
    fn open(&mut self) -> Scene {
        let scene = self.idle();
        if let Some(first) = scene.controls.first() {
            first.request_focus();
        }
        self.idle()
    }
    fn key(&mut self, key: Key, shift: bool) -> Scene {
        let modifiers = if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        self.step(
            [true, false]
                .map(|pressed| Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .into(),
            GamepadState::default(),
        )
    }
    fn pad(&mut self, state: GamepadState) -> Scene {
        let scene = self.step(vec![], state);
        self.idle();
        scene
    }
    fn focus(&self) -> Option<Id> {
        self.ctx.memory(|memory| memory.focused())
    }
}

#[test]
fn keyboard_keeps_native_tab_activation_and_escape_without_frame_stops() {
    let mut host = Host::default();
    let scene = host.open();
    assert!(scene.frames.iter().all(|frame| !frame.sense.is_focusable()));
    assert!(scene.controls.iter().all(Response::enabled));
    for expected in [action_b(), editor(), Id::new("other")] {
        let scene = host.key(Key::Tab, false);
        assert!(
            scene.tab_available,
            "engagement must not consume physical Tab"
        );
        assert_eq!(host.focus(), Some(expected));
    }
    host.key(Key::Enter, false);
    assert_eq!(host.model.clicks, 1);
    assert!(
        host.key(Key::Escape, false).back,
        "keyboard Escape belongs to normal page UI"
    );
}

#[test]
fn controller_enters_once_cycles_only_current_controls_remembers_and_disengages() {
    let mut host = Host::default();
    host.open();
    host.pad(GamepadState {
        next_focus: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(region_b()));
    host.pad(GamepadState {
        previous_focus: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(region_a()));
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(action_a()));
    assert_eq!(host.model.clicks, 0);
    for expected in [action_b(), editor(), action_a()] {
        host.pad(GamepadState {
            next_focus: true,
            ..Default::default()
        });
        assert_eq!(host.focus(), Some(expected));
    }
    host.pad(GamepadState {
        previous_focus: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(editor()));
    host.pad(GamepadState {
        cancel: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), None, "first B lets the native editor blur");
    let back = host.pad(GamepadState {
        cancel: true,
        ..Default::default()
    });
    assert!(!back.back);
    assert_eq!(host.focus(), Some(region_a()));
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(editor()));
    host.key(Key::Tab, false);
    assert_eq!(host.adapter.device(), InputDevice::KeyboardMouse);
    assert_eq!(
        host.focus(),
        Some(Id::new("other")),
        "physical Tab can leave the former engagement region"
    );
}

#[test]
fn pointer_motion_does_not_cancel_controller_but_one_click_operates_any_child() {
    let mut host = Host::default();
    let scene = host.open();
    host.pad(GamepadState {
        next_focus: true,
        ..Default::default()
    });
    host.step(
        vec![Event::PointerMoved(pos2(2.0, 2.0))],
        GamepadState::default(),
    );
    assert_eq!(host.adapter.device(), InputDevice::Gamepad);
    assert_eq!(host.focus(), Some(region_b()));
    let point = scene.controls[0].rect.center();
    host.step(events::pointer(point, true), GamepadState::default());
    let clicked = host.step(events::pointer(point, false), GamepadState::default());
    assert_eq!(host.model.clicks, 1);
    assert_eq!(host.adapter.device(), InputDevice::KeyboardMouse);
    assert!(
        clicked
            .frames
            .iter()
            .all(|frame| !frame.sense.is_focusable())
    );
    assert!(clicked.controls.iter().all(Response::enabled));
}

#[test]
fn physical_input_in_the_same_frame_wins_without_being_consumed_or_activated_twice() {
    let mut host = Host::default();
    host.open();
    let scene = host.step(
        vec![events::key(Key::Tab)],
        GamepadState {
            confirm: true,
            ..Default::default()
        },
    );
    assert!(scene.tab_available);
    assert_eq!(host.focus(), Some(action_b()));
    assert_eq!(host.model.clicks, 0);
    assert_eq!(host.adapter.device(), InputDevice::KeyboardMouse);
}

#[test]
fn native_text_arrows_and_controller_button_direction_respect_the_region_boundary() {
    let mut host = Host::default();
    host.open();
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    host.pad(GamepadState {
        direction: Some(Direction::Down),
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(action_b()));
    host.pad(GamepadState {
        next_focus: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(editor()));
    host.pad(GamepadState {
        direction: Some(Direction::Down),
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(editor()));
    assert_eq!(host.model.value, "hunter");

    let mut host = Host::default();
    host.model.content = Content::Single;
    host.open();
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    host.pad(GamepadState {
        direction: Some(Direction::Down),
        ..Default::default()
    });
    assert_eq!(
        host.focus(),
        Some(action_a()),
        "native geometry must not escape to the other region"
    );
}

#[test]
fn native_slider_retains_its_direction_keys_including_at_both_endpoints() {
    let mut host = Host::default();
    host.model.content = Content::Slider;
    let scene = host.open();
    let slider = scene.controls[0].id;
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    host.pad(GamepadState {
        direction: Some(Direction::Right),
        ..Default::default()
    });
    assert!(host.model.slider > 0.5);
    assert_eq!(host.focus(), Some(slider));
    for (value, direction) in [(1.0, Direction::Right), (0.0, Direction::Left)] {
        host.model.slider = value;
        host.pad(GamepadState {
            direction: Some(direction),
            ..Default::default()
        });
        assert_eq!(host.model.slider, value);
        assert_eq!(host.focus(), Some(slider));
    }
}

#[test]
fn hidden_and_disabled_targets_fall_back_without_affecting_keyboard_availability() {
    let mut host = Host::default();
    host.open();
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    host.pad(GamepadState {
        next_focus: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(action_b()));
    host.model.show_b = false;
    host.idle();
    assert_eq!(host.focus(), Some(action_a()));
    host.model.enabled = false;
    let scene = host.idle();
    assert_eq!(host.focus(), Some(editor()));
    assert!(!scene.controls[0].enabled());
    assert!(scene.controls.iter().skip(1).all(Response::enabled));
}

#[test]
fn no_regions_empty_declarations_and_disappeared_pages_keep_the_native_adapter_fallback() {
    for (render_scope, empty) in [(false, false), (true, true)] {
        let mut host = Host::default();
        host.model.render_scope = render_scope;
        host.model.empty = empty;
        let scene = host.idle();
        host.pad(GamepadState {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(
            host.focus(),
            Some(scene.header.id),
            "no-focus fallback must reach a native target"
        );
        host.pad(GamepadState {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(host.model.clicks, 1);
    }
    let mut host = Host::default();
    host.open();
    host.model.render_scope = false;
    let scene = host.idle();
    scene.header.request_focus();
    host.idle();
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(host.model.clicks, 1);
}

#[test]
fn a_non_root_viewport_is_native_and_cannot_route_controller_actions_into_root_regions() {
    let mut host = Host::default();
    host.open();
    host.pad(GamepadState {
        next_focus: true,
        ..Default::default()
    });
    assert_eq!(host.focus(), Some(region_b()));
    let child = egui::ViewportId::from_hash_of("native-child");
    let mut adapter = NavigationInput::default();
    let mut raw = RawInput {
        viewport_id: child,
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
        ..Default::default()
    };
    raw.viewports.insert(
        child,
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            ..Default::default()
        },
    );
    adapter.apply(
        &host.ctx,
        &mut raw,
        GamepadState {
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(
        host.focus(),
        Some(region_b()),
        "pre-input routing must not enter the root's selected region"
    );
    host.ctx
        .run_ui(raw, |ui| {
            let mut engagement = FocusEngagement::new(Id::new("child-scope"));
            engagement.begin(ui, Some(Id::new("child-frame")));
            let frame = engagement.show(ui, Id::new("child-frame"), |ui, controls| {
                controls.push(ui.button("Native child action"));
            });
            assert!(!frame.response.sense.is_focusable());
            engagement.navigate(ui);
        })
        .drop_without_applying_deltas();
    let scene = host.idle();
    assert_eq!(host.focus(), Some(region_b()));
    assert!(
        scene.frames[1].sense.is_focusable(),
        "a different viewport must not switch root input mode"
    );
    assert_eq!(host.adapter.device(), InputDevice::Gamepad);
}
