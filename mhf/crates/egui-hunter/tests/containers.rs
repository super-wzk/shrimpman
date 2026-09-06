mod events;

use std::time::Duration;

use egui::{Context, Event, Id, Key, PopupCloseBehavior, RawInput, Rect, Response, pos2, vec2};
use egui_hunter::{
    Button, Dialog, DialogState, Icon, NavigationStack, NavigationState, NoticeKind, Notifications,
    Panel, Popup, ResponsiveColumns, Tab, Tabs, TextField, Theme, Window,
};
use events::{key, pointer};

fn frame<R>(
    ctx: &Context,
    width: f32,
    time: f64,
    events: Vec<Event>,
    mut show: impl FnMut(&mut egui::Ui) -> R,
) -> R {
    let mut result = None;
    let output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 700.0))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                result = Some(show(ui));
            });
        },
    );
    output.drop_without_applying_deltas();
    result.unwrap()
}

#[derive(Default)]
struct Overlays {
    dialog: DialogState,
    nested: DialogState,
    open: bool,
    open_popup: bool,
    open_nested: bool,
    accepts: usize,
}

impl Overlays {
    fn show(&mut self, ui: &mut egui::Ui) -> Response {
        let ctx = ui.ctx().clone();
        let opener = ui.add(Button::new("open").id(Id::new("opener")));
        if self.open {
            self.dialog.open_from(&opener);
            self.open = false;
        }
        Dialog::new(Id::new("dialog"), "Dialog")
            .initial_focus(Id::new("confirm"))
            .show(&ctx, &mut self.dialog, |ui| {
                let confirm = ui.add(Button::new("confirm").id(Id::new("confirm")));
                if confirm.clicked() {
                    self.accepts += 1;
                    ui.close();
                }
                let anchor = ui.add(Button::new("popup").id(Id::new("popup-anchor")));
                if self.open_popup {
                    egui::Popup::open_id(&ctx, egui::Popup::default_response_id(&anchor));
                    self.open_popup = false;
                }
                Popup::new(&anchor)
                    .initial_focus(Id::new("popup-item"))
                    .show(|ui| {
                        ui.add(Button::new("item").id(Id::new("popup-item")));
                    });
                let nested = ui.add(Button::new("nested").id(Id::new("nested-opener")));
                if self.open_nested {
                    self.nested.open_from(&nested);
                    self.open_nested = false;
                }
            });
        Dialog::new(Id::new("nested"), "Nested")
            .initial_focus(Id::new("nested-confirm"))
            .show(&ctx, &mut self.nested, |ui| {
                ui.add(Button::new("close").id(Id::new("nested-confirm")));
            });
        opener
    }

    fn frame(&mut self, ctx: &Context, events: Vec<Event>) -> Response {
        frame(ctx, 800.0, 0.0, events, |ui| self.show(ui))
    }
}

#[test]
fn dialog_enters_with_focus_accepts_once_and_restores_opener() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut app = Overlays {
        open: true,
        ..Default::default()
    };
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("confirm")));
    app.frame(&ctx, vec![key(Key::Enter)]);
    assert!(!app.dialog.is_open());
    assert_eq!(app.accepts, 1);
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("opener")));
    app.frame(&ctx, vec![]);
    assert_eq!(app.accepts, 1);
}

#[test]
fn escape_closes_popup_before_its_dialog() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut app = Overlays {
        open: true,
        open_popup: true,
        ..Default::default()
    };
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("popup-item")));
    app.frame(&ctx, vec![]);
    assert!(egui::Popup::is_any_open(&ctx));
    app.frame(&ctx, vec![key(Key::Escape)]);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(app.dialog.is_open());
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("popup-anchor")));
    app.frame(
        &ctx,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    app.frame(&ctx, vec![key(Key::Escape)]);
    assert!(!app.dialog.is_open());
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("opener")));
}

