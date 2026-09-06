mod events;

use egui::{Context, Event, Id, Key, RawInput, Rect, Response, Shape, pos2, vec2};
use egui_hunter::{Button, Panel, ScrollPanel, Theme};

struct Frame {
    panel: Response,
    list: Response,
    inner_rect: Rect,
    offset: f32,
    rows: Vec<Response>,
    after: Response,
    highlights: Vec<(Rect, Rect)>,
    full_focus_frames: usize,
}

fn frame(ctx: &Context, events: Vec<Event>) -> Frame {
    let mut result = None;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 700.0))),
            time: Some(ctx.cumulative_pass_nr() as f64 / 60.0),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let outer = Panel::new("Outer").show(ui, |ui| {
                    let mut rows = Vec::new();
                    let mut panel = ScrollPanel::new(Id::new("records"), "Records");
                    panel.scroll = panel.scroll.max_height(240.0);
                    let panel = panel.show_list(
                        ui,
                        36.0,
                        10_000,
                        |_| true,
                        |ui, row| {
                            let response = ui.add_sized(
                                [ui.available_width(), 36.0],
                                Button::new(&row.to_string())
                                    .id(Id::new(("row", row)))
                                    .sense(egui::Sense::CLICK),
                            );
                            rows.push(response.clone());
                            response
                        },
                    );
                    let after = ui.add(Button::new("Outside"));
                    result = Some(Frame {
                        panel: panel.response,
                        list: panel.inner.response,
                        inner_rect: panel.inner.scroll.inner_rect,
                        offset: panel.inner.scroll.state.offset.y,
                        rows,
                        after,
                        highlights: Vec::new(),
                        full_focus_frames: 0,
                    });
                });
                assert!(!outer.response.sense.is_focusable());
            });
        },
    );
    output.textures_delta.clear();
    let mut result = result.unwrap();
    let active = ctx.global_style().visuals.widgets.active.bg_fill;
    for shape in output.shapes {
        collect_focus_shapes(&shape.shape, shape.clip_rect, active, &mut result);
    }
    result
}

fn collect_focus_shapes(shape: &Shape, clip: Rect, active: egui::Color32, frame: &mut Frame) {
    match shape {
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect_focus_shapes(shape, clip, active, frame);
            }
        }
        Shape::Path(path) if path.fill == active => {
            frame
                .highlights
                .push((Rect::from_points(&path.points), clip));
        }
        Shape::Path(path)
            if path.closed
                && path.stroke.color == egui::epaint::ColorMode::Solid(active)
                && path.stroke.width > 0.0 =>
        {
            frame.full_focus_frames += 1;
        }
        _ => {}
    }
}

fn press(ctx: &Context, key: Key) -> Frame {
    frame(
        ctx,
        [true, false]
            .map(|pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .into(),
    )
}

fn assert_highlight(frame: &Frame, target: Rect) {
    assert_eq!(
        frame.full_focus_frames, 0,
        "focus must not recolor a full frame"
    );
    assert_eq!(
        frame.highlights.len(),
        1,
        "only the current part uses the native active fill"
    );
    let (highlight, clip) = frame.highlights[0];
    assert!(target.contains_rect(highlight));
    assert!(clip.contains_rect(highlight));
    assert!(highlight.width() > target.width() * 0.5);
}

#[test]
fn only_the_active_list_row_is_highlighted_inside_decorative_panels() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let initial = frame(&ctx, vec![]);
    assert!(initial.highlights.is_empty());
    initial.list.request_focus();
    let row = frame(&ctx, vec![]);
    assert_highlight(&row, row.rows[0].rect);
    press(&ctx, Key::End);
    let scrolled = frame(&ctx, vec![]);
    assert!(scrolled.offset > 0.0);
    let last = scrolled
        .rows
        .iter()
        .find(|row| row.id == Id::new(("row", 9_999)))
        .unwrap();
    assert_highlight(&scrolled, last.rect);
    assert!(scrolled.panel.rect.contains_rect(scrolled.inner_rect));
    assert!(scrolled.inner_rect.left() > scrolled.panel.rect.left());
    for row in &scrolled.rows {
        assert_eq!(row.interact_rect.x_range(), scrolled.inner_rect.x_range());
    }
    press(&ctx, Key::Tab);
    let after = frame(&ctx, vec![]);
    assert_highlight(&after, after.after.rect);
}

#[test]
fn keyboard_scrolling_highlights_the_viewport_without_recoloring_its_panel() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let draw = |ctx: &Context| {
        let mut response = None;
        let mut viewport = Rect::NOTHING;
        let mut background = egui::Color32::TRANSPARENT;
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let mut panel = ScrollPanel::new(Id::new("scroll-viewport"), "Archive");
            panel.scroll = panel.scroll.max_height(140.0);
            let output = panel.show_rows(ui, 30.0, 10_000, |ui, rows| {
                background = ui.stack().bg_color();
                for row in rows {
                    ui.add_sized([100.0, 30.0], egui::Label::new(format!("Archive {row}")));
                }
            });
            viewport = output.inner.inner_rect;
            response = Some(output.response);
        });
        output.textures_delta.clear();
        (output, response.unwrap(), viewport, background)
    };
    let (_, response, _, _) = draw(&ctx);
    response.request_focus();
    let (output, response, viewport, background) = draw(&ctx);
    assert!(response.has_focus());
    assert!(response.sense.is_focusable());
    assert!(response.rect.contains_rect(viewport));
    assert_eq!(response.rect.y_range(), viewport.y_range());
    let active = ctx.global_style().visuals.widgets.active.bg_fill;
    let mut highlights = Vec::new();
    fn collect(shape: &Shape, active: egui::Color32, highlights: &mut Vec<Rect>) {
        match shape {
            Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, active, highlights);
                }
            }
            Shape::Path(path) if path.fill == active => {
                highlights.push(Rect::from_points(&path.points));
            }
            _ => {}
        }
    }
    for shape in output.shapes {
        collect(&shape.shape, active, &mut highlights);
    }
    assert_eq!(highlights, [viewport.shrink(0.5)]);
    assert_eq!(background, active);
}

#[test]
fn scrolling_focus_belongs_to_the_viewport_and_child_clicks_keep_their_target() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let draw = |events| {
        let mut response = None;
        let output = ctx.run_ui(
            RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                let mut panel = ScrollPanel::new(Id::new("scroll-pointer"), "Archive");
                panel.scroll = panel.scroll.max_height(140.0);
                response = Some(panel.show(ui, |ui| {
                    let child = ui.add(Button::new("Child"));
                    ui.add_space(400.0);
                    child
                }));
            },
        );
        output.drop_without_applying_deltas();
        response.unwrap()
    };
    draw(vec![]);
    let initial = draw(vec![]);
    let title = initial.response.rect.center_top() - vec2(0.0, 20.0);
    for pressed in [true, false] {
        let output = draw(events::pointer(title, pressed));
        assert!(!output.response.clicked());
        assert_ne!(
            ctx.memory(|memory| memory.focused()),
            Some(output.response.id)
        );
    }
    let child = initial.inner.inner.rect.center();
    draw(events::pointer(child, true));
    let clicked = draw(events::pointer(child, false));
    assert!(clicked.inner.inner.clicked());
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(clicked.inner.inner.id)
    );
    let blank = initial.inner.inner_rect.right_center() - vec2(20.0, 0.0);
    draw(events::pointer(blank, true));
    let clicked = draw(events::pointer(blank, false));
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(clicked.response.id)
    );
    let tab = draw(vec![events::key(Key::Tab)]);
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(tab.inner.inner.id)
    );
}
