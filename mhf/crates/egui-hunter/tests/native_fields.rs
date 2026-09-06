mod events;

use egui::{Context, Event, Id, Key, RawInput, Rect, Response, Shape, pos2, vec2};
use egui_hunter::{Icon, TextField, Theme};

struct Frame {
    response: Response,
    icon_clip: Rect,
    text_clip: Rect,
    parent_cancel: bool,
}

fn frame(ctx: &Context, value: &mut String, width: f32, events: Vec<Event>) -> Frame {
    let mut response = None;
    let mut parent_cancel = false;
    let output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
            events,
            ..Default::default()
        },
        |ui| {
            ui.set_width(width);
            response = Some(ui.add(TextField::new(Id::new("search"), value).icon(Icon::Search)));
            let _ = ui.button("Outside");
            parent_cancel = egui_hunter::consume_escape(ui.ctx());
        },
    );
    let mut icon_clip = Rect::NOTHING;
    let mut text_clip = Rect::NOTHING;
    for shape in &output.shapes {
        match &shape.shape {
            Shape::Circle(_) => icon_clip = shape.clip_rect,
            Shape::Text(text) if text.galley.job.text == *value => text_clip = shape.clip_rect,
            _ => {}
        }
    }
    output.drop_without_applying_deltas();
    Frame {
        response: response.unwrap(),
        icon_clip,
        text_clip,
        parent_cancel,
    }
}

#[test]
fn prefix_and_scrolled_text_keep_separate_clip_regions_at_different_widths_and_scales() {
    for scale in [1.0, 2.0] {
        let ctx = Context::default();
        Theme::default().apply(&ctx);
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
    let ctx = Context::default();
    Theme::default().apply(&ctx);
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
    let ctx = Context::default();
    Theme::default().apply(&ctx);
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