#[test]
fn clicking_another_input_closes_popup_without_stealing_its_focus() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut text = String::new();
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new("anchor").id(Id::new("anchor")));
        ui.add_space(300.0);
        let input = ui.add(
            TextField::new(Id::new("input"), &mut text)
                .hint("type here")
                .icon(Icon::Search),
        );
        Popup::new(&anchor).show(|ui| {
            ui.label("contents");
        });
        (anchor.rect.center(), input.rect.center())
    };
    let (anchor, input) = frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    frame(&ctx, 800.0, 0.1, pointer(anchor, true), &mut draw);
    frame(&ctx, 800.0, 0.2, pointer(anchor, false), &mut draw);
    assert!(egui::Popup::is_any_open(&ctx));
    frame(&ctx, 800.0, 0.3, vec![], &mut draw);
    frame(&ctx, 800.0, 0.4, pointer(input, true), &mut draw);
    frame(&ctx, 800.0, 0.5, pointer(input, false), &mut draw);
    frame(&ctx, 800.0, 0.6, vec![Event::Text("x".into())], &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("input")));
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(text, "x");
}

#[test]
fn switching_popup_anchors_keeps_only_the_new_popup_open() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let first = Id::new("first-popup");
    let second = Id::new("second-popup");
    egui::Popup::open_id(&ctx, first);
    let mut draw = |ui: &mut egui::Ui| {
        let first_anchor = ui.add(Button::new("first"));
        let mut popup = Popup::new(&first_anchor);
        popup.native = popup.native.id(first);
        popup.show(|ui| {
            ui.label("first contents");
        });
        ui.add_space(300.0);
        let second_anchor = ui.add(Button::new("second"));
        let mut popup = Popup::new(&second_anchor).initial_focus(Id::new("second-item"));
        popup.native = popup.native.id(second);
        popup.show(|ui| {
            ui.add(Button::new("second item").id(Id::new("second-item")));
        });
        second_anchor.rect.center()
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let pos = frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    frame(&ctx, 800.0, 0.2, pointer(pos, true), &mut draw);
    frame(&ctx, 800.0, 0.3, pointer(pos, false), &mut draw);
    assert!(!egui::Popup::is_id_open(&ctx, first));
    assert!(egui::Popup::is_id_open(&ctx, second));
    frame(&ctx, 800.0, 0.4, vec![], &mut draw);
    assert!(!egui::Popup::is_id_open(&ctx, first));
    assert!(egui::Popup::is_id_open(&ctx, second));
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("second-item")));
}

#[test]
fn native_popup_commands_control_the_styled_popup_and_restore_focus_once() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("popup");
    let anchor_id = Id::new("anchor");
    let other_id = Id::new("other");
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new("anchor").id(anchor_id));
        ui.add(Button::new("other").id(other_id));
        let mut popup = Popup::new(&anchor).initial_focus(Id::new("item"));
        popup.native = popup.native.id(id);
        popup
            .show(|ui| {
                ui.add(Button::new("item").id(Id::new("item")));
            })
            .is_some()
    };
    assert!(!frame(&ctx, 800.0, 0.0, vec![], &mut draw));
    egui::Popup::open_id(&ctx, id);
    assert!(frame(&ctx, 800.0, 0.1, vec![], &mut draw));
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("item")));
    egui::Popup::close_id(&ctx, id);
    assert!(!egui::Popup::is_id_open(&ctx, id));
    assert!(!frame(&ctx, 800.0, 0.2, vec![], &mut draw));
    frame(&ctx, 800.0, 0.3, vec![], &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(anchor_id));
    ctx.memory_mut(|memory| memory.request_focus(other_id));
    frame(&ctx, 800.0, 0.4, vec![], &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(other_id));
    egui::Popup::open_id(&ctx, id);
    frame(&ctx, 800.0, 0.5, vec![], &mut draw);
    frame(&ctx, 800.0, 0.6, vec![key(Key::Escape)], &mut draw);
    assert!(!egui::Popup::is_id_open(&ctx, id));
    egui::Popup::open_id(&ctx, id);
    frame(&ctx, 800.0, 0.7, vec![], &mut draw);
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(Id::new("item")),
        "immediate reopening must give initial focus again"
    );
}

