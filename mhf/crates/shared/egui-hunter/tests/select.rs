pub mod events;

use egui::{Context, Event, FullOutput, Id, InnerResponse, Key, Response, Ui, vec2};
use egui_hunter::{Field, FormLayout, LabelPlacement, SelectField, TextField, Validation};

fn render(
    ctx: &Context,
    width: f32,
    events: Vec<Event>,
    mut content: impl FnMut(&mut Ui),
) -> FullOutput {
    events::frame(ctx, events::input(vec2(640.0, 480.0), None, events), |ui| {
        ui.set_width(width);
        content(ui);
    })
    .1
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
    let ctx = events::themed_context();
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
    let ctx = events::themed_context();
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
fn open_selection_keeps_its_native_id_when_the_form_reflows() {
    let ctx = events::themed_context();
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
    let ctx = events::themed_context();
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
        let ctx = events::themed_context();
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
