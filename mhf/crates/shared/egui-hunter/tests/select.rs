mod events;

use egui::{
    Color32, Context, Event, FontId, FullOutput, Id, InnerResponse, Key, RawInput, Rect, Response,
    Shape, Stroke, Ui, pos2, vec2,
};
use egui_hunter::{Field, FormLayout, LabelPlacement, SelectField, TextField, Theme, Validation};

fn render(
    ctx: &Context,
    width: f32,
    events: Vec<Event>,
    mut content: impl FnMut(&mut Ui),
) -> FullOutput {
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
            events,
            ..Default::default()
        },
        |ui| {
            ui.set_width(width);
            content(ui);
        },
    );
    output.textures_delta.clear();
    output
}

fn selection_frame(
    ctx: &Context,
    selected: &mut usize,
    enabled: bool,
    events: Vec<Event>,
) -> (InnerResponse<Option<bool>>, Vec<Response>) {
    let mut output = None;
    let mut options = Vec::new();
    render(ctx, 220.0, events, |ui| {
        options.clear();
        output = Some(
            ui.add_enabled_ui(enabled, |ui| {
                SelectField::new(Id::new("selection"), ["First", "Second"][*selected])
                    .label("Version")
                    .show_ui(ui, |ui| {
                        for (index, label) in ["First", "Second"].into_iter().enumerate() {
                            let option = ui.selectable_value(selected, index, label);
                            if option.clicked() {
                                ui.close();
                            }
                            options.push(option);
                        }
                        options.iter().any(Response::changed)
                    })
            })
            .inner,
        );
        assert!(
            !egui_hunter::consume_escape(ui.ctx()),
            "a dismissed selection must consume Escape before its parent"
        );
    });
    (output.unwrap(), options)
}

#[test]
fn native_keyboard_open_pointer_selection_and_escape_keep_the_menu_contract() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut selected = 0;
    let (closed, _) = selection_frame(&ctx, &mut selected, true, vec![]);
    assert_eq!(closed.inner, None);
    closed.response.request_focus();
    let (opened, _) = selection_frame(&ctx, &mut selected, true, vec![events::key(Key::Enter)]);
    assert_eq!(opened.inner, Some(false));
    assert!(egui::ComboBox::is_open(&ctx, opened.response.id));
    let (_, options) = selection_frame(&ctx, &mut selected, true, vec![]);
    let position = options[1].rect.center();
    selection_frame(&ctx, &mut selected, true, events::pointer(position, true));
    let (changed, _) = selection_frame(&ctx, &mut selected, true, events::pointer(position, false));
    assert_eq!(selected, 1);
    assert_eq!(changed.inner, Some(true));
    assert!(!changed.response.changed());
    assert!(!egui::ComboBox::is_open(&ctx, changed.response.id));

    changed.response.request_focus();
    selection_frame(&ctx, &mut selected, true, vec![events::key(Key::Enter)]);
    selection_frame(&ctx, &mut selected, true, vec![events::key(Key::Escape)]);
    let (closed, _) = selection_frame(&ctx, &mut selected, true, vec![]);
    assert_eq!(closed.inner, None);
    assert_eq!(selected, 1);
}

#[test]
fn disabled_selection_releases_focus_and_ignores_keyboard_and_pointer() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut selected = 0;
    let (control, _) = selection_frame(&ctx, &mut selected, true, vec![]);
    control.response.request_focus();
    let (disabled, _) = selection_frame(&ctx, &mut selected, false, vec![events::key(Key::Enter)]);
    assert!(!disabled.response.enabled());
    assert!(!disabled.response.has_focus());
    assert_eq!(disabled.inner, None);
    for pressed in [true, false] {
        let (control, _) = selection_frame(
            &ctx,
            &mut selected,
            false,
            events::pointer(disabled.response.rect.center(), pressed),
        );
        assert_eq!(control.inner, None);
    }
    assert_eq!(selected, 0);

    let (control, _) = selection_frame(&ctx, &mut selected, true, vec![]);
    control.response.request_focus();
    selection_frame(&ctx, &mut selected, true, vec![events::key(Key::Enter)]);
    let (_, options) = selection_frame(&ctx, &mut selected, true, vec![]);
    let position = options[1].rect.center();
    for pressed in [true, false] {
        let (disabled, options) = selection_frame(
            &ctx,
            &mut selected,
            false,
            events::pointer(position, pressed),
        );
        assert_eq!(disabled.inner, None);
        assert!(options.is_empty());
        assert!(!egui::ComboBox::is_open(&ctx, disabled.response.id));
    }
    assert_eq!(selected, 0);
}

