pub mod events;

use egui::{Context, Event, Id, Key, Rect, Response, Shape, vec2};
use egui_hunter::{Density, Icon, TextField, Theme};

struct Frame {
    response: Response,
    icon_clip: Rect,
    text_clip: Rect,
    parent_cancel: bool,
}

fn frame(ctx: &Context, value: &mut String, width: f32, events: Vec<Event>) -> Frame {
    let ((response, parent_cancel), output) =
        events::frame(ctx, events::input(vec2(640.0, 480.0), None, events), |ui| {
            ui.set_width(width);
            let response = ui.add(TextField::new(Id::new("search"), value).icon(Icon::Search));
            let _ = ui.button("Outside");
            (response, egui_hunter::consume_escape(ui.ctx()))
        });
    let mut icon_clip = Rect::NOTHING;
    let mut text_clip = Rect::NOTHING;
    for shape in &output.shapes {
        match &shape.shape {
            Shape::Circle(_) => icon_clip = shape.clip_rect,
            Shape::Text(text) if text.galley.job.text == *value => text_clip = shape.clip_rect,
            _ => {}
        }
    }
    Frame {
        response,
        icon_clip,
        text_clip,
        parent_cancel,
    }
}

#[test]
fn prefix_and_scrolled_text_keep_separate_clip_regions_at_different_widths_and_scales() {
    for scale in [1.0, 2.0] {
        let ctx = events::themed_context();
        ctx.set_pixels_per_point(scale);
        let mut value = "A long record name that must scroll inside the search field".to_owned();
        for width in [240.0, 96.0] {
            frame(&ctx, &mut value, width, vec![]);
            ctx.memory_mut(|memory| memory.request_focus(Id::new("search")));
            frame(&ctx, &mut value, width, vec![events::key(Key::End)]);
            let output = frame(&ctx, &mut value, width, vec![]);
            assert!(output.icon_clip.is_positive());
            assert!(output.response.rect.contains_rect(output.icon_clip));
            assert!(output.text_clip.is_positive());
            assert!(output.text_clip.left() >= output.icon_clip.right());
            assert!(output.response.rect.width() <= width + 0.1);
        }
    }
}

#[test]
fn clicking_prefix_focuses_editor_without_adding_a_tab_stop_and_escape_is_consumed() {
    let ctx = events::themed_context();
    let mut value = "Search".to_owned();
    frame(&ctx, &mut value, 200.0, vec![]);
    let output = frame(&ctx, &mut value, 200.0, vec![]);
    frame(
        &ctx,
        &mut value,
        200.0,
        events::pointer(output.icon_clip.center(), true),
    );
    frame(
        &ctx,
        &mut value,
        200.0,
        events::pointer(output.icon_clip.center(), false),
    );
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(Id::new("search"))
    );
    let output = frame(&ctx, &mut value, 200.0, vec![events::key(Key::Escape)]);
    assert!(output.response.lost_focus());
    assert!(!output.parent_cancel);

    ctx.memory_mut(|memory| memory.request_focus(Id::new("search")));
    frame(&ctx, &mut value, 200.0, vec![]);
    frame(&ctx, &mut value, 200.0, vec![events::key(Key::Tab)]);
    let focused = ctx.memory(|memory| memory.focused()).unwrap();
    assert_ne!(focused, Id::new("search"));
    let outside = ctx.read_response(focused).unwrap();
    assert!(outside.rect.top() >= output.response.rect.bottom());
}

#[test]
fn confirming_a_single_line_keeps_focus_and_tab_continues_to_the_next_control() {
    let ctx = events::themed_context();
    let mut value = "Hunter".to_owned();
    frame(&ctx, &mut value, 200.0, vec![]);
    ctx.memory_mut(|memory| memory.request_focus(Id::new("search")));
    frame(&ctx, &mut value, 200.0, vec![Event::Text(" name".into())]);
    let confirmed = frame(&ctx, &mut value, 200.0, vec![events::key(Key::Enter)]);
    assert_eq!(value, "Hunter name");
    assert!(confirmed.response.has_focus());
    assert!(!confirmed.response.lost_focus());
    frame(&ctx, &mut value, 200.0, vec![events::key(Key::Tab)]);
    let next = ctx.memory(|memory| memory.focused()).unwrap();
    assert_ne!(next, confirmed.response.id);
    assert!(ctx.read_response(next).unwrap().rect.top() >= confirmed.response.rect.bottom());
}

fn password_frame(
    ctx: &Context,
    value: &mut String,
    visible: &mut bool,
    width: f32,
    enabled: bool,
    events: Vec<Event>,
) -> (Response, Response, Vec<egui::epaint::ClippedShape>) {
    let id = Id::new("password");
    let ((editor, reveal), output) =
        events::frame(ctx, events::input(vec2(640.0, 480.0), None, events), |ui| {
            ui.set_width(width);
            let editor =
                ui.add_enabled(enabled, TextField::new(id, value).password_visible(visible));
            let reveal = ctx.read_response(id.with("visibility"));
            ui.add(egui_hunter::Button::new("After").id(Id::new("after-password")));
            (editor, reveal)
        });
    (editor, reveal.unwrap(), output.shapes)
}