#[test]
fn popup_does_not_restore_stale_focus_when_its_parent_returns() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("popup");
    let other_id = Id::new("other");
    let draw = |ui: &mut egui::Ui, close| {
        let anchor = ui.add(Button::new("anchor"));
        let mut popup = Popup::new(&anchor);
        popup.native = popup.native.id(id);
        popup.show(|ui| {
            ui.label("item");
            if close {
                ui.close();
            }
        });
        ui.add(Button::new("other").id(other_id));
    };
    egui::Popup::open_id(&ctx, id);
    frame(&ctx, 800.0, 0.0, vec![], |ui| draw(ui, false));
    frame(&ctx, 800.0, 0.1, vec![], |ui| draw(ui, true));
    frame(&ctx, 800.0, 0.2, vec![], |ui| {
        ui.add(Button::new("other").id(other_id)).request_focus();
    });
    frame(&ctx, 800.0, 0.3, vec![], |ui| draw(ui, false));
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(other_id));
}

#[test]
fn selecting_a_popup_value_returns_focus_before_the_next_tab() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let anchor_id = Id::new("choice-anchor");
    let popup_id = Id::new("choice-popup");
    let after_id = Id::new("after-choice");
    let mut selected = 0;
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new(["First", "Second"][selected]).id(anchor_id));
        let mut popup = Popup::new(&anchor).initial_focus(popup_id.with(selected));
        popup.native = popup.native.id(popup_id);
        popup.show(|ui| {
            for (index, label) in ["First", "Second"].into_iter().enumerate() {
                if ui
                    .add(
                        Button::new(label)
                            .id(popup_id.with(index))
                            .selected(selected == index),
                    )
                    .clicked()
                {
                    selected = index;
                    ui.close();
                }
            }
        });
        ui.add(Button::new("Next").id(after_id));
        selected
    };
    let pulse = |key| {
        [true, false]
            .map(|pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .to_vec()
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    ctx.memory_mut(|memory| memory.request_focus(anchor_id));
    frame(&ctx, 800.0, 0.1, pulse(Key::Enter), &mut draw);
    assert!(egui::Popup::is_id_open(&ctx, popup_id));
    frame(&ctx, 800.0, 0.2, vec![], &mut draw);
    frame(&ctx, 800.0, 0.3, pulse(Key::ArrowDown), &mut draw);
    let selected = frame(&ctx, 800.0, 0.4, pulse(Key::Enter), &mut draw);
    assert_eq!(selected, 1);
    assert!(!egui::Popup::is_id_open(&ctx, popup_id));
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(anchor_id));
    frame(&ctx, 800.0, 0.5, pulse(Key::Tab), &mut draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(after_id));
}

#[test]
fn replaced_popup_does_not_consume_escape_for_the_new_popup() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let first = Id::new("first-popup");
    let second = Id::new("second-popup");
    egui::Popup::open_id(&ctx, first);
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new("first"));
        let mut popup = Popup::new(&anchor);
        popup.native = popup.native.id(first);
        popup.show(|ui| {
            ui.label("first");
        });
        let anchor = ui.add(Button::new("second"));
        let mut popup = Popup::new(&anchor);
        popup.native = popup.native.id(second);
        popup.show(|ui| {
            ui.label("second");
        });
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    egui::Popup::open_id(&ctx, second);
    frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], &mut draw);
    assert!(!egui::Popup::is_any_open(&ctx));
}

#[test]
fn nested_dialog_escape_pops_one_layer_and_restores_parent_focus() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut app = Overlays {
        open: true,
        open_nested: true,
        ..Default::default()
    };
    app.frame(&ctx, vec![]);
    app.frame(&ctx, vec![]);
    app.frame(&ctx, vec![key(Key::Escape)]);
    assert!(!app.nested.is_open());
    assert!(app.dialog.is_open());
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("nested-opener")));
    app.frame(&ctx, vec![key(Key::Escape)]);
    assert!(
        app.dialog.is_open(),
        "held Escape must not close the parent"
    );
}

#[test]
fn backdrop_dismissal_can_be_disabled() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut state = DialogState::default();
    state.open(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        Dialog::new(Id::new("dialog"), "Title")
            .dismiss_on_backdrop(false)
            .show(ui.ctx(), &mut state, |ui| {
                ui.label("body");
            });
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    frame(&ctx, 800.0, 0.1, pointer(pos2(20.0, 20.0), true), &mut draw);
    frame(
        &ctx,
        800.0,
        0.2,
        pointer(pos2(20.0, 20.0), false),
        &mut draw,
    );
    assert!(state.is_open());
    frame(&ctx, 800.0, 0.3, vec![key(Key::Escape)], |ui| {
        Dialog::new(Id::new("dialog"), "Title")
            .dismiss_on_backdrop(false)
            .show(ui.ctx(), &mut state, |ui| {
                ui.label("body");
            });
    });
    assert!(!state.is_open());
}

