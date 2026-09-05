mod events;

use std::time::Duration;

use egui::{Context, Event, Id, Key, RawInput, Rect, Response, pos2, vec2};
use egui_hunter::{MenuStack, NoticeKind, Notifications, OverlayState, Tab, TabsState, Theme};
use events::{key, pointer};

fn frame<R>(
    ctx: &Context,
    width: f32,
    time: f64,
    events: Vec<Event>,
    mut show: impl FnMut(&mut egui::Ui) -> R,
) -> R {
    let mut result = None;
    let mut output = ctx.run_ui(
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
    output.textures_delta.clear();
    result.unwrap()
}

#[derive(Default)]
struct Overlays {
    dialog: OverlayState,
    popup: OverlayState,
    nested: OverlayState,
    open: bool,
    open_popup: bool,
    open_nested: bool,
    accepts: usize,
}

impl Overlays {
    fn show(&mut self, ui: &mut egui::Ui) -> Response {
        let theme = Theme::default();
        let ctx = ui.ctx().clone();
        let opener = ui.add(theme.button("open").id(Id::new("opener")));
        if self.open {
            self.dialog.open_from(&opener);
            self.open = false;
        }
        theme
            .dialog(Id::new("dialog"), "Dialog")
            .initial_focus(Id::new("confirm"))
            .show(&ctx, &mut self.dialog, |ui| {
                let confirm = ui.add(theme.button("confirm").id(Id::new("confirm")));
                if confirm.clicked() {
                    self.accepts += 1;
                    ui.close();
                }
                let anchor = ui.add(theme.button("popup").id(Id::new("popup-anchor")));
                if self.open_popup {
                    self.popup.open_from(&anchor);
                    self.open_popup = false;
                }
                theme
                    .popup(&anchor)
                    .initial_focus(Id::new("popup-item"))
                    .show(&mut self.popup, |ui| {
                        ui.add(theme.button("item").id(Id::new("popup-item")));
                    });
                let nested = ui.add(theme.button("nested").id(Id::new("nested-opener")));
                if self.open_nested {
                    self.nested.open_from(&nested);
                    self.open_nested = false;
                }
            });
        theme
            .dialog(Id::new("nested"), "Nested")
            .initial_focus(Id::new("nested-confirm"))
            .show(&ctx, &mut self.nested, |ui| {
                ui.add(theme.button("close").id(Id::new("nested-confirm")));
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
    let mut app = Overlays {
        open: true,
        open_popup: true,
        ..Default::default()
    };
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("popup-item")));
    app.frame(&ctx, vec![]);
    assert!(app.popup.is_open());
    app.frame(&ctx, vec![key(Key::Escape)]);
    assert!(!app.popup.is_open());
    assert!(app.dialog.is_open());
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("popup-anchor")));
    app.frame(&ctx, vec![key(Key::Escape)]);
    assert!(!app.dialog.is_open());
    app.frame(&ctx, vec![]);
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("opener")));
}

#[test]
fn clicking_another_input_closes_popup_without_stealing_its_focus() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut popup = OverlayState::default();
    let mut text = String::new();
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(theme.button("anchor").id(Id::new("anchor")));
        ui.add_space(300.0);
        let input = theme.text_edit(ui, Id::new("input"), &mut text, "type here");
        theme.popup(&anchor).show(&mut popup, |ui| {
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
    assert!(!popup.is_open());
    assert_eq!(text, "x");
}

#[test]
fn switching_popup_anchors_keeps_only_the_new_popup_open() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut first = OverlayState::default();
    let mut second = OverlayState::default();
    first.open(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        let first_anchor = ui.add(theme.button("first"));
        theme.popup(&first_anchor).show(&mut first, |ui| {
            ui.label("first contents");
        });
        ui.add_space(300.0);
        let second_anchor = ui.add(theme.button("second"));
        theme
            .popup(&second_anchor)
            .initial_focus(Id::new("second-item"))
            .show(&mut second, |ui| {
                ui.add(theme.button("second item").id(Id::new("second-item")));
            });
        second_anchor.rect.center()
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let pos = frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    frame(&ctx, 800.0, 0.2, pointer(pos, true), &mut draw);
    frame(&ctx, 800.0, 0.3, pointer(pos, false), &mut draw);
    frame(&ctx, 800.0, 0.4, vec![], &mut draw);
    assert!(!first.is_open());
    assert!(second.is_open());
    assert_eq!(ctx.memory(|m| m.focused()), Some(Id::new("second-item")));
}

#[test]
fn styled_window_retains_native_movement_and_edge_resizing() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut draw = |ui: &mut egui::Ui| {
        theme
            .window("Movable")
            .default_pos([100.0, 100.0])
            .default_size([300.0, 240.0])
            .show(ui.ctx(), |ui| {
                ui.label("body");
            })
            .unwrap()
            .response
            .rect
    };
    frame(&ctx, 1000.0, 0.0, vec![], &mut draw);
    let before = frame(&ctx, 1000.0, 0.1, vec![], &mut draw);
    let from = before.min + vec2(160.0, 24.0);
    let to = from + vec2(70.0, 45.0);
    frame(&ctx, 1000.0, 0.2, pointer(from, true), &mut draw);
    frame(&ctx, 1000.0, 0.3, vec![Event::PointerMoved(to)], &mut draw);
    frame(&ctx, 1000.0, 0.4, pointer(to, false), &mut draw);
    let moved = frame(&ctx, 1000.0, 0.5, vec![], &mut draw);
    assert!(moved.left() > before.left() + 40.0 && moved.top() > before.top() + 20.0);
    let edge = moved.right_bottom() - vec2(1.0, 1.0);
    let resized = edge + vec2(90.0, 70.0);
    frame(&ctx, 1000.0, 0.6, pointer(edge, true), &mut draw);
    frame(
        &ctx,
        1000.0,
        0.7,
        vec![Event::PointerMoved(resized)],
        &mut draw,
    );
    frame(&ctx, 1000.0, 0.8, pointer(resized, false), &mut draw);
    let after = frame(&ctx, 1000.0, 0.9, vec![], &mut draw);
    assert!(
        after.width() > moved.width() + 40.0 && after.height() > moved.height() + 30.0,
        "before={before:?} moved={moved:?} after={after:?}"
    );
}

