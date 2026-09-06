use egui::{
    Color32, Context, FontId, FullOutput, Id, RawInput, Rect, Response, Shape, Stroke, Ui, pos2,
    vec2,
};
use egui_hunter::{
    Button, ButtonKind, Checkbox, ItemSlot, Panel, Surface, TextField, Theme, Toggle, Tokens,
    Validation,
};

fn render(ctx: &Context, mut content: impl FnMut(&mut Ui)) -> FullOutput {
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 1000.0))),
            ..Default::default()
        },
        |ui| content(ui),
    );
    output.textures_delta.clear();
    output
}

fn visit(shape: &Shape, check: &mut impl FnMut(&Shape)) {
    check(shape);
    if let Shape::Vec(shapes) = shape {
        for shape in shapes {
            visit(shape, check);
        }
    }
}

fn has_fill(output: &FullOutput, rect: Rect, color: Color32) -> bool {
    let mut found = false;
    for shape in &output.shapes {
        visit(&shape.shape, &mut |shape| {
            if let Shape::Path(path) = shape {
                found |=
                    path.fill == color && path.points.iter().all(|point| rect.contains(*point));
            }
        });
    }
    found
}

#[test]
fn widgets_resolve_local_style_at_render_time_and_siblings_keep_their_style() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let background = Color32::from_rgb(35, 75, 125);
    let strong_background = Color32::from_rgb(55, 95, 145);
    let foreground = Color32::from_rgb(240, 90, 170);
    let mut sizes = Vec::new();
    let mut targets = Vec::new();
    let output = render(&ctx, |ui| {
        sizes.clear();
        targets.clear();
        sizes.push(ui.add(Button::new("Before")).rect.size());
        let button = Button::new("Local button");
        ui.scope(|ui| {
            ui.visuals_mut().widgets.inactive.weak_bg_fill = background;
            ui.visuals_mut().widgets.inactive.bg_fill = strong_background;
            ui.visuals_mut().override_text_color = Some(foreground);
            ui.style_mut().override_font_id = Some(FontId::proportional(26.0));
            ui.spacing_mut().button_padding = vec2(25.0, 20.0);
            ui.spacing_mut().interact_size.y = 70.0;
            let response = ui.add(button);
            sizes.push(response.rect.size());
            targets.push((response.rect, background));
            let mut checked = false;
            targets.push((
                ui.add(Checkbox::new(&mut checked, "Local checkbox")).rect,
                strong_background,
            ));
            targets.push((ui.add(ItemSlot::new("Local item")).rect, strong_background));
            let _ = ui.button("Local native");
            egui_hunter::key_hint(ui, "Local key", "Hint");
            ui.add(egui_hunter::Meter::new(0.5).label("Local meter"));
            ui.spacing_mut().icon_width = 48.0;
            let large_icon = ui.add(Button::new("Large icon").icon(egui_hunter::Icon::Quest));
            assert!(large_icon.rect.height() >= 48.0 + 2.0 * ui.spacing().button_padding.y);
        });
        sizes.push(ui.add(Button::new("After")).rect.size());
    });
    assert!(sizes[1].y >= 70.0);
    assert_eq!(sizes[0].y, sizes[2].y);
    assert!(sizes[0].y < sizes[1].y);
    for (target, background) in targets {
        assert!(
            has_fill(&output, target, background),
            "custom surface must use the local native fill"
        );
    }
    let mut labels = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, &mut |shape| {
            if let Shape::Text(text) = shape
                && text.galley.job.text.starts_with("Local")
            {
                assert_eq!(text.fallback_color, foreground);
                assert!(
                    text.galley
                        .job
                        .sections
                        .iter()
                        .all(|section| section.format.font_id.size == 26.0)
                );
                labels.push(text.galley.job.text.clone());
            }
        });
    }
    assert_eq!(
        labels,
        [
            "Local button",
            "Local checkbox",
            "Local native",
            "Local key",
            "Local meter"
        ]
    );
}