#[test]
fn window_ui_close_updates_host_state_and_stops_rendering_content() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut open = true;
    frame(&ctx, 800.0, 0.0, vec![], |ui| {
        Window::new("Window").open(&mut open).show(ui.ctx(), |ui| {
            ui.label("body");
        });
    });
    frame(&ctx, 800.0, 0.1, vec![], |ui| {
        Window::new("Window").open(&mut open).show(ui.ctx(), |ui| {
            ui.close();
        });
    });
    assert!(!open);
    frame(&ctx, 800.0, 1.0, vec![], |ui| {
        Window::new("Window")
            .open(&mut open)
            .show(ui.ctx(), |_| panic!("closed content rendered"));
    });
}

#[test]
fn window_with_nested_panels_keeps_its_height_across_idle_frames() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut open = true;
    let mut draw = |ui: &mut egui::Ui| {
        let mut window = Window::new("Field notes").open(&mut open);
        window.native = window.native.default_size([340.0, 220.0]);
        window
            .show(ui.ctx(), |ui| {
                ui.label("A brief introduction");
                Panel::new("Nested panel").show(ui, |ui| {
                    ui.label("one");
                    ui.label("two");
                    ui.label("three");
                });
            })
            .unwrap()
            .response
            .rect
    };
    frame(&ctx, 1000.0, 0.0, vec![], &mut draw);
    let settled = frame(&ctx, 1000.0, 0.1, vec![], &mut draw);
    for index in 2..12 {
        let rect = frame(&ctx, 1000.0, index as f64 * 0.1, vec![], &mut draw);
        assert!(
            (rect.height() - settled.height()).abs() < 1.0,
            "frame {index}: initial={settled:?}, actual={rect:?}"
        );
    }
}

#[test]
fn responsive_columns_reflow_without_changing_child_identity() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        ResponsiveColumns::new(Id::new("columns"))
            .min_column_width(300.0)
            .show(ui, 3, |ui, _| ui.add(Button::new("item").full_width()))
            .inner
    };
    let wide = frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let narrow = frame(&ctx, 420.0, 0.1, vec![], &mut draw);
    assert!((wide[0].rect.top() - wide[1].rect.top()).abs() < 1.0);
    assert!(wide[2].rect.top() > wide[0].rect.bottom());
    assert!(narrow[1].rect.top() > narrow[0].rect.bottom());
    for (wide, narrow) in wide.iter().zip(&narrow) {
        assert_eq!(wide.id, narrow.id);
        assert!(narrow.rect.right() <= 420.0);
    }
    frame(&ctx, 420.0, 0.2, vec![], |ui| {
        let output = ResponsiveColumns::new(Id::new("empty"))
            .show(ui, 0, |_, _| panic!("empty layout invoked"));
        assert!(output.inner.is_empty());
    });
}

#[test]
fn tabs_skip_disabled_headers_preserve_content_identity_and_leave_text_arrows_alone() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("tabs");
    let first = Id::new("first");
    let last = Id::new("last");
    let tabs = [
        Tab::new(first, "first"),
        Tab::new(Id::new("disabled"), "disabled").enabled(false),
        Tab::new(last, "last"),
    ];
    let mut state = NavigationState::default();
    let mut text = String::from("abc");
    let mut draw = |ui: &mut egui::Ui| {
        Tabs::new(id)
            .show(ui, &mut state, &tabs, |ui, _| {
                ui.text_edit_singleline(&mut text)
            })
            .inner
            .unwrap()
            .id
    };
    let first_input = frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    ctx.memory_mut(|m| m.request_focus(id.with(("header", first))));
    let last_input = frame(&ctx, 800.0, 0.1, vec![key(Key::ArrowRight)], &mut draw);
    assert_ne!(first_input, last_input);
    assert_eq!(ctx.memory(|m| m.focused()), Some(id.with(("header", last))));
    ctx.memory_mut(|m| m.request_focus(last_input));
    assert_eq!(
        frame(&ctx, 800.0, 0.2, vec![key(Key::ArrowLeft)], &mut draw),
        last_input
    );
    ctx.memory_mut(|m| m.request_focus(id.with(("header", last))));
    assert_eq!(
        frame(&ctx, 800.0, 0.3, vec![key(Key::Home)], &mut draw),
        first_input
    );
    assert_eq!(state.selected(), Some(first));
    frame(&ctx, 800.0, 0.4, vec![], |ui| {
        Tabs::new(id).show(ui, &mut state, &tabs[2..], |_, page| assert_eq!(page, last));
    });
    assert_eq!(state.selected(), Some(last));
    frame(&ctx, 800.0, 0.5, vec![], |ui| {
        Tabs::new(id).show(ui, &mut state, &tabs[1..2], |_, _| {
            panic!("disabled content invoked")
        });
    });
    assert_eq!(state.selected(), None);
}