#[test]
fn external_validation_reaches_bare_controls_without_changing_siblings() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let mut invalid = Rect::NOTHING;
    let mut text = Rect::NOTHING;
    let mut ordinary = Rect::NOTHING;
    let mut value = "Hunter".to_owned();
    let output = render(&ctx, 240.0, vec![], |ui| {
        invalid = Field::new(Id::new("version"))
            .label("Version")
            .help("Choose a version")
            .validation(Validation::Error("Required"))
            .show(ui, |ui| {
                SelectField::new(Id::new("version"), "Choose")
                    .show_ui(ui, |_| ())
                    .response
            })
            .rect;
        text = Field::new(Id::new("name"))
            .validation(Validation::Warning("Check the name"))
            .show(ui, |ui| ui.add(TextField::new(Id::new("name"), &mut value)))
            .rect;
        ordinary = SelectField::new(Id::new("ordinary"), "Ordinary")
            .show_ui(ui, |_| ())
            .response
            .rect;
    });
    let mut invalid_borders = Vec::new();
    let mut warning_borders = Vec::new();
    let mut ordinary_borders = Vec::new();
    let mut messages = Vec::new();
    for shape in output.shapes {
        match shape.shape {
            Shape::Rect(shape) if shape.rect == invalid => invalid_borders.push(shape.stroke),
            Shape::Rect(shape) if shape.rect == text && shape.stroke.width > 0.0 => {
                warning_borders.push(shape.stroke)
            }
            Shape::Rect(shape) if shape.rect == ordinary => ordinary_borders.push(shape.stroke),
            Shape::Text(shape) => messages.push(shape.galley.job.text.clone()),
            _ => {}
        }
    }
    assert_eq!(
        invalid_borders,
        [Stroke::new(1.0, theme.style.visuals.error_fg_color)]
    );
    assert_eq!(
        warning_borders,
        [Stroke::new(1.0, theme.style.visuals.warn_fg_color)]
    );
    assert_eq!(
        ordinary_borders,
        [theme.style.visuals.widgets.inactive.bg_stroke]
    );
    assert_eq!(
        messages.iter().filter(|text| *text == "Required").count(),
        1
    );
    assert!(!messages.iter().any(|text| text == "Choose a version"));
}

#[test]
fn native_menu_styling_survives_field_validation_without_inheriting_its_border() {
    for custom_popup in [false, true] {
        let ctx = Context::default();
        let theme = Theme::default();
        theme.apply(&ctx);
        let custom_stroke = Stroke::new(3.0, Color32::LIGHT_BLUE);
        let mut response: Option<Response> = None;
        let mut menu_style = None;
        for events in [vec![], vec![events::key(Key::Enter)]] {
            if let Some(response) = &response {
                response.request_focus();
            }
            render(&ctx, 240.0, events, |ui| {
                let mut field = SelectField::new(Id::new("styled-menu"), "Current")
                    .validation(Validation::Error("Invalid"));
                field.native = field.native.width(160.0);
                if custom_popup {
                    field.native = field.native.popup_style(egui::style::StyleModifier::from(
                        move |style: &mut egui::Style| {
                            style.visuals.widgets.inactive.bg_stroke = custom_stroke;
                            style.spacing.button_padding.y = 13.0;
                        },
                    ));
                }
                response = Some(
                    field
                        .show_ui(ui, |ui| {
                            menu_style = Some((
                                ui.visuals().widgets.inactive.bg_stroke,
                                ui.spacing().button_padding.y,
                            ));
                            let _ = ui.selectable_label(false, "Option");
                        })
                        .response,
                );
            });
            assert_eq!(response.as_ref().unwrap().rect.width(), 160.0);
        }
        let (stroke, padding) = menu_style.unwrap();
        assert_eq!(
            stroke,
            if custom_popup {
                custom_stroke
            } else {
                theme.style.visuals.widgets.inactive.bg_stroke
            }
        );
        assert_eq!(
            padding,
            if custom_popup {
                13.0
            } else {
                theme.style.spacing.button_padding.y
            }
        );
    }
}

