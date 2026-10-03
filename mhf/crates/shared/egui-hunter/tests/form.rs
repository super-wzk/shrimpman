pub mod events;

use egui::{Context, Id, Rect, Response, Shape, vec2};
use egui_hunter::{Field, FormLayout, LabelPlacement, SelectField, TextField, Validation};

struct Frame {
    controls: Vec<Response>,
    texts: Vec<(String, Rect)>,
    bounds: Rect,
    calls: Vec<usize>,
}

fn frame(
    ctx: &Context,
    width: f32,
    fields: &[Field<'_>],
    layout: impl Fn() -> FormLayout,
    heights: &[f32],
) -> Frame {
    let mut controls = Vec::new();
    let mut bounds = Rect::NOTHING;
    let mut calls = vec![0; fields.len()];
    let (_, output) = events::frame(
        ctx,
        events::input(vec2(960.0, 1200.0), None, vec![]),
        |ui| {
            ui.set_width(width);
            let result = layout().show(ui, fields, |ui, index| {
                calls[index] += 1;
                ui.add(
                    egui::Button::new(format!("control-{index}"))
                        .min_size(vec2(ui.available_width(), heights[index])),
                )
            });
            controls = result.inner;
            bounds = result.response.rect;
        },
    );
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) => Some((
                text.galley.job.text.clone(),
                Rect::from_min_size(text.pos, text.galley.size()),
            )),
            _ => None,
        })
        .collect();
    Frame {
        controls,
        texts,
        bounds,
        calls,
    }
}

fn text_rect(frame: &Frame, value: &str) -> Rect {
    frame
        .texts
        .iter()
        .find_map(|(text, rect)| (text == value).then_some(*rect))
        .unwrap_or_else(|| panic!("missing text {value:?}"))
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() <= 1.0, "{a} != {b}");
}

#[test]
fn above_labels_reserve_the_tallest_label_and_feedback_in_each_row() {
    let ctx = events::themed_context();
    let long_label = "A label which wraps onto several lines to describe this input";
    let error =
        "Please choose another value because this one cannot be used for the selected operation.";
    let fields = [
        Field::new(Id::new("first"))
            .label("Short")
            .validation(Validation::Error(error)),
        Field::new(Id::new("second")).label(long_label),
        Field::new(Id::new("third")),
        Field::new(Id::new("fourth")).label("Fourth"),
    ];
    let result = frame(
        &ctx,
        420.0,
        &fields,
        || {
            FormLayout::new(Id::new("columns-form"))
                .max_columns(2)
                .min_column_width(180.0)
        },
        &[40.0; 4],
    );
    assert!(text_rect(&result, long_label).height() > text_rect(&result, "Short").height());
    close(result.controls[0].rect.top(), result.controls[1].rect.top());
    close(result.controls[2].rect.top(), result.controls[3].rect.top());
    assert!(result.controls[2].rect.top() > text_rect(&result, error).bottom());
    assert!(result.bounds.contains_rect(result.controls[3].rect));
    assert_eq!(result.calls, [1, 1, 1, 1]);
}

#[test]
fn left_labels_share_control_starts_when_one_label_wraps_past_control_height() {
    let ctx = events::themed_context();
    let long_label = "A left label with enough words to span many lines";
    let fields = [
        Field::new(Id::new("wrapped-left")).label(long_label),
        Field::new(Id::new("short-left")).label("Short"),
    ];
    let result = frame(
        &ctx,
        640.0,
        &fields,
        || {
            FormLayout::new(Id::new("wrapped-left-form"))
                .max_columns(2)
                .label_placement(LabelPlacement::Left)
                .label_width(90.0)
        },
        &[40.0; 2],
    );
    assert!(text_rect(&result, long_label).height() > result.controls[0].rect.height());
    close(result.controls[0].rect.top(), result.controls[1].rect.top());
    close(
        text_rect(&result, long_label).center().y,
        result.controls[0].rect.center().y,
    );
    close(
        text_rect(&result, "Short").center().y,
        result.controls[1].rect.center().y,
    );
}

