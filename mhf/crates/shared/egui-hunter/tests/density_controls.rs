mod events;

use egui::{
    Context, Event, FontId, FullOutput, Id, Key, RawInput, Rect, Response, Shape, Ui, pos2, vec2,
};
use egui_hunter::{
    Button, ButtonKind, Density, Field, FormLayout, LabelPlacement, NavigationState, SelectField,
    Tab, Tabs, TextField, Theme, Tokens,
};

fn render(ctx: &Context, events: Vec<Event>, mut content: impl FnMut(&mut Ui)) -> FullOutput {
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 3000.0))),
            events,
            ..Default::default()
        },
        |ui| {
            ui.set_width(320.0);
            content(ui);
        },
    );
    output.textures_delta.clear();
    output
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1.0,
        "actual {actual}, expected {expected}"
    );
}

fn control_heights(ui: &mut Ui) -> [f32; 8] {
    let mut value = String::new();
    let native = ui.button("Control").rect.height();
    let hunter = ui.add(Button::new("Control")).rect.height();
    let primary = ui
        .add(Button::new("Control").kind(ButtonKind::Primary))
        .rect
        .height();
    let text_id = ui.make_persistent_id("text");
    let text = ui.add(TextField::new(text_id, &mut value)).rect.height();
    let select_id = ui.make_persistent_id("select");
    let select = SelectField::new(select_id, "Control")
        .show_ui(ui, |_| ())
        .response
        .rect
        .height();
    let field_id = ui.make_persistent_id("native-field");
    let native_field = Field::new(field_id)
        .label("Label")
        .show(ui, |ui| ui.button("Control"))
        .rect
        .height();
    let form_id = ui.make_persistent_id("form");
    let form_text_id = ui.make_persistent_id("form-text");
    let form_select_id = ui.make_persistent_id("form-select");
    let form = FormLayout::new(form_id)
        .label_placement(LabelPlacement::Left)
        .show(
            ui,
            &[
                Field::new(form_text_id).label("Text"),
                Field::new(form_select_id).label("Select"),
            ],
            |ui, index| {
                if index == 0 {
                    ui.add(TextField::new(form_text_id, &mut value))
                } else {
                    SelectField::new(form_select_id, "Control")
                        .show_ui(ui, |_| ())
                        .response
                }
            },
        );
    [
        native,
        hunter,
        primary,
        text,
        select,
        native_field,
        form.inner[0].rect.height(),
        form.inner[1].rect.height(),
    ]
}

#[test]
fn native_and_hunter_controls_share_density_without_changing_fonts_or_siblings() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let pixels_per_point = ctx.pixels_per_point();
    let mut standard = [0.0; 8];
    let mut compact = [0.0; 8];
    render(&ctx, vec![], |ui| {
        let inherited = ui.style().clone();
        standard = ui.push_id("before", control_heights).inner;
        Density::Compact.scope(ui, |ui| {
            assert_eq!(Density::get(ui), Density::Compact);
            assert_eq!(ui.style().text_styles, inherited.text_styles);
            assert_eq!(ui.style().override_font_id, inherited.override_font_id);
            compact = ui.push_id("compact", control_heights).inner;
            Density::Standard.scope(ui, |ui| {
                assert_eq!(Density::get(ui), Density::Standard);
                let nested = ui.push_id("nested-standard", control_heights).inner;
                for (actual, expected) in nested.into_iter().zip(standard) {
                    close(actual, expected);
                }
            });
            assert_eq!(Density::get(ui), Density::Compact);
            close(
                ui.button("Compact after nested scope").rect.height(),
                compact[0],
            );
        });
        assert_eq!(Density::get(ui), Density::Standard);
        assert_eq!(ui.spacing().interact_size, inherited.spacing.interact_size);
        assert_eq!(
            ui.spacing().button_padding,
            inherited.spacing.button_padding
        );
        close(ui.button("Standard sibling").rect.height(), standard[0]);
    });
    for (actual, expected) in standard
        .into_iter()
        .zip([36.0, 36.0, 44.0, 40.0, 40.0, 40.0, 40.0, 40.0])
    {
        close(actual, expected);
    }
    for (actual, expected) in compact
        .into_iter()
        .zip([24.0, 24.0, 28.0, 28.0, 28.0, 28.0, 28.0, 28.0])
    {
        close(actual, expected);
    }
    assert_eq!(ctx.pixels_per_point(), pixels_per_point);
    assert_eq!(Tokens::from_context(&ctx).density, Density::Standard);
}