#[test]
fn fields_fill_the_column_and_respect_larger_local_control_sizes() {
    for width in [240.0, 100.0] {
        for height in [40.0, 68.0] {
            let ctx = Context::default();
            Theme::default().apply(&ctx);
            render(&ctx, width, vec![], |ui| {
                ui.spacing_mut().interact_size.y = height;
                if height > 40.0 {
                    ui.style_mut().override_font_id = Some(FontId::proportional(28.0));
                }
                let available = ui.available_width();
                let selected = SelectField::new(
                    Id::new("sized-select"),
                    "A selected value that should truncate inside its field",
                )
                .show_ui(ui, |_| ())
                .response;
                assert!((selected.rect.width() - available).abs() <= 1.0);
                assert!(selected.rect.height() >= height);
                let mut value = String::new();
                let text = ui.add(TextField::new(Id::new("sized-text"), &mut value));
                assert!((text.rect.width() - available).abs() <= 1.0);
                assert!(text.rect.height() >= height, "{text:?}; minimum {height}");
            });
        }
    }
}

#[test]
fn open_selection_keeps_its_native_id_when_the_form_reflows() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut value = String::new();
    let mut previous: Option<Response> = None;
    for (width, events) in [
        (600.0, vec![]),
        (600.0, vec![events::key(Key::Enter)]),
        (180.0, vec![]),
        (600.0, vec![]),
    ] {
        if let Some(previous) = &previous {
            previous.request_focus();
        }
        let mut current = None;
        render(&ctx, width, events, |ui| {
            FormLayout::new(Id::new("responsive-form"))
                .max_columns(2)
                .label_placement(LabelPlacement::Left)
                .show(
                    ui,
                    &[
                        Field::new(Id::new("name")).label("Name"),
                        Field::new(Id::new("version")).label("Version"),
                    ],
                    |ui, index| {
                        if index == 0 {
                            ui.add(TextField::new(Id::new("name"), &mut value))
                        } else {
                            let response = SelectField::new(Id::new("version"), "Current")
                                .show_ui(ui, |ui| {
                                    let _ = ui.selectable_label(false, "Option");
                                })
                                .response;
                            current = Some(response.clone());
                            response
                        }
                    },
                );
        });
        let current = current.unwrap();
        if let Some(previous) = previous {
            assert_eq!(current.id, previous.id);
            assert!(egui::ComboBox::is_open(&ctx, current.id));
        }
        previous = Some(current);
    }
}

#[test]
fn selection_metadata_changes_preserve_the_open_menu_and_native_id() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut previous: Option<Response> = None;
    for (validation, help, events) in [
        (Validation::None, None, vec![]),
        (Validation::None, None, vec![events::key(Key::Enter)]),
        (Validation::Error("Required"), None, vec![]),
        (Validation::None, Some("Choose a version"), vec![]),
        (Validation::None, None, vec![]),
    ] {
        if let Some(previous) = &previous {
            previous.request_focus();
        }
        let mut current = None;
        render(&ctx, 240.0, events, |ui| {
            let mut field = SelectField::new(Id::new("dynamic"), "Current").validation(validation);
            if let Some(help) = help {
                field = field.help(help);
            }
            current = Some(
                field
                    .show_ui(ui, |ui| {
                        let _ = ui.selectable_label(false, "Option");
                    })
                    .response,
            );
        });
        let current = current.unwrap();
        if let Some(previous) = previous {
            assert_eq!(current.id, previous.id);
            assert!(egui::ComboBox::is_open(&ctx, current.id));
        }
        previous = Some(current);
    }
}

#[test]
fn keyboard_dismissal_returns_focus_from_the_menu_option_to_the_selection() {
    for (key, option) in [(Key::Escape, 1), (Key::Enter, 1), (Key::Enter, 0)] {
        let ctx = Context::default();
        Theme::default().apply(&ctx);
        let mut selected = 0;
        let (control, _) = selection_frame(&ctx, &mut selected, true, vec![]);
        control.response.request_focus();
        selection_frame(&ctx, &mut selected, true, vec![events::key(Key::Enter)]);
        let (_, options) = selection_frame(&ctx, &mut selected, true, vec![]);
        options[option].request_focus();
        selection_frame(&ctx, &mut selected, true, vec![events::key(key)]);
        let (closed, _) = selection_frame(&ctx, &mut selected, true, vec![]);
        assert_eq!(closed.inner, None, "dismissed by {key:?}");
        assert_eq!(selected, if key == Key::Enter { option } else { 0 });
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(control.response.id)
        );
    }
}