#[test]
fn narrowing_and_reordering_preserve_native_control_ids_and_focus() {
    let ctx = events::themed_context();
    let fields = [
        Field::new(Id::new("account")).label("Account"),
        Field::new(Id::new("password")).label("Password"),
    ];
    let layout = || {
        FormLayout::new(Id::new("responsive-form"))
            .max_columns(2)
            .min_column_width(260.0)
            .label_placement(LabelPlacement::Left)
            .label_width(100.0)
    };
    let wide = frame(&ctx, 640.0, &fields, layout, &[40.0; 2]);
    ctx.memory_mut(|memory| memory.request_focus(wide.controls[1].id));
    let narrow = frame(&ctx, 180.0, &fields, layout, &[40.0; 2]);
    for index in 0..2 {
        assert_eq!(wide.controls[index].id, narrow.controls[index].id);
        assert!(narrow.controls[index].rect.width() <= 180.0 + 1.0);
    }
    assert!(text_rect(&narrow, "Account").bottom() < narrow.controls[0].rect.top());
    assert!(narrow.controls[1].rect.top() > narrow.controls[0].rect.bottom());
    assert!(narrow.controls[1].has_focus());
    assert_eq!(narrow.calls, [1, 1]);
    let reordered = frame(&ctx, 640.0, &[fields[1], fields[0]], layout, &[40.0; 2]);
    assert_eq!(reordered.controls[0].id, wide.controls[1].id);
    assert_eq!(reordered.controls[1].id, wide.controls[0].id);
    assert!(reordered.controls[0].has_focus());

    for width in [180.0, 640.0] {
        let zero_width = frame(
            &ctx,
            width,
            &fields,
            || layout().label_width(0.0),
            &[40.0; 2],
        );
        for (index, label) in ["Account", "Password"].iter().enumerate() {
            assert!(text_rect(&zero_width, label).bottom() < zero_width.controls[index].rect.top());
            assert_eq!(zero_width.controls[index].id, wide.controls[index].id);
        }
    }
    let unlabelled = frame(
        &ctx,
        180.0,
        &[Field::new(Id::new("unlabelled"))],
        || layout().label_width(0.0),
        &[40.0],
    );
    close(unlabelled.bounds.top(), unlabelled.controls[0].rect.top());
}

#[test]
fn validation_surrounding_bare_controls_colors_one_border_and_replaces_help() {
    for selection in [false, true] {
        let ctx = events::themed_context();
        let field_id = Id::new("validated-text");
        let mut value = String::new();
        let mut response = None;
        let mut native = None;
        let mut next = None;
        let mut error_color = egui::Color32::TRANSPARENT;
        let fields = [
            Field::new(field_id)
                .label("Version")
                .help("Ordinary help must be hidden")
                .validation(Validation::Error("Invalid version")),
            Field::new(Id::new("after-validation")).label("After"),
        ];
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.set_width(320.0);
            error_color = ui.visuals().error_fg_color;
            let fields =
                FormLayout::new(Id::new("validation-form")).show(ui, &fields, |ui, index| {
                    if index == 0 {
                        let control = if selection {
                            SelectField::new(field_id, "Current")
                                .show_ui(ui, |_| ())
                                .response
                        } else {
                            ui.add(TextField::new(field_id, &mut value))
                        };
                        native = Some(control.clone());
                        control
                    } else {
                        ui.button("After")
                    }
                });
            response = Some(fields.inner[0].clone());
            next = Some(fields.inner[1].clone());
        });
        let response = response.unwrap();
        let native = native.unwrap();
        assert_eq!(response.id, native.id);
        assert_eq!(response.rect, native.rect);
        let mut messages = 0;
        let mut borders = 0;
        for shape in &output.shapes {
            match &shape.shape {
                Shape::Text(text) if text.galley.job.text == "Invalid version" => {
                    messages += 1;
                    assert!(next.as_ref().unwrap().rect.top() > text.pos.y + text.galley.size().y);
                }
                Shape::Text(text) => {
                    assert_ne!(text.galley.job.text, "Ordinary help must be hidden")
                }
                Shape::Rect(rect)
                    if rect.rect == response.rect && rect.stroke.color == error_color =>
                {
                    borders += 1;
                }
                _ => {}
            }
        }
        assert_eq!(messages, 1);
        assert_eq!(borders, 1);
        output.drop_without_applying_deltas();
    }
}