#[test]
fn tabs_move_selection_and_focus_once_in_both_directions_and_at_wrap() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("tabs");
    let ids = [Id::new("first"), Id::new("second"), Id::new("third")];
    let tabs = [
        Tab::new(ids[0], "first"),
        Tab::new(Id::new("disabled"), "disabled").enabled(false),
        Tab::new(ids[1], "second"),
        Tab::new(ids[2], "third"),
    ];
    let mut state = NavigationState::default();
    let draw = |ui: &mut egui::Ui, state: &mut NavigationState| {
        Tabs::new(id).show(ui, state, &tabs, |ui, _| {
            ui.label("content");
        });
    };
    frame(&ctx, 800.0, 0.0, vec![], |ui| draw(ui, &mut state));
    ctx.memory_mut(|memory| memory.request_focus(id.with(("header", ids[0]))));
    for (step, (key, expected)) in [
        (Key::ArrowRight, 1),
        (Key::ArrowRight, 2),
        (Key::ArrowRight, 0),
        (Key::ArrowLeft, 2),
        (Key::Home, 0),
        (Key::End, 2),
        (Key::ArrowLeft, 1),
    ]
    .into_iter()
    .enumerate()
    {
        frame(
            &ctx,
            800.0,
            (step + 1) as f64 * 0.1,
            vec![events::key(key)],
            |ui| draw(ui, &mut state),
        );
        assert_eq!(state.selected(), Some(ids[expected]), "{key:?}");
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(id.with(("header", ids[expected]))),
            "{key:?} must not trigger another geometric focus move"
        );
    }
}

#[test]
fn menu_back_preserves_root_and_restores_parent_control_after_render() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut menu = NavigationStack::new(Id::new("menu"), 0_u8);
    let root = frame(&ctx, 800.0, 0.0, vec![], |ui| {
        menu.show(ui, |ui, _| ui.add(Button::new("next"))).inner
    });
    menu.push_from(&root, 1);
    frame(&ctx, 800.0, 0.1, vec![], |ui| {
        menu.show(ui, |ui, _| ui.label("child"));
    });
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], |ui| {
        menu.show(ui, |ui, _| ui.add(Button::new("next")));
    });
    assert_eq!(*menu.current(), 0);
    frame(&ctx, 800.0, 0.3, vec![], |ui| {
        menu.show(ui, |ui, _| ui.add(Button::new("next")));
    });
    assert_eq!(ctx.memory(|m| m.focused()), Some(root.id));
    assert!(!menu.back(&ctx));
    assert_eq!(menu.depth(), 1);
}

#[test]
fn menu_escape_waits_for_modal_to_close() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut menu = NavigationStack::new(Id::new("menu"), 0_u8);
    menu.push(&ctx, 1);
    let mut app = Overlays {
        open: true,
        ..Default::default()
    };
    let mut draw = |ui: &mut egui::Ui| {
        menu.show(ui, |ui, _| {
            ui.label("child");
        });
        app.show(ui);
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], &mut draw);
    assert!(!app.dialog.is_open());
    assert_eq!(menu.depth(), 2);
}