#[test]
fn native_active_style_updates_reach_focused_controls_without_reinstalling_the_theme() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("styled-button");
    let mut button: Option<Response> = None;
    render(&ctx, |ui| {
        button = Some(ui.add(Button::new("Focus").id(id)))
    });
    button.as_ref().unwrap().request_focus();
    let idle = Color32::from_rgb(35, 75, 125);
    let active = Color32::from_rgb(110, 45, 155);
    let selected = Color32::from_rgb(10, 105, 60);
    ctx.all_styles_mut(|style| {
        style.visuals.widgets.inactive.weak_bg_fill = idle;
        style.visuals.widgets.active.weak_bg_fill = active;
        style.visuals.widgets.active.bg_stroke = Stroke::new(3.0, Color32::LIGHT_BLUE);
        style.visuals.selection.bg_fill = selected;
    });
    let mut selected_rect = Rect::NOTHING;
    let mut meter_rect = Rect::NOTHING;
    let output = render(&ctx, |ui| {
        button = Some(ui.add(Button::new("Focus").id(id)));
        selected_rect = ui.add(Button::new("Selection").selected(true)).rect;
        meter_rect = ui.add(egui_hunter::Meter::new(0.5)).rect;
    });
    assert!(button.as_ref().unwrap().has_focus());
    let focused = button.unwrap().rect;
    assert!(has_fill(&output, focused, active));
    assert!(!has_fill(&output, focused, idle));
    assert!(has_fill(&output, selected_rect, selected));
    assert!(has_fill(&output, meter_rect, selected));
}

#[test]
fn parchment_and_nested_tokens_apply_to_real_children_without_leaking() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let global = Tokens::from_context(&ctx);
    let local = Tokens {
        cut: 11.0,
        parchment: Color32::from_rgb(220, 200, 180),
        ink: Color32::from_rgb(55, 40, 30),
        ..global
    };
    let paper = Tokens {
        primary: local.parchment.lerp_to_gamma(local.ink, 0.18),
        success: local.ink.lerp_to_gamma(local.success, 0.25),
        ..local
    };
    let nested = Tokens { cut: 2.0, ..paper };
    let mut button_rect = Rect::NOTHING;
    let mut editor_rect = Rect::NOTHING;
    let output = render(&ctx, |ui| {
        assert_eq!(Tokens::get(ui), global);
        local.scope(ui, |ui| {
            Panel::new("Paper")
                .surface(Surface::Parchment)
                .show(ui, |ui| {
                    assert_eq!(Tokens::get(ui), paper);
                    assert_eq!(ui.visuals().text_color(), local.ink);
                    button_rect = ui.add(Button::new("Paper button")).rect;
                    let mut value = "Paper editor".to_owned();
                    editor_rect = ui
                        .add(TextField::new(Id::new("paper-field"), &mut value))
                        .rect;
                    nested.scope(ui, |ui| assert_eq!(Tokens::get(ui), nested));
                    assert_eq!(Tokens::get(ui), paper);
                });
        });
        assert_eq!(Tokens::get(ui), global);
        assert_eq!(
            ui.visuals().window_fill,
            ctx.global_style().visuals.window_fill
        );
    });
    assert!(has_fill(&output, button_rect, local.parchment));
    assert!(has_fill(&output, editor_rect, local.parchment));
    let mut editor_seen = false;
    for shape in &output.shapes {
        visit(&shape.shape, &mut |shape| {
            if let Shape::Text(text) = shape
                && text.galley.job.text == "Paper editor"
            {
                editor_seen = true;
                assert_eq!(text.fallback_color, local.ink);
            }
        });
    }
    assert!(editor_seen);
}

