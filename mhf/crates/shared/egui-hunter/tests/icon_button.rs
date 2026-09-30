mod events;

use egui::{
    Context, Event, FullOutput, Id, Key, Pos2, RawInput, Rect, Response, Shape, Ui, Vec2, vec2,
};
use egui_hunter::{Button, ButtonKind, Density, Icon, IconButton, Theme};

fn frame<R>(
    ctx: &Context,
    events: Vec<Event>,
    mut show: impl FnMut(&mut Ui) -> R,
) -> (R, FullOutput) {
    let mut result = None;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| result = Some(show(ui)));
        },
    );
    output.textures_delta.clear();
    (result.unwrap(), output)
}

#[test]
fn icon_buttons_are_square_and_centered_without_reserving_label_space() {
    for (density, side) in [(Density::Standard, 36.0), (Density::Compact, 24.0)] {
        let ctx = Context::default();
        Theme::default().density(density).apply(&ctx);
        let ((first, second), output) = frame(&ctx, vec![], |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                (
                    ui.add(IconButton::new(Icon::Pause, "Pause")),
                    ui.add(
                        IconButton::new(Icon::Pause, "A much longer action label")
                            .kind(ButtonKind::Primary),
                    ),
                )
            })
            .inner
        });
        for response in [&first, &second] {
            assert_eq!(response.rect.size(), Vec2::splat(side));
            let glyph = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    Shape::Rect(rect)
                        if response.rect.contains_rect(rect.rect) && rect.rect.width() < side =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .reduce(Rect::union)
                .expect("pause glyph");
            assert!(glyph.center().distance(response.rect.center()) < 0.001);
        }
        assert!(
            output
                .shapes
                .iter()
                .all(|shape| !matches!(shape.shape, Shape::Text(_)))
        );
    }
}

#[test]
fn icon_actions_expose_accessible_names_roles_and_selection() {
    use egui::accesskit::{Action, Role, Toggled};

    let ctx = Context::default();
    ctx.enable_accesskit();
    Theme::default().apply(&ctx);
    let (responses, output) = frame(&ctx, vec![], |ui| {
        [
            ui.add(IconButton::new(Icon::Apply, "Apply project").id(Id::new("apply"))),
            ui.add(
                IconButton::new(Icon::Eye, "Show trace")
                    .id(Id::new("trace"))
                    .selected(true),
            ),
        ]
    });
    let tree = output.platform_output.accesskit_update.unwrap();
    for (response, label) in responses.iter().zip(["Apply project", "Show trace"]) {
        let node = &tree
            .nodes
            .iter()
            .find(|(id, _)| *id == response.id.accesskit_id())
            .unwrap()
            .1;
        assert_eq!(node.role(), Role::Button);
        assert_eq!(node.label(), Some(label));
        assert!(node.supports_action(Action::Click));
        assert!(node.supports_action(Action::Focus));
    }
    let trace = &tree
        .nodes
        .iter()
        .find(|(id, _)| *id == responses[1].id.accesskit_id())
        .unwrap()
        .1;
    assert_eq!(trace.toggled(), Some(Toggled::True));
}

#[test]
fn hover_press_and_focus_preserve_geometry_and_show_a_focus_tooltip() {
    for density in [Density::Standard, Density::Compact] {
        let ctx = Context::default();
        Theme::default().density(density).apply(&ctx);
        let id = Id::new("apply");
        let mut draw = |ui: &mut Ui| {
            ui.visuals_mut().widgets.hovered.expansion = 5.0;
            ui.visuals_mut().widgets.active.expansion = 8.0;
            ui.add(IconButton::new(Icon::Apply, "Apply project").id(id))
        };
        let (initial, _) = frame(&ctx, vec![], &mut draw);
        let point = initial.rect.center();
        let (hovered, hover_output) = frame(&ctx, vec![Event::PointerMoved(point)], &mut draw);
        assert!(hovered.hovered());
        let (pressed, press_output) = frame(&ctx, events::pointer(point, true), &mut draw);
        assert!(pressed.is_pointer_button_down_on());
        frame(
            &ctx,
            events::pointer(Pos2::new(-10.0, -10.0), false),
            &mut draw,
        );
        initial.request_focus();
        let (focused, focus_output) = frame(&ctx, vec![], &mut draw);
        assert!(focused.has_focus());
        for (response, output) in [
            (hovered, hover_output),
            (pressed, press_output),
            (focused, focus_output),
        ] {
            assert_eq!(response.rect, initial.rect);
            let surface = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    Shape::Rect(rect)
                        if rect.rect.contains(initial.rect.center())
                            && (rect.fill != egui::Color32::TRANSPARENT
                                || rect.stroke.width > 0.0) =>
                    {
                        Some(rect)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(surface.rect, initial.rect);
            assert_eq!(surface.stroke_kind, egui::StrokeKind::Inside);
        }
        // Tooltip does not create a selectable/focusable label or open a menu.
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(id));
        assert!(!egui::Popup::is_any_open(&ctx));
        let (_, tooltip_output) = frame(&ctx, vec![], &mut draw);
        assert!(tooltip_output.shapes.iter().any(|shape| matches!(
            &shape.shape, Shape::Text(text) if text.galley.job.text == "Apply project"
        )));
    }
}