#[test]
fn embedded_password_button_owns_pointer_and_keyboard_activation_without_editing_text() {
    for density in [Density::Standard, Density::Compact] {
        let ctx = Context::default();
        Theme::default().density(density).apply(&ctx);
        let id = Id::new("password");
        let visibility_id = id.with("visibility");
        let mut value = "secret".to_owned();
        let mut visible = false;
        password_frame(&ctx, &mut value, &mut visible, 240.0, true, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(id));
        let (_, button, _) = password_frame(
            &ctx,
            &mut value,
            &mut visible,
            240.0,
            true,
            vec![events::key(Key::Home)],
        );
        let cursor = egui::text_edit::TextEditState::load(&ctx, id)
            .unwrap()
            .cursor
            .char_range();
        for pressed in [true, false] {
            let (editor, _, _) = password_frame(
                &ctx,
                &mut value,
                &mut visible,
                240.0,
                true,
                events::pointer(button.rect.center(), pressed),
            );
            assert!(!editor.clicked());
            assert!(!editor.dragged());
            assert!(!editor.changed());
        }
        assert!(visible);
        assert_eq!(value, "secret");
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(visibility_id));
        assert_eq!(
            egui::text_edit::TextEditState::load(&ctx, id)
                .unwrap()
                .cursor
                .char_range(),
            cursor
        );

        ctx.memory_mut(|memory| memory.request_focus(id));
        password_frame(
            &ctx,
            &mut value,
            &mut visible,
            240.0,
            true,
            vec![events::key(Key::Tab)],
        );
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(visibility_id));
        let (editor, _, _) = password_frame(
            &ctx,
            &mut value,
            &mut visible,
            240.0,
            true,
            vec![events::key(Key::Enter)],
        );
        assert!(!visible);
        assert!(!editor.changed());
        let (editor, _, _) = password_frame(
            &ctx,
            &mut value,
            &mut visible,
            240.0,
            true,
            vec![events::key(Key::Space), Event::Text(" ".into())],
        );
        assert!(visible);
        assert!(!editor.changed());
        assert_eq!(value, "secret");
        password_frame(
            &ctx,
            &mut value,
            &mut visible,
            240.0,
            true,
            vec![events::key(Key::Tab)],
        );
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(Id::new("after-password"))
        );

        ctx.memory_mut(|memory| memory.request_focus(visibility_id));
        let (editor, button, _) = password_frame(
            &ctx,
            &mut value,
            &mut visible,
            240.0,
            false,
            vec![events::key(Key::Enter)],
        );
        assert!(!editor.enabled(), "editor must be disabled: {editor:?}");
        assert!(!button.enabled(), "visibility must be disabled: {button:?}");
        assert_ne!(ctx.memory(|memory| memory.focused()), Some(visibility_id));
        for pressed in [true, false] {
            password_frame(
                &ctx,
                &mut value,
                &mut visible,
                240.0,
                false,
                events::pointer(button.rect.center(), pressed),
            );
        }
        assert!(visible);
        assert_eq!(value, "secret");
    }
}

#[test]
fn embedded_password_button_reserves_text_space_and_focus_stays_inside_one_field_border() {
    for density in [Density::Standard, Density::Compact] {
        for scale in [1.0, 2.0] {
            let ctx = Context::default();
            Theme::default().density(density).apply(&ctx);
            ctx.set_pixels_per_point(scale);
            let id = Id::new("password");
            let mut value =
                "A long password that must scroll without entering the eye button".to_owned();
            let mut visible = true;
            for width in [240.0, 120.0, 80.0] {
                password_frame(&ctx, &mut value, &mut visible, width, true, vec![]);
                for focus_id in [id, id.with("visibility")] {
                    ctx.memory_mut(|memory| memory.request_focus(focus_id));
                    let (editor, button, shapes) = password_frame(
                        &ctx,
                        &mut value,
                        &mut visible,
                        width,
                        true,
                        vec![events::key(Key::End)],
                    );
                    assert!(editor.rect.contains_rect(button.rect));
                    assert!(editor.rect.width() <= width + 0.1);
                    let mut focus_borders = 0;
                    let mut text_seen = false;
                    for shape in shapes {
                        match shape.shape {
                            Shape::Text(text) if text.galley.job.text == value => {
                                assert!(shape.clip_rect.right() <= button.rect.left());
                                text_seen = true;
                            }
                            Shape::Rect(rect)
                                if rect.stroke.width == 2.0
                                    && rect.stroke.color
                                        == egui_hunter::Tokens::from_context(&ctx).focus =>
                            {
                                assert_eq!(rect.rect, editor.rect);
                                assert_eq!(rect.stroke_kind, egui::StrokeKind::Inside);
                                focus_borders += 1;
                            }
                            Shape::Rect(rect) if rect.stroke.width > 0.0 => {
                                assert_ne!(rect.stroke_kind, egui::StrokeKind::Outside)
                            }
                            _ => {}
                        }
                    }
                    assert!(text_seen);
                    assert_eq!(focus_borders, 1);
                }
            }
        }
    }
}
