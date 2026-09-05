mod events;

use egui::{Context, Event, Id, Key, RawInput, Rect, Response, pos2, vec2};
use egui_hunter::Theme;
use events::{key, pointer};

struct ListFrame {
    viewport: Rect,
    offset: f32,
    rows: Vec<(usize, Response)>,
    activated: Vec<usize>,
    after: Response,
}

fn frame(
    ctx: &Context,
    events: Vec<Event>,
    count: usize,
    enabled: impl Fn(usize) -> bool,
) -> ListFrame {
    let theme = Theme::default();
    let mut result = None;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 700.0))),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut rows = Vec::new();
                let mut activated = Vec::new();
                let list = theme
                    .scroll_panel(Id::new("archive"), "委托档案")
                    .max_height(258.0)
                    .show_list(ui, 36.0, count, &enabled, |ui, row| {
                        let response = ui.add_sized(
                            [ui.available_width(), 36.0],
                            theme.button(&format!("第 {:05} 号委托", row + 1)),
                        );
                        if response.clicked() {
                            activated.push(row);
                        }
                        rows.push((row, response.clone()));
                        response
                    })
                    .inner;
                let after = ui.add(theme.button("播放营地消息").id(Id::new("camp-messages")));
                result = Some(ListFrame {
                    viewport: list.inner_rect,
                    offset: list.state.offset.y,
                    rows,
                    activated,
                    after,
                });
            });
        },
    );
    output.textures_delta.clear();
    result.unwrap()
}

fn assert_focused_row(ctx: &Context, frame: &ListFrame, index: usize) {
    let (_, row) = frame
        .rows
        .iter()
        .find(|(row, _)| *row == index)
        .expect("the focused row must be rendered");
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(row.id),
        "row {index} must keep focus"
    );
    assert!(
        frame.viewport.expand(1.0).contains_rect(row.rect),
        "row {index}: {:?}, viewport {:?}",
        row.rect,
        frame.viewport
    );
    assert!(
        frame.rows.len() < 12,
        "navigation must preserve virtualization"
    );
    assert!(!frame.after.clicked());
}

#[test]
fn tab_then_down_crosses_row_six_and_scrolls_instead_of_focusing_the_next_panel() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    frame(&ctx, vec![], 10_000, |_| true);
    let first = frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    assert_focused_row(&ctx, &first, 0);
    for index in 1..24 {
        let output = frame(&ctx, vec![key(Key::ArrowDown)], 10_000, |_| true);
        assert_focused_row(&ctx, &output, index);
        if index >= 6 {
            assert!(output.offset > 0.0);
        }
        let idle = frame(&ctx, vec![], 10_000, |_| true);
        assert_focused_row(&ctx, &idle, index);
    }
    let output = frame(&ctx, vec![key(Key::Enter)], 10_000, |_| true);
    assert_eq!(output.activated, [23]);
    for index in (0..23).rev() {
        let output = frame(&ctx, vec![key(Key::ArrowUp)], 10_000, |_| true);
        assert_focused_row(&ctx, &output, index);
    }
}

#[test]
fn list_navigation_clamps_at_logical_ends_and_handles_page_jumps_and_shrinking_data() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    frame(&ctx, vec![], 10_000, |_| true);
    frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    let output = frame(&ctx, vec![key(Key::ArrowUp)], 10_000, |_| true);
    assert_focused_row(&ctx, &output, 0);
    let output = frame(&ctx, vec![key(Key::PageDown)], 10_000, |_| true);
    assert_focused_row(&ctx, &output, 5);
    let output = frame(&ctx, vec![key(Key::PageUp)], 10_000, |_| true);
    assert_focused_row(&ctx, &output, 0);
    for event in [Key::End, Key::ArrowDown] {
        let output = frame(&ctx, vec![key(event)], 10_000, |_| true);
        assert_focused_row(&ctx, &output, 9_999);
    }
    let output = frame(&ctx, vec![], 4, |_| true);
    assert_focused_row(&ctx, &output, 3);
    frame(&ctx, vec![], 0, |_| true);
    assert!(ctx.memory(|m| m.focused()).is_none());
}

#[test]
fn disabled_offscreen_rows_are_skipped_without_rendering_them() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let enabled = |row| row != 0 && row != 9_999 && !(3..500).contains(&row);
    frame(&ctx, vec![], 10_000, enabled);
    let output = frame(&ctx, vec![key(Key::Tab)], 10_000, enabled);
    assert_focused_row(&ctx, &output, 1);
    for expected in [2, 500] {
        let output = frame(&ctx, vec![key(Key::ArrowDown)], 10_000, enabled);
        assert_focused_row(&ctx, &output, expected);
    }
    let output = frame(&ctx, vec![key(Key::ArrowUp)], 10_000, enabled);
    assert_focused_row(&ctx, &output, 2);
    let output = frame(&ctx, vec![key(Key::Home)], 10_000, enabled);
    assert_focused_row(&ctx, &output, 1);
    let output = frame(&ctx, vec![key(Key::End)], 10_000, enabled);
    assert_focused_row(&ctx, &output, 9_998);
    let output = frame(&ctx, vec![key(Key::ArrowDown)], 10_000, enabled);
    assert_focused_row(&ctx, &output, 9_998);
}

#[test]
fn tab_can_leave_the_list_and_external_clicks_are_not_overridden() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    frame(&ctx, vec![], 10_000, |_| true);
    frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    frame(&ctx, vec![key(Key::End)], 10_000, |_| true);
    // The native Tab order includes the scroll viewport after the final row.
    frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    let output = frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    assert_eq!(ctx.memory(|m| m.focused()), Some(output.after.id));
    let output = frame(&ctx, vec![key(Key::Home)], 10_000, |_| true);
    assert_eq!(ctx.memory(|m| m.focused()), Some(output.after.id));
    let last_id = output
        .rows
        .iter()
        .find(|(row, _)| *row == 9_999)
        .unwrap()
        .1
        .id;
    ctx.memory_mut(|m| m.request_focus(last_id));
    let pos = output.after.rect.center();
    for pressed in [true, false] {
        let output = frame(&ctx, pointer(pos, pressed), 10_000, |_| true);
        if !pressed {
            assert!(output.after.clicked());
        }
    }
    let output = frame(&ctx, vec![], 10_000, |_| true);
    assert_ne!(ctx.memory(|m| m.focused()), Some(last_id));
    assert!(output.activated.is_empty());
}