#[test]
fn icon_actions_support_native_keyboard_pointer_and_disabled_behavior() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let draw = |ui: &mut Ui, enabled| -> [Response; 2] {
        [
            ui.add_enabled(
                enabled,
                IconButton::new(Icon::Apply, "Apply project").id(Id::new("apply")),
            ),
            ui.add(IconButton::new(Icon::Save, "Save project").id(Id::new("save"))),
        ]
    };
    let (initial, _) = frame(&ctx, vec![], |ui| draw(ui, true));
    initial[0].request_focus();
    for key in [Key::Enter, Key::Space] {
        let (activated, _) = frame(&ctx, vec![events::key(key)], |ui| draw(ui, true));
        assert!(activated[0].clicked());
        assert!(!activated[1].clicked());
    }
    frame(&ctx, vec![events::key(Key::Tab)], |ui| draw(ui, true));
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(initial[1].id));
    let point = initial[0].rect.center();
    frame(&ctx, events::pointer(point, true), |ui| draw(ui, true));
    let (clicked, _) = frame(&ctx, events::pointer(point, false), |ui| draw(ui, true));
    assert!(clicked[0].clicked());
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(initial[0].id));
    let (disabled, _) = frame(&ctx, vec![events::key(Key::Enter)], |ui| draw(ui, false));
    assert!(!disabled[0].enabled());
    assert!(!disabled[0].clicked());
    assert_eq!(disabled[0].rect, initial[0].rect);
}

#[test]
fn disabled_icon_actions_keep_their_hover_labels() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    let mut draw =
        |ui: &mut Ui| ui.add_enabled(false, IconButton::new(Icon::Pause, "Pause execution"));
    let (disabled, _) = frame(&ctx, vec![], &mut draw);
    frame(
        &ctx,
        vec![Event::PointerMoved(disabled.rect.center())],
        &mut draw,
    );
    frame(&ctx, vec![], &mut draw);
    let (hovered, output) = frame(&ctx, vec![], &mut draw);
    assert!(!hovered.enabled());
    assert!(!hovered.hovered());
    assert!(hovered.contains_pointer());
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape, Shape::Text(text) if text.galley.job.text == "Pause execution"
    )));
}