#[test]
fn compact_controls_honor_larger_local_minimum_heights() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    render(&ctx, vec![], |ui| {
        Density::Compact.scope(ui, |ui| {
            ui.spacing_mut().interact_size.y = 64.0;
            for height in control_heights(ui) {
                assert!(height >= 64.0);
            }
        });
    });
}

#[test]
fn tab_geometry_contains_large_text_and_uses_the_local_button_padding() {
    for density in [Density::Standard, Density::Compact] {
        let ctx = Context::default();
        Theme::default().apply(&ctx);
        let tabs_id = Id::new("density-tabs");
        let tab_id = Id::new("first");
        let mut state = NavigationState::default();
        let mut response = None;
        let mut padding = egui::Vec2::ZERO;
        let output = render(&ctx, vec![], |ui| {
            density.scope(ui, |ui| {
                ui.style_mut().override_font_id = Some(FontId::proportional(48.0));
                padding = ui.spacing().button_padding;
                Tabs::new(tabs_id).show(
                    ui,
                    &mut state,
                    &[Tab::new(tab_id, "Large tab")],
                    |_, _| (),
                );
                response = ctx.read_response(tabs_id.with(("header", tab_id)));
            });
        });
        let response = response.unwrap();
        let shape = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Text(text) if text.galley.job.text == "Large tab" => Some((shape, text)),
                _ => None,
            })
            .unwrap();
        let text_rect = Rect::from_min_size(shape.1.pos, shape.1.galley.size());
        assert!(response.rect.contains_rect(text_rect));
        assert!(shape.0.clip_rect.contains_rect(text_rect));
        close(response.rect.width(), text_rect.width() + padding.x * 2.0);
        assert!(response.rect.height() >= text_rect.height() + padding.y * 2.0);
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            Shape::LineSegment { points, stroke }
                if stroke.width == 2.0
                    && (points[0].x - (response.rect.left() + padding.x)).abs() < 0.1
                    && (points[1].x - (response.rect.right() - padding.x)).abs() < 0.1
        )));
    }
}

fn password_frame(
    ctx: &Context,
    density: Density,
    value: &mut String,
    visible: &mut bool,
    icon_size: Option<f32>,
    enabled: bool,
    events: Vec<Event>,
) -> (Response, Response, FullOutput) {
    let id = Id::new("density-password");
    let mut editor = None;
    let mut button = None;
    let output = render(ctx, events, |ui| {
        density.scope(ui, |ui| {
            if let Some(size) = icon_size {
                ui.spacing_mut().icon_width_inner = size;
            }
            editor =
                Some(ui.add_enabled(enabled, TextField::new(id, value).password_visible(visible)));
            button = ui.ctx().read_response(id.with("visibility"));
            ui.add(Button::new("After").id(Id::new("after-density-password")));
        });
    });
    (editor.unwrap(), button.unwrap(), output)
}

#[test]
fn password_icons_fit_their_click_region_and_keep_one_inside_focus_border() {
    for (density, minimum_width, minimum_height) in [
        (Density::Standard, 28.0, 40.0),
        (Density::Compact, 24.0, 28.0),
    ] {
        for icon_size in [None, Some(48.0)] {
            let ctx = Context::default();
            Theme::default().apply(&ctx);
            let mut value = "A long visible password used to inspect clipping".to_owned();
            let mut visible = true;
            password_frame(
                &ctx,
                density,
                &mut value,
                &mut visible,
                icon_size,
                true,
                vec![],
            );
            ctx.memory_mut(|memory| {
                memory.request_focus(Id::new("density-password").with("visibility"))
            });
            let (editor, button, output) = password_frame(
                &ctx,
                density,
                &mut value,
                &mut visible,
                icon_size,
                true,
                vec![],
            );
            assert!(editor.rect.contains_rect(button.rect));
            assert!(button.rect.width() >= minimum_width);
            if let Some(size) = icon_size {
                assert!(button.rect.width() >= size);
                assert!(button.rect.height() >= size);
            } else {
                close(editor.rect.height(), minimum_height);
                close(button.rect.width(), minimum_width);
            }
            let mut focus_borders = 0;
            let mut text_seen = false;
            for shape in output.shapes {
                match shape.shape {
                    Shape::Text(text) if text.galley.job.text == value => {
                        assert!(shape.clip_rect.right() <= button.rect.left());
                        text_seen = true;
                    }
                    Shape::Rect(rect)
                        if rect.stroke.width == 2.0
                            && rect.stroke.color == Tokens::from_context(&ctx).focus =>
                    {
                        assert_eq!(rect.rect, editor.rect);
                        assert_eq!(rect.stroke_kind, egui::StrokeKind::Inside);
                        focus_borders += 1;
                    }
                    _ => {}
                }
            }
            assert!(text_seen);
            assert_eq!(focus_borders, 1);
        }
    }
}