#[test]
fn native_active_backgrounds_keep_selection_and_validation_visible() {
    fn controls(ui: &mut Ui) -> Vec<Response> {
        let mut checked = true;
        let mut enabled = true;
        let mut value = "Hunter".to_owned();
        vec![
            ui.add(Button::new("Selected").selected(true)),
            ui.add(Button::new("Accept").kind(ButtonKind::Primary)),
            ui.add(ItemSlot::new("Potion").selected(true)),
            ui.add(Checkbox::new(&mut checked, "Checked")),
            ui.add(Toggle::new(&mut enabled, "Enabled")),
            ui.add(
                TextField::new(Id::new("invalid-name"), &mut value)
                    .validation(Validation::Error("Invalid name")),
            ),
        ]
    }

    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let mut responses = Vec::new();
    render(&ctx, |ui| responses = controls(ui));
    for focused in 0..responses.len() {
        responses[focused].request_focus();
        let output = render(&ctx, |ui| responses = controls(ui));
        assert!(responses[focused].has_focus());
        let active_fill = theme.style.visuals.widgets.active.bg_fill;
        let mut highlights = Vec::new();
        let mut selections = Vec::new();
        let mut field_strokes = Vec::new();
        let mut errors = 0;
        let mut focused_text = None;
        let label = [
            Some("Selected"),
            Some("Accept"),
            None,
            Some("Checked"),
            Some("Enabled"),
            Some("Hunter"),
        ][focused];
        for (index, shape) in output.shapes.iter().enumerate() {
            visit(&shape.shape, &mut |shape| match shape {
                Shape::Path(path) if path.fill == active_fill => {
                    highlights.push((Rect::from_points(&path.points), index));
                    if Rect::from_points(&path.points) == responses[5].rect.shrink(0.5) {
                        field_strokes.push(path.stroke.color.clone());
                    }
                }
                Shape::Path(path) if path.stroke.width > 0.0 => {
                    if !path.closed
                        && path.stroke.color
                            == egui::epaint::ColorMode::Solid(
                                theme.style.visuals.selection.stroke.color,
                            )
                    {
                        selections.push(Rect::from_points(&path.points));
                    }
                    if path.closed
                        && Rect::from_points(&path.points) == responses[5].rect.shrink(0.5)
                    {
                        field_strokes.push(path.stroke.color.clone());
                    }
                    assert_ne!(
                        path.stroke.color,
                        egui::epaint::ColorMode::Solid(active_fill),
                        "active fill must not become another control outline"
                    );
                }
                Shape::Text(text) if text.galley.job.text == "Invalid name" => errors += 1,
                Shape::Text(text) if Some(text.galley.job.text.as_str()) == label => {
                    focused_text = Some(index)
                }
                _ => {}
            });
        }
        assert_eq!(highlights.len(), 1, "control {focused}");
        assert!(responses[focused].rect.contains_rect(highlights[0].0));
        if let Some(text) = focused_text {
            assert!(
                highlights[0].1 < text,
                "focus changes the background before content, without a later overlay"
            );
        }
        assert_eq!(
            selections.len(),
            3,
            "selected button, slot and checkbox keep their checks"
        );
        for selected in [0, 2, 3] {
            assert!(
                selections
                    .iter()
                    .any(|rect| responses[selected].rect.contains_rect(*rect))
            );
        }
        assert!(has_fill(
            &output,
            responses[0].rect,
            if focused == 0 {
                active_fill
            } else {
                theme.style.visuals.selection.bg_fill
            }
        ));
        assert!(has_fill(
            &output,
            responses[1].rect,
            if focused == 1 {
                active_fill
            } else {
                theme.tokens.primary
            }
        ));
        assert_eq!(
            field_strokes,
            [egui::epaint::ColorMode::Solid(
                theme.style.visuals.widgets.inactive.bg_stroke.color
            )]
        );
        assert_eq!(errors, 1);
    }
}

#[test]
fn parchment_focus_uses_local_native_style_and_restores_the_outer_theme() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let id = Id::new("paper-focus");
    let mut button = None;
    let mut active_fill = Color32::TRANSPARENT;
    let mut draw = |ui: &mut Ui| {
        Panel::new("Paper")
            .surface(Surface::Parchment)
            .show(ui, |ui| {
                active_fill = ui.visuals().widgets.active.bg_fill;
                button = Some(ui.add(Button::new("Paper action").id(id)));
            });
        assert_eq!(Tokens::get(ui), theme.tokens);
    };
    render(&ctx, &mut draw);
    ctx.memory_mut(|memory| memory.request_focus(id));
    let output = render(&ctx, &mut draw);
    let button = button.unwrap();
    assert!(button.has_focus());
    assert_ne!(active_fill, theme.style.visuals.widgets.active.bg_fill);
    assert!(has_fill(&output, button.rect, active_fill));
}

