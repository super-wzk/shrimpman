pub mod events;

use egui::{Context, Event, Id, Rect, Ui, Vec2, vec2};
use egui_hunter::{Button, SplitPane};

fn frame<R>(
    ctx: &Context,
    size: Vec2,
    time: f64,
    events: Vec<Event>,
    mut show: impl FnMut(&mut Ui) -> R,
) -> R {
    events::frame(ctx, events::input(size, Some(time), events), |ui| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, &mut show)
            .inner
    })
    .0
}

#[test]
fn both_axes_fill_the_parent_and_keep_stable_child_ids() {
    for horizontal in [true, false] {
        let ctx = events::themed_context();
        let mut draw = |ui: &mut Ui| {
            let pane = if horizontal {
                SplitPane::horizontal(Id::new("split"))
            } else {
                SplitPane::vertical(Id::new("split"))
            };
            pane.default_ratio(0.6)
                .min_sizes(100.0, 80.0)
                .show(ui, |first, second| {
                    (
                        first.add(Button::new("First")),
                        second.add(Button::new("Second")),
                    )
                })
        };
        let large = frame(&ctx, vec2(800.0, 600.0), 0.0, vec![], &mut draw);
        assert_eq!(large.response.rect.size(), vec2(800.0, 600.0));
        assert!((large.ratio - 0.6).abs() < 0.001);
        assert!(large.response.rect.contains_rect(large.first.rect));
        assert!(large.response.rect.contains_rect(large.second.rect));
        assert!(!large.divider.sense.is_focusable());
        ctx.memory_mut(|memory| memory.request_focus(large.inner.0.id));
        let small = frame(&ctx, vec2(400.0, 300.0), 0.1, vec![], &mut draw);
        assert_eq!(small.inner.0.id, large.inner.0.id);
        assert_eq!(small.inner.1.id, large.inner.1.id);
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(large.inner.0.id)
        );
        frame(
            &ctx,
            vec2(400.0, 300.0),
            0.2,
            vec![events::key(egui::Key::Tab)],
            &mut draw,
        );
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(small.inner.1.id)
        );
        if horizontal {
            assert_eq!(small.first.rect.height(), small.response.rect.height());
            assert_eq!(small.second.rect.height(), small.response.rect.height());
            assert_eq!(small.first.rect.right(), small.divider.rect.left());
            assert_eq!(small.second.rect.left(), small.divider.rect.right());
        } else {
            assert_eq!(small.first.rect.width(), small.response.rect.width());
            assert_eq!(small.second.rect.width(), small.response.rect.width());
            assert_eq!(small.first.rect.bottom(), small.divider.rect.top());
            assert_eq!(small.second.rect.top(), small.divider.rect.bottom());
        }
    }
}

#[test]
fn dragging_remembers_the_ratio_and_double_click_restores_the_default() {
    for horizontal in [true, false] {
        let ctx = events::themed_context();
        let size = vec2(800.0, 600.0);
        let mut draw = |ui: &mut Ui| {
            let pane = if horizontal {
                SplitPane::horizontal(Id::new("split"))
            } else {
                SplitPane::vertical(Id::new("split"))
            };
            pane.default_ratio(0.4)
                .min_sizes(80.0, 80.0)
                .show(ui, |first, second| {
                    first.label("First");
                    second.label("Second");
                })
        };
        let initial = frame(&ctx, size, 0.0, vec![], &mut draw);
        let start = initial.divider.rect.center();
        frame(&ctx, size, 0.1, events::pointer(start, true), &mut draw);
        let delta = if horizontal {
            vec2(100.0, 0.0)
        } else {
            vec2(0.0, 100.0)
        };
        let dragged = frame(
            &ctx,
            size,
            0.2,
            vec![Event::PointerMoved(start + delta)],
            &mut draw,
        );
        assert!(dragged.divider.dragged());
        assert!(dragged.response.changed());
        assert!(dragged.ratio > initial.ratio + 0.1);
        frame(
            &ctx,
            size,
            0.3,
            events::pointer(start + delta, false),
            &mut draw,
        );
        let remembered = frame(&ctx, size, 0.4, vec![], &mut draw);
        assert!((remembered.ratio - dragged.ratio).abs() < 0.001);
        let reset = remembered.divider.rect.center();
        for (time, pressed) in [(1.0, true), (1.01, false), (1.1, true), (1.11, false)] {
            frame(&ctx, size, time, events::pointer(reset, pressed), &mut draw);
        }
        let restored = frame(&ctx, size, 1.2, vec![], &mut draw);
        assert!((restored.ratio - 0.4).abs() < 0.001, "{}", restored.ratio);
    }
}