#[test]
fn stale_popup_state_does_not_consume_escape_for_its_replacement() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut first = OverlayState::default();
    let mut second = OverlayState::default();
    first.open(&ctx);
    let draw = |ui: &mut egui::Ui, first: &mut OverlayState, second: &mut OverlayState| {
        let anchor = ui.add(theme.button("first"));
        theme.popup(&anchor).show(first, |ui| {
            ui.label("first");
        });
        let anchor = ui.add(theme.button("second"));
        theme.popup(&anchor).show(second, |ui| {
            ui.label("second");
        });
    };
    frame(&ctx, 800.0, 0.0, vec![], |ui| {
        draw(ui, &mut first, &mut second)
    });
    second.open(&ctx);
    frame(&ctx, 800.0, 0.1, vec![], |ui| {
        draw(ui, &mut first, &mut second)
    });
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], |ui| {
        draw(ui, &mut first, &mut second)
    });
    assert!(!first.is_open());
    assert!(!second.is_open());
}

#[test]
fn nested_dialog_escape_pops_one_layer_and_restores_parent_focus() {
    let ctx = Context::default();
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
}

#[test]
fn popup_keeps_inside_clicks_and_closes_on_outside_click_without_reopening() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut state = OverlayState::default();
    let mut open = true;
    let mut draw = |ui: &mut egui::Ui| {
        let anchor = ui.add(theme.button("anchor"));
        if open {
            state.open_from(&anchor);
            open = false;
        }
        theme
            .popup(&anchor)
            .show(&mut state, |ui| ui.add(theme.button("inside")).rect)
            .map(|response| response.inner)
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let inside = frame(&ctx, 800.0, 0.1, vec![], &mut draw).unwrap().center();
    frame(&ctx, 800.0, 0.2, pointer(inside, true), &mut draw);
    assert!(frame(&ctx, 800.0, 0.3, pointer(inside, false), &mut draw).is_some());
    let outside = pos2(720.0, 600.0);
    frame(&ctx, 800.0, 0.4, pointer(outside, true), &mut draw);
    frame(&ctx, 800.0, 0.5, pointer(outside, false), &mut draw);
    assert!(frame(&ctx, 800.0, 0.6, vec![], &mut draw).is_none());
    assert!(frame(&ctx, 800.0, 0.7, vec![], &mut draw).is_none());
    assert!(!state.is_open());
}

#[test]
fn backdrop_dismissal_can_be_disabled() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut state = OverlayState::default();
    state.open(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        theme
            .dialog(Id::new("dialog"), "Title")
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
        theme
            .dialog(Id::new("dialog"), "Title")
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
    let theme = Theme::default();
    let mut open = true;
    frame(&ctx, 800.0, 0.0, vec![], |ui| {
        theme.window("Window").open(&mut open).show(ui.ctx(), |ui| {
            ui.label("body");
        });
    });
    frame(&ctx, 800.0, 0.1, vec![], |ui| {
        theme.window("Window").open(&mut open).show(ui.ctx(), |ui| {
            ui.close();
        });
    });
    assert!(!open);
    frame(&ctx, 800.0, 1.0, vec![], |ui| {
        theme
            .window("Window")
            .open(&mut open)
            .show(ui.ctx(), |_| panic!("closed content rendered"));
    });
}

