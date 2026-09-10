use egui::{
    Color32, Context, FontId, FullOutput, Id, RawInput, Rect, Response, Shape, Ui, pos2, vec2,
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
            if let Shape::Rect(shape) = shape {
                found |= shape.fill == color && rect.contains_rect(shape.rect);
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
fn style_and_tokens_update_focused_controls_without_reinstalling_the_theme() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("styled-button");
    render(&ctx, |ui| {
        ui.add(Button::new("Focus").id(id));
    });
    ctx.memory_mut(|memory| memory.request_focus(id));
    let idle = Color32::from_rgb(35, 75, 125);
    let selected = Color32::from_rgb(10, 105, 60);
    let local = Tokens {
        focus: Color32::LIGHT_BLUE,
        ..Tokens::from_context(&ctx)
    };
    ctx.all_styles_mut(|style| {
        style.visuals.widgets.inactive.weak_bg_fill = idle;
        style.visuals.selection.bg_fill = selected;
    });
    let mut button = Rect::NOTHING;
    let mut selection = Rect::NOTHING;
    let output = render(&ctx, |ui| {
        local.scope(ui, |ui| {
            let response = ui.add(Button::new("Focus").id(id));
            assert!(response.has_focus());
            button = response.rect;
            selection = ui.add(Button::new("Selection").selected(true)).rect;
        });
    });
    assert!(has_fill(&output, button, idle));
    assert!(has_fill(&output, selection, selected));
    assert_focus_border(&output, button, local.focus);
}

fn assert_focus_border(output: &FullOutput, control: Rect, focus: Color32) {
    let mut borders = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, &mut |shape| {
            if let Shape::Rect(rect) = shape {
                if rect.stroke.width > 0.0 {
                    assert_ne!(rect.stroke_kind, egui::StrokeKind::Outside);
                }
                if rect.stroke.color == focus && rect.stroke.width == 2.0 {
                    assert_eq!(rect.stroke_kind, egui::StrokeKind::Inside);
                    borders.push(rect.rect);
                }
            }
        });
    }
    assert_eq!(
        borders,
        [control],
        "focus replaces the original border without expanding it"
    );
}

#[test]
fn raised_surfaces_inherit_nested_tokens_without_leaking() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let local = Tokens {
        primary: Color32::LIGHT_BLUE,
        ..theme.tokens
    };
    let nested = Tokens {
        success: Color32::YELLOW,
        ..local
    };
    let mut button = Rect::NOTHING;
    let mut field = Rect::NOTHING;
    let output = render(&ctx, |ui| {
        local.scope(ui, |ui| {
            Panel::new("Raised")
                .surface(Surface::Raised)
                .show(ui, |ui| {
                    assert_eq!(Tokens::get(ui), local);
                    assert!(ui.visuals().dark_mode);
                    assert_eq!(ui.stack().bg_color(), theme.style.visuals.faint_bg_color);
                    button = ui.add(Button::new("Action").kind(ButtonKind::Primary)).rect;
                    let mut text = "Hunter".to_owned();
                    field = ui.add(TextField::new(Id::new("name"), &mut text)).rect;
                    nested.scope(ui, |ui| assert_eq!(Tokens::get(ui), nested));
                    assert_eq!(Tokens::get(ui), local);
                });
        });
        assert_eq!(Tokens::get(ui), theme.tokens);
    });
    assert!(has_fill(&output, button, local.primary));
    assert!(has_fill(
        &output,
        field,
        theme.style.visuals.text_edit_bg_color()
    ));
}