#[test]
fn compact_password_keeps_pointer_tab_and_disabled_behavior() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let id = Id::new("density-password");
    let visibility_id = id.with("visibility");
    let mut value = "secret".to_owned();
    let mut visible = false;
    let mut frame = |events, enabled| {
        password_frame(
            &ctx,
            Density::Compact,
            &mut value,
            &mut visible,
            None,
            enabled,
            events,
        )
    };
    let (_, button, _) = frame(vec![], true);
    for pressed in [true, false] {
        let (editor, _, _) = frame(events::pointer(button.rect.center(), pressed), true);
        assert!(!editor.clicked());
        assert!(!editor.dragged());
        assert!(!editor.changed());
    }
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(visibility_id));
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(vec![events::key(Key::Tab)], true);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(visibility_id));
    let (editor, _, _) = frame(vec![events::key(Key::Enter)], true);
    assert!(!editor.changed());
    frame(vec![events::key(Key::Tab)], true);
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(Id::new("after-density-password"))
    );
    ctx.memory_mut(|memory| memory.request_focus(visibility_id));
    let (editor, button, _) = frame(vec![events::key(Key::Enter)], false);
    assert!(!editor.enabled());
    assert!(!button.enabled());
    assert_ne!(ctx.memory(|memory| memory.focused()), Some(visibility_id));
    assert_eq!(value, "secret");
    assert!(
        !visible,
        "pointer and Enter toggle twice; disabled Enter must not toggle"
    );
}

#[test]
fn compact_selection_popup_preserves_density_and_native_popup_overrides() {
    for custom_popup in [false, true] {
        let ctx = Context::default();
        Theme::default().apply(&ctx);
        let mut response: Option<Response> = None;
        let mut menu = None;
        for events in [vec![], vec![events::key(Key::Enter)]] {
            if let Some(response) = &response {
                response.request_focus();
            }
            render(&ctx, events, |ui| {
                Density::Compact.scope(ui, |ui| {
                    let mut field = SelectField::new(Id::new("density-menu"), "Current");
                    if custom_popup {
                        field.native = field.native.popup_style(egui::style::StyleModifier::from(
                            |style: &mut egui::Style| {
                                style.spacing.button_padding.y = 11.0;
                                // An intentional override may equal the closed field height.
                                style.spacing.interact_size.y = 28.0;
                            },
                        ));
                    }
                    response = Some(
                        field
                            .show_ui(ui, |ui| {
                                let density = Density::get(ui);
                                let padding = ui.spacing().button_padding.y;
                                let minimum_height = ui.spacing().interact_size.y;
                                let native = ui.button("Action").rect.height();
                                let primary = ui
                                    .add(Button::new("Action").kind(ButtonKind::Primary))
                                    .rect
                                    .height();
                                let mut value = String::new();
                                let text = ui
                                    .add(TextField::new(Id::new("menu-text"), &mut value))
                                    .rect
                                    .height();
                                menu =
                                    Some((density, padding, minimum_height, native, primary, text));
                            })
                            .response,
                    );
                });
                assert_eq!(Density::get(ui), Density::Standard);
            });
        }
        let (density, padding, minimum_height, native, primary, text) = menu.unwrap();
        assert_eq!(density, Density::Compact);
        close(padding, if custom_popup { 11.0 } else { 3.0 });
        close(minimum_height, if custom_popup { 28.0 } else { 24.0 });
        close(primary, native.max(28.0));
        close(text, 28.0);
        assert_eq!(Tokens::from_context(&ctx).density, Density::Standard);
    }
}
