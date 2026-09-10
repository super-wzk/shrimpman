mod events;

use std::time::Duration;

use egui::{Context, Event, Id, Key, RawInput, Rect, Response, pos2, vec2};
use egui_hunter::{
    Button, Dialog, DialogState, Direction, FocusGroup, GamepadState, InputDevice, NavigationInput,
    Popup, Property, RichTooltip, TextField, Theme, Validation,
    primitives::{focus::focus_on_click, layout::scroll_keyboard},
    properties,
};
use events::{key, pointer};

fn frame<R>(ctx: &Context, raw: RawInput, mut show: impl FnMut(&mut egui::Ui) -> R) -> R {
    let mut result = None;
    let output = ctx.run_ui(raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| result = Some(show(ui)));
    });
    output.drop_without_applying_deltas();
    result.unwrap()
}

fn input(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
        time: Some(time),
        events,
        ..Default::default()
    }
}

fn grid(ui: &mut egui::Ui, wrap: bool) -> Vec<Response> {
    let mut controls = Vec::new();
    egui::Grid::new("items").show(ui, |ui| {
        for index in 0..8 {
            controls.push(ui.add_enabled(
                index != 1 && index != 3,
                Button::new("物品").id(Id::new(index)),
            ));
            if index % 3 == 2 {
                ui.end_row();
            }
        }
    });
    ui.add(Button::new("网格外").id(Id::new("outside")));
    FocusGroup::grid(3).wrap(wrap).navigate(ui, &controls);
    controls
}

#[test]
fn grid_skips_disabled_cells_clamps_edges_and_wraps_in_the_same_column() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    frame(&ctx, input(0.0, vec![]), |ui| grid(ui, false));
    ctx.memory_mut(|m| m.request_focus(Id::new(0)));
    frame(&ctx, input(0.1, vec![key(Key::ArrowRight)]), |ui| {
        grid(ui, false)
    });
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new(2)));
    frame(&ctx, input(0.2, vec![key(Key::ArrowDown)]), |ui| {
        grid(ui, false)
    });
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new(5)));
    frame(&ctx, input(0.3, vec![key(Key::ArrowDown)]), |ui| {
        grid(ui, false)
    });
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(Id::new(5)),
        "ragged row must not escape the grid"
    );
    frame(&ctx, input(0.4, vec![key(Key::ArrowDown)]), |ui| {
        grid(ui, true)
    });
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new(2)));
    ctx.memory_mut(|m| m.request_focus(Id::new(0)));
    frame(&ctx, input(0.5, vec![key(Key::ArrowDown)]), |ui| {
        grid(ui, false)
    });
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new(6)));
    frame(&ctx, input(0.6, vec![key(Key::Tab)]), |ui| grid(ui, false));
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(Id::new(7)),
        "Tab retains native order"
    );
}

#[test]
fn vertical_groups_keep_their_edges_but_leave_horizontal_navigation_native() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let first = Id::new("first");
    let last = Id::new("last");
    let outside = Id::new("outside");
    let mut draw = |ui: &mut egui::Ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let controls = [
                    ui.add(Button::new("first").id(first)),
                    ui.add(Button::new("last").id(last)),
                ];
                FocusGroup::vertical().navigate(ui, &controls);
            });
            ui.add(Button::new("outside").id(outside));
        });
    };
    frame(&ctx, input(0.0, vec![]), &mut draw);
    ctx.memory_mut(|memory| memory.request_focus(first));
    frame(&ctx, input(0.1, vec![key(Key::ArrowRight)]), &mut draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(outside));
    frame(&ctx, input(0.2, vec![key(Key::ArrowLeft)]), &mut draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(first));
    frame(&ctx, input(0.3, vec![key(Key::ArrowDown)]), &mut draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(last));
    frame(&ctx, input(0.4, vec![key(Key::ArrowDown)]), &mut draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(last));
    frame(&ctx, input(0.5, vec![key(Key::Tab)]), &mut draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(outside));
}