#[test]
fn attention_styles_do_not_add_content_padding() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let mut rects = Vec::new();
    let output = render(&ctx, |ui| {
        rects.clear();
        let mut checked = false;
        let mut text = "Field".to_owned();
        rects.push(ui.add(Button::new("Button")).rect);
        rects.push(ui.add(Checkbox::new(&mut checked, "Checkbox")).rect);
        rects.push(ui.add(Toggle::new(&mut checked, "Toggle")).rect);
        rects.push(ui.add(TextField::new(Id::new("field"), &mut text)).rect);
        rects.push(
            ui.add(ItemSlot::new("Item").image(egui::TextureId::User(17)))
                .rect,
        );
    });
    let spacing = &theme.style.spacing;
    let mut labels = 0;
    let mut image_seen = false;
    for shape in &output.shapes {
        visit(&shape.shape, &mut |shape| match shape {
            Shape::Text(text) => {
                let expected = match text.galley.job.text.as_str() {
                    "Button" => rects[0].center().x - text.galley.size().x * 0.5,
                    "Checkbox" => rects[1].left() + spacing.icon_width + spacing.icon_spacing,
                    "Toggle" => rects[2].left() + spacing.icon_width * 2.0 + spacing.icon_spacing,
                    "Field" => rects[3].left() + spacing.button_padding.x,
                    _ => return,
                };
                assert!(
                    (text.pos.x - expected).abs() <= 0.5,
                    "{} must retain its ordinary content alignment",
                    text.galley.job.text
                );
                labels += 1;
            }
            Shape::Mesh(mesh) if mesh.texture_id == egui::TextureId::User(17) => {
                let points = mesh
                    .vertices
                    .iter()
                    .map(|vertex| vertex.pos)
                    .collect::<Vec<_>>();
                assert_eq!(Rect::from_points(&points).center(), rects[4].center());
                image_seen = true;
            }
            _ => {}
        });
    }
    assert_eq!(labels, 4);
    assert!(image_seen);
}

#[test]
fn panel_surface_metadata_reaches_native_children_without_painting_a_rectangle() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let output = render(&ctx, |ui| {
        Panel::new("Leather").show(ui, |ui| {
            assert_eq!(
                ui.stack().bg_color(),
                ctx.global_style().visuals.window_fill
            );
            Panel::new("Parchment")
                .surface(Surface::Parchment)
                .show(ui, |ui| {
                    assert_eq!(
                        ui.stack().bg_color(),
                        egui_hunter::Tokens::from_context(&ctx).parchment
                    );
                    egui::ScrollArea::vertical()
                        .max_height(60.0)
                        .show(ui, |ui| {
                            assert_eq!(
                                ui.stack().bg_color(),
                                egui_hunter::Tokens::from_context(&ctx).parchment
                            );
                            ui.allocate_space(vec2(80.0, 200.0));
                        });
                });
            assert_eq!(
                ui.stack().bg_color(),
                ctx.global_style().visuals.window_fill
            );
        });
    });

    for color in [
        ctx.global_style().visuals.window_fill,
        egui_hunter::Tokens::from_context(&ctx).parchment,
    ] {
        let mut surfaces = 0;
        for shape in &output.shapes {
            visit(&shape.shape, &mut |shape| match shape {
                Shape::Path(path) if path.fill == color => {
                    assert!(path.closed);
                    assert_eq!(path.points.len(), 8, "Surface must retain its cut corners");
                    surfaces += 1;
                }
                Shape::Rect(rect) if rect.fill == color => {
                    panic!("Native stack metadata must not paint a rectangular surface")
                }
                _ => {}
            });
        }
        assert_eq!(surfaces, 1);
    }
}