#[test]
fn controller_cancel_only_reaches_the_current_engagement_region() {
    use egui_hunter::{EngagementPlugin, FocusEngagement, GamepadState, NavigationInput};

    let ctx = Context::default();
    Theme::default().apply(&ctx);
    ctx.add_plugin(EngagementPlugin::default());
    let mut adapter = NavigationInput::default();
    let mut menu = NavigationStack::new(Id::new("scoped-menu"), 0_u8);
    menu.push(&ctx, 1);
    let mut draw = |ui: &mut egui::Ui| {
        let mut engagement = FocusEngagement::new(Id::new("menu-engagement"));
        engagement.begin(ui, None);
        engagement.show(ui, Id::new("menu-region"), |ui, controls| {
            controls.push(
                menu.show(ui, |ui, page| {
                    ui.add(Button::new("Menu action").id(Id::new(("menu-action", *page))))
                })
                .inner,
            );
        });
        engagement.show(ui, Id::new("other-region"), |ui, controls| {
            controls.push(ui.add(Button::new("Other action").id(Id::new("other-action"))));
        });
        engagement.navigate(ui);
        menu.depth()
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let mut time = 0.0;
    let mut send = |state| {
        time += 0.1;
        let mut raw = RawInput {
            time: Some(time),
            ..Default::default()
        };
        adapter.apply(&ctx, &mut raw, state);
        frame(&ctx, 800.0, time, raw.events, &mut draw)
    };
    let cancel = GamepadState {
        cancel: true,
        ..Default::default()
    };

    ctx.memory_mut(|memory| memory.request_focus(Id::new("other-action")));
    send(GamepadState::default());
    assert_eq!(send(cancel), 2, "another region's menu must not receive B");
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(Id::new("other-region"))
    );

    send(GamepadState::default());
    ctx.memory_mut(|memory| memory.request_focus(Id::new(("menu-action", 1_u8))));
    send(GamepadState::default());
    assert_eq!(
        send(cancel),
        1,
        "the engaged menu handles B before region exit"
    );
}

#[test]
fn menu_inside_modal_returns_before_closing_its_host() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut menu = NavigationStack::new(Id::new("modal-menu"), 0_u8);
    menu.push(&ctx, 1);
    let mut dialog = DialogState::default();
    dialog.open(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        Dialog::new(Id::new("menu-host"), "Menu")
            .initial_focus(Id::new("menu-action"))
            .show(ui.ctx(), &mut dialog, |ui| {
                menu.show(ui, |ui, _| {
                    ui.add(Button::new("Action").id(Id::new("menu-action")));
                });
            });
        (menu.depth(), dialog.is_open())
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    assert_eq!(
        frame(&ctx, 800.0, 0.2, pulse(Key::Escape), &mut draw),
        (1, true)
    );
    assert_eq!(
        frame(&ctx, 800.0, 0.3, pulse(Key::Escape), &mut draw),
        (1, false)
    );
}

fn pulse(key: Key) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .into()
}

#[test]
fn menu_escape_closes_a_native_popup_without_also_popping_the_page() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut menu = NavigationStack::new(Id::new("menu"), 0_u8);
    menu.push(&ctx, 1);
    let id = Id::new("native-popup");
    egui::Popup::open_id(&ctx, id);
    let mut draw = |ui: &mut egui::Ui| {
        menu.show(ui, |ui, _| {
            let anchor = ui.button("native popup");
            egui::Popup::from_response(&anchor)
                .id(id)
                .open_memory(None)
                .show(|ui| {
                    ui.label("inside");
                });
        });
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], &mut draw);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(menu.depth(), 2);
}

#[test]
fn menu_escape_leaves_its_editor_before_returning_to_the_parent_page() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut menu = NavigationStack::new(Id::new("menu"), 0_u8);
    let mut text = String::from("keep this");
    menu.push(&ctx, 1);
    let draw = |ui: &mut egui::Ui, menu: &mut NavigationStack<u8>, text: &mut String| {
        menu.show(ui, |ui, _| {
            ui.add(
                TextField::new(Id::new("editor"), text)
                    .hint("")
                    .icon(Icon::Search),
            )
        })
        .inner
    };
    frame(&ctx, 800.0, 0.0, vec![], |ui| {
        draw(ui, &mut menu, &mut text)
    })
    .request_focus();
    frame(&ctx, 800.0, 0.1, vec![], |ui| {
        draw(ui, &mut menu, &mut text)
    });
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], |ui| {
        draw(ui, &mut menu, &mut text)
    });
    assert_eq!(menu.depth(), 2);
    assert!(ctx.memory(|m| m.focused()).is_none());
    frame(
        &ctx,
        800.0,
        0.3,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        |ui| draw(ui, &mut menu, &mut text),
    );
    frame(&ctx, 800.0, 0.4, vec![key(Key::Escape)], |ui| {
        draw(ui, &mut menu, &mut text)
    });
    assert_eq!(menu.depth(), 1);
    assert_eq!(text, "keep this");
}