#[test]
fn navigation_leaves_text_cursor_keys_and_disabled_groups_alone() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("name");
    let mut value = String::new();
    let mut draw = |ui: &mut egui::Ui| {
        ui.add(TextField::new(id, &mut value).label("姓名"));
        let buttons = [ui.add(Button::new("第一项")), ui.add(Button::new("第二项"))];
        FocusGroup::vertical().navigate(ui, &buttons);
    };
    frame(&ctx, input(0.0, vec![]), &mut draw);
    ctx.memory_mut(|m| m.request_focus(id));
    frame(
        &ctx,
        input(0.1, vec![Event::Text("猎人".into())]),
        &mut draw,
    );
    frame(&ctx, input(0.2, vec![key(Key::ArrowLeft)]), &mut draw);
    frame(&ctx, input(0.3, vec![Event::Text("小".into())]), &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(id));
    assert_eq!(value, "猎小人");
    frame(&ctx, input(0.4, vec![]), |ui| {
        ui.add_enabled_ui(false, |ui| {
            let controls = [ui.add(Button::new("禁用"))];
            assert!(FocusGroup::vertical().navigate(ui, &controls).is_none());
        });
        assert!(FocusGroup::grid(0).navigate(ui, &[]).is_none());
    });
}

#[test]
fn controller_repeats_directions_but_never_confirmation_and_releases_native_keys() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut adapter = NavigationInput::default()
        .repeat_timing(Duration::from_millis(300), Duration::from_millis(100));
    let mut clicks = 0;
    let mut draw = |ui: &mut egui::Ui| {
        for index in 0..3 {
            let response = ui.add(Button::new("确认").id(Id::new(index)));
            clicks += usize::from(response.clicked());
        }
    };
    frame(&ctx, input(0.0, vec![]), &mut draw);
    // The first controller direction enters the native focus order.
    let down = GamepadState {
        direction: Some(Direction::Down),
        ..Default::default()
    };
    let mut raw = input(0.1, vec![]);
    adapter.apply(&ctx, &mut raw, down);
    frame(&ctx, raw, &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new(0)));
    for (time, expected) in [(0.2, 0), (0.41, 1), (0.52, 2)] {
        let mut raw = input(time, vec![]);
        adapter.apply(&ctx, &mut raw, down);
        frame(&ctx, raw, &mut draw);
        assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new(expected)));
        assert!(!ctx.input(|i| i.key_down(Key::ArrowDown)));
    }
    assert_eq!(adapter.device(), InputDevice::Gamepad);
    let confirm = GamepadState {
        confirm: true,
        ..Default::default()
    };
    for time in [0.6, 0.8, 1.2] {
        let mut raw = input(time, vec![]);
        adapter.apply(&ctx, &mut raw, confirm);
        frame(&ctx, raw, &mut draw);
    }
    assert_eq!(clicks, 1);
    let mut raw = input(1.3, vec![Event::PointerMoved(pos2(5.0, 5.0))]);
    adapter.apply(&ctx, &mut raw, GamepadState::default());
    assert_eq!(adapter.device(), InputDevice::Gamepad);
    let mut raw = input(
        1.4,
        vec![Event::PointerButton {
            pos: pos2(5.0, 5.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    adapter.apply(&ctx, &mut raw, GamepadState::default());
    assert_eq!(adapter.device(), InputDevice::KeyboardMouse);
}

#[test]
fn controller_does_not_release_physical_keys_or_activate_after_window_focus_returns() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("confirm");
    frame(&ctx, input(0.0, vec![]), |ui| {
        ui.add(Button::new("确认").id(id))
    });
    ctx.memory_mut(|m| m.request_focus(id));
    let mut adapter = NavigationInput::default();
    let held = GamepadState {
        confirm: true,
        ..Default::default()
    };
    let mut raw = input(0.1, vec![key(Key::Enter)]);
    adapter.apply(&ctx, &mut raw, held);
    assert_eq!(
        raw.events.len(),
        1,
        "do not append a synthetic release of Enter"
    );
    let mut raw = input(0.2, vec![]);
    raw.focused = false;
    adapter.apply(&ctx, &mut raw, held);
    assert!(raw.events.is_empty());
    let mut raw = input(0.3, vec![]);
    adapter.apply(&ctx, &mut raw, held);
    assert!(raw.events.is_empty());
    adapter.apply(&ctx, &mut raw, GamepadState::default());
    adapter.apply(&ctx, &mut raw, held);
    assert!(raw.events.iter().any(|event| matches!(
        event,
        Event::Key {
            key: Key::Enter,
            pressed: true,
            ..
        }
    )));
}

#[test]
fn field_validation_changes_preserve_focus_and_read_only_and_disabled_values() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("field");
    let mut value = String::new();
    frame(&ctx, input(0.0, vec![]), |ui| {
        ui.add(
            TextField::new(id, &mut value)
                .label("名字")
                .validation(Validation::Error("必填")),
        )
    });
    ctx.memory_mut(|m| m.request_focus(id));
    let response = frame(&ctx, input(0.1, vec![Event::Text("猎人".into())]), |ui| {
        ui.add(
            TextField::new(id, &mut value)
                .label("名字")
                .help("可使用中文"),
        )
    });
    assert!(response.changed());
    assert!(response.has_focus());
    let response = frame(
        &ctx,
        input(
            0.2,
            vec![
                Event::Text("修改".into()),
                Event::Paste("粘贴".into()),
                key(Key::Backspace),
            ],
        ),
        |ui| ui.add(TextField::new(id, &mut value).read_only(true)),
    );
    assert_eq!(value, "猎人");
    assert!(response.enabled(), "read-only remains selectable");
    let response = frame(&ctx, input(0.3, vec![Event::Text("修改".into())]), |ui| {
        ui.add_enabled(false, TextField::new(id, &mut value))
    });
    assert_eq!(value, "猎人");
    assert!(!response.enabled());
    assert!(!response.changed());
}

#[test]
fn rich_tooltip_opens_on_focus_without_taking_it_or_opening_a_menu() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("anchor");
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new("详细属性").id(id));
        RichTooltip::new(&anchor, "装备资料").show(|ui| {
            properties(ui, &[Property::new("攻击力", "120 +12")]);
            42
        })
    };
    assert!(frame(&ctx, input(0.0, vec![]), &mut draw).is_none());
    ctx.memory_mut(|m| m.request_focus(id));
    let tooltip = frame(&ctx, input(0.1, vec![]), &mut draw).unwrap();
    assert_eq!(tooltip.inner, 42);
    assert_eq!(ctx.memory(|m| m.focused()), Some(id));
    assert!(!egui::Popup::is_any_open(&ctx));
    ctx.memory_mut(|m| m.surrender_focus(id));
    assert!(frame(&ctx, input(0.2, vec![]), &mut draw).is_none());
}