#[test]
fn window_with_nested_panels_keeps_its_height_across_idle_frames() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let mut open = true;
    let mut draw = |ui: &mut egui::Ui| {
        theme
            .window("Field notes")
            .open(&mut open)
            .default_size([340.0, 220.0])
            .show(ui.ctx(), |ui| {
                ui.label("A brief introduction");
                theme.panel("Nested panel").show(ui, |ui| {
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
fn scroll_panel_virtualizes_large_lists_and_preserves_each_offset() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut offset = Some(vec2(0.0, 1200.0));
    let mut draw = |ui: &mut egui::Ui| {
        let mut range = 0..0;
        let mut panel = theme
            .scroll_panel(Id::new("first"), "Archive")
            .max_height(160.0);
        if let Some(value) = offset.take() {
            panel = panel.offset(value);
        }
        let output = panel
            .show_rows(ui, 24.0, 10_000, |ui, rows| {
                range = rows.clone();
                for row in rows {
                    ui.add_sized([100.0, 24.0], egui::Label::new(row.to_string()));
                }
            })
            .inner;
        let other = theme
            .scroll_panel(Id::new("second"), "Other")
            .max_height(100.0)
            .show_rows(ui, 24.0, 10_000, |ui, rows| {
                for row in rows {
                    ui.add_sized([100.0, 24.0], egui::Label::new(row.to_string()));
                }
            })
            .inner;
        (
            range,
            output.state.offset.y,
            output.inner_rect.height(),
            other.state.offset.y,
        )
    };
    frame(&ctx, 800.0, 0.0, vec![], &mut draw);
    let (range, offset, height, other) = frame(&ctx, 800.0, 0.1, vec![], &mut draw);
    assert!(
        range.start > 20 && range.len() < 20,
        "visible range was {range:?}"
    );
    assert!((offset - 1200.0).abs() < 1.0);
    assert!(height <= 160.0);
    assert_eq!(other, 0.0);
}

#[test]
fn responsive_columns_reflow_without_changing_child_identity() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut draw = |ui: &mut egui::Ui| {
        theme
            .columns(Id::new("columns"))
            .min_column_width(300.0)
            .show(ui, 3, |ui, _| ui.add(theme.button("item").full_width()))
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
        let output = theme
            .columns(Id::new("empty"))
            .show(ui, 0, |_, _| panic!("empty layout invoked"));
        assert!(output.inner.is_empty());
    });
}

#[test]
fn tabs_skip_disabled_headers_preserve_content_identity_and_leave_text_arrows_alone() {
    let ctx = Context::default();
    let theme = Theme::default();
    let id = Id::new("tabs");
    let first = Id::new("first");
    let last = Id::new("last");
    let tabs = [
        Tab::new(first, "first"),
        Tab::new(Id::new("disabled"), "disabled").enabled(false),
        Tab::new(last, "last"),
    ];
    let mut state = TabsState::default();
    let mut text = String::from("abc");
    let mut draw = |ui: &mut egui::Ui| {
        theme
            .tabs(id)
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
        theme
            .tabs(id)
            .show(ui, &mut state, &tabs[2..], |_, page| assert_eq!(page, last));
    });
    assert_eq!(state.selected(), Some(last));
    frame(&ctx, 800.0, 0.5, vec![], |ui| {
        theme.tabs(id).show(ui, &mut state, &tabs[1..2], |_, _| {
            panic!("disabled content invoked")
        });
    });
    assert_eq!(state.selected(), None);
}

#[test]
fn menu_back_preserves_root_and_restores_parent_control_after_render() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut menu = MenuStack::new(Id::new("menu"), 0_u8);
    let root = frame(&ctx, 800.0, 0.0, vec![], |ui| {
        menu.show(ui, |ui, _| ui.add(theme.button("next"))).inner
    });
    menu.push_from(&root, 1);
    frame(&ctx, 800.0, 0.1, vec![], |ui| {
        menu.show(ui, |ui, _| ui.label("child"));
    });
    frame(&ctx, 800.0, 0.2, vec![key(Key::Escape)], |ui| {
        menu.show(ui, |ui, _| ui.add(theme.button("next")));
    });
    assert_eq!(*menu.current(), 0);
    assert_eq!(ctx.memory(|m| m.focused()), Some(root.id));
    assert!(!menu.back(&ctx));
    assert_eq!(menu.depth(), 1);
}

#[test]
fn menu_escape_waits_for_modal_to_close() {
    let ctx = Context::default();
    let mut menu = MenuStack::new(Id::new("menu"), 0_u8);
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
fn notifications_start_lifetime_when_shown_and_bound_the_waiting_queue() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut notices = Notifications::with_capacity(Id::new("notices"), 3);
    let first = notices.push_for(&ctx, NoticeKind::Success, "first", Duration::from_secs(2));
    let second = notices.push_for(&ctx, NoticeKind::Warning, "second", Duration::from_secs(2));
    frame(&ctx, 800.0, 100.0, vec![], |ui| {
        notices.show(ui.ctx(), &theme);
    });
    assert_eq!(notices.front_id(), Some(first));
    frame(&ctx, 800.0, 102.1, vec![], |ui| {
        notices.show(ui.ctx(), &theme);
    });
    assert_eq!(notices.front_id(), Some(second));
    frame(&ctx, 800.0, 103.0, vec![], |ui| {
        notices.show(ui.ctx(), &theme);
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
        assert!(notices.show(ui.ctx(), &theme).is_none());
    });
    assert!(notices.is_empty());
}