#[test]
fn notifications_start_lifetime_when_shown_and_bound_the_waiting_queue() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut notices = Notifications::with_capacity(Id::new("notices"), 3);
    let first = notices.push_for(&ctx, NoticeKind::Success, "first", Duration::from_secs(2));
    let second = notices.push_for(&ctx, NoticeKind::Warning, "second", Duration::from_secs(2));
    frame(&ctx, 800.0, 100.0, vec![], |ui| {
        notices.show(ui.ctx());
    });
    assert_eq!(notices.front_id(), Some(first));
    frame(&ctx, 800.0, 102.1, vec![], |ui| {
        notices.show(ui.ctx());
    });
    assert_eq!(notices.front_id(), Some(second));
    frame(&ctx, 800.0, 103.0, vec![], |ui| {
        notices.show(ui.ctx());
    });
    assert_eq!(
        notices.front_id(),
        Some(second),
        "waiting item gets its full visible lifetime"
    );
    let old = notices.push(&ctx, NoticeKind::Success, "old waiting");
    let kept = notices.push(&ctx, NoticeKind::Success, "kept waiting");
    notices.push(&ctx, NoticeKind::Success, "newest waiting");
    assert_eq!(notices.len(), 3);
    assert!(!notices.dismiss(&ctx, old));
    assert!(notices.dismiss(&ctx, second));
    assert_eq!(notices.front_id(), Some(kept));
    notices.clear(&ctx);
    notices.push_for(&ctx, NoticeKind::Danger, "zero", Duration::ZERO);
    frame(&ctx, 800.0, 104.0, vec![], |ui| {
        assert!(notices.show(ui.ctx()).is_none());
    });
    assert!(notices.is_empty());
}

#[test]
fn notifications_stay_compact_and_allow_clicks_through_to_the_focused_control() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut notices = Notifications::new(Id::new("toasts"));
    notices.push(&ctx, NoticeKind::Success, "操作完成");
    let mut draw = |ui: &mut egui::Ui| {
        let button = egui::Area::new(Id::new("under-toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0])
            .show(ui.ctx(), |ui| {
                ui.add(
                    Button::new("继续操作")
                        .id(Id::new("continue"))
                        .min_size(vec2(520.0, 80.0)),
                )
            })
            .inner;
        let toast = notices.show(ui.ctx()).unwrap().response;
        (button, toast)
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let (button, toast) = frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    assert!(toast.rect.width() < button.rect.width());
    assert!(toast.rect.height() < button.rect.height());
    let pos = toast.rect.center();
    assert_eq!(ctx.layer_id_at(pos), Some(button.layer_id));
    ctx.memory_mut(|memory| memory.request_focus(button.id));
    frame(&ctx, 800.0, 0.2, pointer(pos, true), &mut draw);
    let (button, _) = frame(&ctx, 800.0, 0.3, pointer(pos, false), &mut draw);
    assert!(button.clicked());
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(button.id));
}

#[test]
fn caller_can_change_notification_pass_through_while_it_is_visible() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut notices = Notifications::new(Id::new("toasts"));
    notices.push(&ctx, NoticeKind::Success, "操作完成");
    for (step, pass_through) in [false, true].into_iter().enumerate() {
        notices.set_pass_through(pass_through);
        let mut draw = |ui: &mut egui::Ui| {
            let button = egui::Area::new(Id::new("under-toast"))
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0])
                .show(ui.ctx(), |ui| {
                    ui.add(Button::new("继续操作").min_size(vec2(520.0, 80.0)))
                })
                .inner;
            let toast = notices.show(ui.ctx()).unwrap().response;
            (button, toast)
        };
        let time = step as f64;
        frame(&ctx, 800.0, time, vec![], &mut draw);
        let (button, toast) = frame(&ctx, 800.0, time + 0.1, vec![], &mut draw);
        let pos = toast.rect.center();
        let layer = if pass_through {
            button.layer_id
        } else {
            toast.layer_id
        };
        assert_eq!(ctx.layer_id_at(pos), Some(layer));
        frame(&ctx, 800.0, time + 0.2, pointer(pos, true), &mut draw);
        let (button, _) = frame(&ctx, 800.0, time + 0.3, pointer(pos, false), &mut draw);
        assert_eq!(button.clicked(), pass_through);
    }
}