#[test]
fn long_properties_and_validation_wrap_within_a_narrow_parent() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut value = String::new();
    let (wide, narrow, field) = frame(&ctx, input(0.0, vec![]), |ui| {
        let rows = [Property::new(
            "限制条件",
            "Complete the expedition before returning to the gathering hall.",
        )];
        let wide = properties(ui, &rows);
        let (narrow, field) = ui
            .scope(|ui| {
                ui.set_width(190.0);
                let narrow = properties(ui, &rows);
                let field = ui.add(
                    TextField::new(Id::new("narrow"), &mut value)
                        .label("姓名")
                        .validation(Validation::Error(
                            "This name is already in use. Please choose another hunter name.",
                        )),
                );
                (narrow, field)
            })
            .inner;
        (wide, narrow, field)
    });
    assert!(narrow.rect.width() <= 190.1);
    assert!(narrow.rect.height() > wide.rect.height());
    assert!(field.rect.width() <= 190.1);
}

#[test]
fn controller_cancel_closes_the_popup_then_the_dialog_on_separate_presses() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut adapter = NavigationInput::default();
    let mut dialog = DialogState::default();
    let popup = Id::new("popup");
    dialog.open(&ctx);
    egui::Popup::open_id(&ctx, popup);
    let mut draw = |ui: &mut egui::Ui| {
        Dialog::new(Id::new("dialog"), "确认").show(ui.ctx(), &mut dialog, |ui| {
            let anchor = ui.add(Button::new("更多"));
            let mut popup_view = Popup::new(&anchor).initial_focus(Id::new("popup-action"));
            popup_view.native = popup_view.native.id(popup);
            popup_view.show(|ui| {
                ui.add(Button::new("操作").id(Id::new("popup-action")));
            });
        });
        (dialog.is_open(), egui::Popup::is_id_open(ui.ctx(), popup))
    };
    frame(&ctx, input(0.0, vec![]), &mut draw);
    frame(&ctx, input(0.1, vec![]), &mut draw);
    let cancel = GamepadState {
        cancel: true,
        ..Default::default()
    };
    for (time, state, expected) in [
        (0.2, cancel, (true, false)),
        (0.6, cancel, (true, false)),
        (0.7, GamepadState::default(), (true, false)),
        (0.8, cancel, (false, false)),
    ] {
        let mut raw = input(time, vec![]);
        adapter.apply(&ctx, &mut raw, state);
        assert_eq!(frame(&ctx, raw, &mut draw), expected);
    }
}