#[test]
fn focus_preserves_semantic_fills_selection_marks_and_validation() {
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
        assert!(has_fill(
            &output,
            responses[0].rect,
            theme.style.visuals.selection.bg_fill
        ));
        assert!(has_fill(&output, responses[1].rect, theme.tokens.primary));
        assert!(has_fill(
            &output,
            responses[2].rect,
            theme.style.visuals.selection.bg_fill
        ));
        assert!(has_fill(&output, responses[3].rect, theme.tokens.primary));
        assert!(has_fill(&output, responses[4].rect, theme.tokens.primary));
        let mut checks = 0;
        let mut errors = 0;
        let mut field_error = false;
        let mut primary_text = false;
        for shape in &output.shapes {
            visit(&shape.shape, &mut |shape| match shape {
                Shape::Path(path) if !path.closed && path.points.len() == 3 => checks += 1,
                Shape::Rect(rect) if rect.rect == responses[5].rect => {
                    field_error |= rect.stroke.color == theme.style.visuals.error_fg_color;
                }
                Shape::Text(text) if text.galley.job.text == "Invalid name" => errors += 1,
                Shape::Text(text) if text.galley.job.text == "Accept" => {
                    primary_text = text.fallback_color == theme.tokens.on_primary;
                }
                _ => {}
            });
        }
        assert_eq!(
            checks, 3,
            "selected button, item and checkbox keep their checks"
        );
        assert_eq!(errors, 1);
        assert!(field_error);
        assert!(primary_text);
        let mut focus_rect = responses[focused].rect;
        if focused == 3 || focused == 4 {
            focus_rect = Rect::from_center_size(
                pos2(
                    focus_rect.left()
                        + theme.style.spacing.icon_width * if focused == 4 { 1.0 } else { 0.5 },
                    focus_rect.center().y,
                ),
                vec2(
                    theme.style.spacing.icon_width * if focused == 4 { 2.0 } else { 1.0 },
                    theme.style.spacing.icon_width,
                ),
            );
        }
        let focus_color = match focused {
            1 | 3 | 4 => theme.tokens.on_primary,
            5 => theme.style.visuals.error_fg_color,
            _ => theme.tokens.focus,
        };
        assert_focus_border(&output, focus_rect, focus_color);
    }
}

#[test]
fn default_theme_matches_the_approved_palette_and_control_sizes() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    assert_eq!(
        theme.style.visuals.panel_fill,
        Color32::from_rgb(0x11, 0x12, 0x14)
    );
    assert_eq!(theme.tokens.primary, Color32::from_rgb(0xD8, 0xB8, 0x78));
    assert_eq!(
        theme.style.visuals.window_corner_radius,
        egui::CornerRadius::same(12)
    );
    render(&ctx, |ui| {
        assert_eq!(ui.add(Button::new("Default")).rect.height(), 36.0);
        assert_eq!(
            ui.add(Button::new("Primary").kind(ButtonKind::Primary))
                .rect
                .height(),
            44.0
        );
        let mut value = String::new();
        let field = ui.add(TextField::new(Id::new("sized-field"), &mut value));
        assert!((field.rect.height() - 40.0).abs() <= 1.0);
    });
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
fn panel_surface_metadata_matches_the_single_rounded_native_frame() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let output = render(&ctx, |ui| {
        Panel::new("Panel").show(ui, |ui| {
            assert_eq!(
                ui.stack().bg_color(),
                ctx.global_style().visuals.window_fill
            );
            Panel::new("Raised")
                .surface(Surface::Raised)
                .show(ui, |ui| {
                    assert_eq!(
                        ui.stack().bg_color(),
                        ctx.global_style().visuals.faint_bg_color
                    );
                    egui::ScrollArea::vertical()
                        .max_height(60.0)
                        .show(ui, |ui| {
                            assert_eq!(
                                ui.stack().bg_color(),
                                ctx.global_style().visuals.faint_bg_color
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
        ctx.global_style().visuals.faint_bg_color,
    ] {
        let mut surfaces = 0;
        for shape in &output.shapes {
            visit(&shape.shape, &mut |shape| match shape {
                Shape::Rect(rect) if rect.fill == color => {
                    assert_eq!(rect.corner_radius, egui::CornerRadius::same(12));
                    surfaces += 1;
                }
                _ => {}
            });
        }
        assert_eq!(surfaces, 1);
    }
}