#[test]
fn narrow_parents_shrink_minimums_without_losing_the_preferred_ratio() {
    let ctx = events::themed_context();
    let mut draw = |ui: &mut Ui| {
        SplitPane::horizontal(Id::new("split"))
            .default_ratio(0.7)
            .min_sizes(200.0, 100.0)
            .show(ui, |_, _| {})
    };
    let large = frame(&ctx, vec2(800.0, 400.0), 0.0, vec![], &mut draw);
    let small = frame(&ctx, vec2(240.0, 160.0), 0.1, vec![], &mut draw);
    assert!((small.ratio - 2.0 / 3.0).abs() < 0.001);
    assert!(small.response.rect.contains_rect(small.first.rect));
    assert!(small.response.rect.contains_rect(small.second.rect));
    let restored = frame(&ctx, vec2(800.0, 400.0), 0.2, vec![], &mut draw);
    assert!((restored.ratio - large.ratio).abs() < 0.001);
    let tiny = frame(&ctx, vec2(2.0, 10.0), 0.3, vec![], &mut draw);
    assert_eq!(tiny.first.rect.width(), 0.0);
    assert_eq!(tiny.second.rect.width(), 0.0);
    assert!(tiny.response.rect.contains_rect(tiny.divider.rect));
}

#[test]
fn overflowing_content_cannot_paint_or_capture_input_in_the_other_pane() {
    let ctx = events::themed_context();
    let size = vec2(400.0, 200.0);
    let mut draw = |ui: &mut Ui| {
        SplitPane::horizontal(Id::new("split")).show(ui, |first, second| {
            let clips = [first.clip_rect(), second.clip_rect()];
            let oversized = first.allocate_rect(
                Rect::from_min_size(first.next_widget_position(), vec2(1000.0, 1000.0)),
                egui::Sense::click(),
            );
            first
                .painter()
                .rect_filled(oversized.rect, 0, egui::Color32::RED);
            let details = second.add(Button::new("Details").full_width());
            (clips, oversized, details)
        })
    };
    let initial = frame(&ctx, size, 0.0, vec![], &mut draw);
    assert_eq!(initial.inner.0, [initial.first.rect, initial.second.rect]);
    assert_eq!(initial.response.rect.size(), size);
    let point = initial.inner.2.rect.center();
    assert!(initial.inner.1.rect.contains(point));
    frame(&ctx, size, 0.1, events::pointer(point, true), &mut draw);
    let clicked = frame(&ctx, size, 0.2, events::pointer(point, false), &mut draw);
    assert!(!clicked.inner.1.clicked());
    assert!(clicked.inner.2.clicked());
}

#[test]
fn nested_splits_keep_independent_state_and_inherit_clipping() {
    let ctx = events::themed_context();
    let mut draw = |ui: &mut Ui| {
        SplitPane::vertical(Id::new("outer"))
            .default_ratio(0.75)
            .min_sizes(120.0, 80.0)
            .show(ui, |top, bottom| {
                let inner = SplitPane::horizontal(Id::new("inner"))
                    .default_ratio(0.65)
                    .min_sizes(100.0, 80.0)
                    .show(top, |left, right| [left.clip_rect(), right.clip_rect()]);
                (inner, bottom.clip_rect())
            })
    };
    let output = frame(&ctx, vec2(800.0, 600.0), 0.0, vec![], &mut draw);
    assert!((output.ratio - 0.75).abs() < 0.001);
    assert!((output.inner.0.ratio - 0.65).abs() < 0.001);
    assert_eq!(output.inner.1, output.second.rect);
    assert_eq!(output.inner.0.response.rect, output.first.rect);
    assert!(output.first.rect.contains_rect(output.inner.0.inner[0]));
    assert!(output.first.rect.contains_rect(output.inner.0.inner[1]));
}