#[test]
fn focused_tooltip_fits_a_narrow_viewport() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("edge-anchor");
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 240.0));
    let mut bounds = Rect::NOTHING;
    for index in 0..3 {
        let mut raw = input(f64::from(index) * 0.1, vec![]);
        raw.screen_rect = Some(viewport);
        bounds = frame(&ctx, raw, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                let anchor = ui.add(Button::new("详细信息").id(id));
                anchor.request_focus();
                RichTooltip::new(&anchor, "装备资料")
                    .width(1000.0)
                    .show(|ui| {
                        ui.label("A description that wraps inside a narrow tooltip.");
                    })
                    .unwrap()
                    .response
                    .rect
            })
            .inner
        });
    }
    assert!(viewport.expand(1.0).contains_rect(bounds), "{bounds:?}");
    assert_eq!(ctx.memory(|m| m.focused()), Some(id));
}

#[test]
fn focused_scroll_viewport_scrolls_while_down_is_held_without_repeat_events() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let focus = Id::new("viewport");
    let mut draw = |ui: &mut egui::Ui| {
        let scope = ui.scope_builder(
            egui::UiBuilder::new().id(focus).sense(egui::Sense::click()),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("held-scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for index in 0..100 {
                            ui.label(format!("Archive {index}"));
                        }
                        scroll_keyboard(ui, focus);
                    })
            },
        );
        focus_on_click(&scope.response);
        scope
    };
    let initial = frame(&ctx, input(0.0, vec![]), &mut draw);
    let focus = initial.response.id;
    ctx.memory_mut(|m| m.request_focus(focus));
    let first = frame(&ctx, input(0.1, vec![key(Key::ArrowDown)]), &mut draw);
    let second = frame(&ctx, input(0.2, vec![]), &mut draw);
    let third = frame(&ctx, input(0.3, vec![]), &mut draw);
    // Native ScrollArea applies scroll targets at the next pass, even for a
    // zero-duration animation. Holding needs no extra OS key-repeat events.
    assert!(second.inner.state.offset.y > first.inner.state.offset.y);
    assert!(third.inner.state.offset.y > second.inner.state.offset.y);
    assert_eq!(ctx.memory(|m| m.focused()), Some(focus));
    let mut release = key(Key::ArrowDown);
    if let Event::Key { pressed, .. } = &mut release {
        *pressed = false;
    }
    let stopped = frame(&ctx, input(0.4, vec![release]), &mut draw);
    let still = frame(&ctx, input(0.5, vec![]), &mut draw);
    assert_eq!(stopped.inner.state.offset.y, still.inner.state.offset.y);
    assert!(still.inner.state.offset.y >= third.inner.state.offset.y);
}

#[test]
fn scroll_viewport_does_not_steal_child_clicks_or_text_arrows() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut value = String::new();
    let id = Id::new("scroll-editor");
    let focus = Id::new("viewport");
    let mut draw = |ui: &mut egui::Ui| {
        let scope = ui.scope_builder(
            egui::UiBuilder::new().id(focus).sense(egui::Sense::click()),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("editor-scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(TextField::new(id, &mut value));
                        let button = ui.add(Button::new("Child"));
                        ui.add_space(400.0);
                        scroll_keyboard(ui, focus);
                        button
                    })
            },
        );
        focus_on_click(&scope.response);
        scope
    };
    frame(&ctx, input(0.0, vec![]), &mut draw);
    ctx.memory_mut(|m| m.request_focus(id));
    frame(
        &ctx,
        input(0.1, vec![Event::Text("hunter".into())]),
        &mut draw,
    );
    frame(&ctx, input(0.2, vec![key(Key::ArrowLeft)]), &mut draw);
    let output = frame(&ctx, input(0.3, vec![key(Key::ArrowDown)]), &mut draw);
    assert_eq!(output.inner.state.offset.y, 0.0);
    assert_eq!(ctx.memory(|m| m.focused()), Some(id));
    let pos = output.inner.inner.rect.center();
    for pressed in [true, false] {
        let events = pointer(pos, pressed);
        let output = frame(
            &ctx,
            input(if pressed { 0.4 } else { 0.5 }, events),
            &mut draw,
        );
        if !pressed {
            assert!(output.inner.inner.clicked());
            assert_ne!(ctx.memory(|m| m.focused()), Some(output.response.id));
        }
    }
}