#[test]
fn danger_actions_keep_red_content_when_selected_hovered_and_disabled() {
    for kind in [ButtonKind::Danger, ButtonKind::DangerQuiet] {
        for selected in [false, true] {
            for enabled in [false, true] {
                let ctx = Context::default();
                let mut theme = Theme::default().density(Density::Compact);
                theme.style.visuals.error_fg_color = egui::Color32::RED;
                theme.apply(&ctx);
                let mut draw = |ui: &mut Ui| {
                    ui.horizontal(|ui| {
                        [
                            ui.add_enabled(
                                enabled,
                                IconButton::new(Icon::BreakpointOff, "Remove breakpoint")
                                    .id(Id::new("remove-breakpoint"))
                                    .kind(kind)
                                    .selected(selected),
                            ),
                            ui.add_enabled(
                                enabled,
                                Button::new("Remove").kind(kind).selected(selected),
                            ),
                        ]
                    })
                    .inner
                };
                let (idle, idle_output) = frame(&ctx, vec![], &mut draw);
                let (hovered, hover_output) = frame(
                    &ctx,
                    vec![Event::PointerMoved(idle[0].rect.center())],
                    &mut draw,
                );
                assert_eq!(hovered[0].hovered(), enabled);
                for (responses, output) in [(&idle, &idle_output), (&hovered, &hover_output)] {
                    let dot_color = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Circle(circle)
                                if circle.center == responses[0].rect.center() =>
                            {
                                Some(circle.stroke.color)
                            }
                            _ => None,
                        })
                        .unwrap();
                    let text_color = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Text(text) if text.galley.job.text == "Remove" => {
                                Some(text.fallback_color)
                            }
                            _ => None,
                        })
                        .unwrap();
                    for color in [dot_color, text_color] {
                        assert!(
                            color.r() > 0,
                            "{kind:?}, selected={selected}, enabled={enabled}"
                        );
                        assert_eq!(color.g(), 0);
                        assert_eq!(color.b(), 0);
                    }
                    assert_eq!(responses[0].rect, idle[0].rect);
                }
                if selected && enabled {
                    let surface = idle_output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Rect(rect) if rect.rect == idle[0].rect => Some(rect),
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(surface.fill, theme.style.visuals.selection.bg_fill);
                    idle[0].request_focus();
                    let (_, output) = frame(
                        &ctx,
                        vec![Event::PointerMoved(Pos2::new(-10.0, -10.0))],
                        &mut draw,
                    );
                    let surface = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Rect(rect) if rect.rect == idle[0].rect => Some(rect),
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(surface.stroke.color, theme.tokens.focus);
                    assert_eq!(surface.stroke.width, 2.0);
                    assert_eq!(surface.fill, theme.style.visuals.selection.bg_fill);
                }
            }
        }
    }
}

#[test]
fn breakpoint_off_has_a_visible_slash_inside_a_hollow_circle() {
    let ctx = Context::default();
    let rect = Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::splat(16.0));
    let (_, output) = frame(&ctx, vec![], |ui| {
        Icon::BreakpointOff.paint(ui.painter(), rect, egui::Color32::RED)
    });
    let circle = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Circle(circle) => Some(circle),
            _ => None,
        })
        .unwrap();
    assert_eq!(circle.fill, egui::Color32::TRANSPARENT);
    assert_eq!(circle.stroke.color, egui::Color32::RED);
    let slash = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Path(path) => Some(path),
            _ => None,
        })
        .unwrap();
    assert_eq!(slash.points.len(), 2);
    assert_eq!(
        slash.stroke.color,
        egui::epaint::ColorMode::Solid(circle.stroke.color),
    );
    assert!(slash.points[0].x < circle.center.x && slash.points[0].y < circle.center.y);
    assert!(slash.points[1].x > circle.center.x && slash.points[1].y > circle.center.y);
    for point in &slash.points {
        assert!(point.distance(circle.center) <= circle.radius + 0.1);
    }
}

#[test]
fn toolbar_icons_use_vectors_and_fit_small_icon_rectangles() {
    for icon in [
        Icon::Breakpoint,
        Icon::BreakpointOff,
        Icon::Play,
        Icon::Pause,
        Icon::Step,
        Icon::StepOut,
        Icon::Attach,
        Icon::Detach,
        Icon::Apply,
        Icon::Refresh,
        Icon::FolderOpen,
        Icon::Undo,
        Icon::Copy,
        Icon::Save,
    ] {
        let ctx = Context::default();
        let rect = Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::splat(16.0));
        let (_, output) = frame(&ctx, vec![], |ui| {
            icon.paint(ui.painter(), rect, egui::Color32::WHITE)
        });
        assert!(!output.shapes.is_empty(), "{icon:?}");
        for shape in &output.shapes {
            if matches!(&shape.shape, Shape::Rect(rect) if rect.fill == egui::Color32::TRANSPARENT && rect.stroke.width == 0.0)
            {
                continue;
            }
            assert!(
                !matches!(shape.shape, Shape::Text(_) | Shape::Mesh(_)),
                "{icon:?}"
            );
            assert!(
                rect.contains_rect(shape.shape.visual_bounding_rect()),
                "{icon:?}"
            );
        }
    }
}