#[test]
fn styled_popup_uses_native_id_width_and_click_policy() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("configured-popup");
    egui::Popup::open_id(&ctx, id);
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new("anchor"));
        let mut popup = Popup::new(&anchor);
        popup.native = popup
            .native
            .id(id)
            .width(360.0)
            .close_behavior(PopupCloseBehavior::IgnoreClicks);
        popup
            .show(|ui| ui.label("contents"))
            .map(|response| response.response.rect)
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let rect = frame(&ctx, 800.0, 0.1, vec![], &mut draw).unwrap();
    assert!((rect.width() - 360.0).abs() < 1.0, "{rect:?}");
    let outside = pos2(720.0, 600.0);
    frame(&ctx, 800.0, 0.2, pointer(outside, true), &mut draw);
    frame(&ctx, 800.0, 0.3, pointer(outside, false), &mut draw);
    assert!(egui::Popup::is_id_open(&ctx, id));
    frame(&ctx, 800.0, 0.4, vec![key(Key::Escape)], &mut draw);
    assert!(!egui::Popup::is_id_open(&ctx, id));
    assert!(!ctx.input(|input| input.key_pressed(Key::Escape)));
}

#[test]
fn styled_popup_limits_native_default_width_to_the_viewport() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("wide-popup");
    egui::Popup::open_id(&ctx, id);
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(Button::new("anchor"));
        let mut popup = Popup::new(&anchor);
        popup.native = popup.native.id(id).width(1000.0);
        popup
            .show(|ui| ui.label("Description wraps inside the available viewport."))
            .unwrap()
            .response
            .rect
    };
    frame(&ctx, 240.0, 0.0, vec![], &mut draw);
    frame(&ctx, 240.0, 0.1, vec![], &mut draw);
    let rect = frame(&ctx, 240.0, 0.2, vec![], &mut draw);
    assert!(rect.width() <= 208.0, "{rect:?}");
    assert!(Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 700.0)).contains_rect(rect));
}

#[test]
fn tabs_accept_native_buttons_without_a_theme_or_a_custom_widget_id() {
    use egui_hunter::components::tabs::interaction::TabsInteraction;

    let ctx = Context::default();
    let mut navigation = NavigationState::default();
    let tabs = [
        Tab::new(Id::new(0), "A"),
        Tab::new(Id::new(1), "B").enabled(false),
        Tab::new(Id::new(2), "C"),
        Tab::new(Id::new(3), "D"),
    ];
    let mut frame = |events| {
        let mut result = Vec::new();
        let output = ctx.run_ui(
            RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                TabsInteraction::new(Id::new("native-tabs")).show(
                    ui,
                    &mut navigation,
                    &tabs,
                    |ui, tab, _, _| {
                        let response = ui.button(tab.label);
                        result.push(response.clone());
                        response
                    },
                    |ui, id| {
                        ui.label(format!("{id:?}"));
                    },
                );
            },
        );
        output.drop_without_applying_deltas();
        (navigation.selected(), result)
    };
    let (_, headers) = frame(vec![]);
    headers[0].request_focus();
    frame(vec![]);
    let (selected, headers) = frame(pulse(Key::ArrowRight));
    assert_eq!(selected, Some(tabs[2].id));
    assert_eq!(ctx.memory(|m| m.focused()), Some(headers[2].id));
    let (selected, headers) = frame(pulse(Key::End));
    assert_eq!(selected, Some(tabs[3].id));
    assert_eq!(ctx.memory(|m| m.focused()), Some(headers[3].id));
}
