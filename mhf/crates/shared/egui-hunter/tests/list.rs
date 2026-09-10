mod events;

use egui::{Context, Event, Id, Key, RawInput, Rect, Response, pos2, vec2};
use egui_hunter::{Button, ScrollPanel, Theme};
use events::{key, pointer};

struct ListFrame {
    accessibility: Option<egui::accesskit::TreeUpdate>,
    container: Response,
    active: Option<usize>,
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
                let mut panel = ScrollPanel::new(Id::new("archive"), "委托档案");
                panel.scroll = panel.scroll.max_height(258.0);
                let list = panel
                    .show_list(ui, 36.0, count, &enabled, |ui, row| {
                        let response = ui.add_sized(
                            [ui.available_width(), 36.0],
                            Button::new(&format!("第 {:05} 号委托", row + 1))
                                .selected(row == 2)
                                .sense(egui::Sense::CLICK),
                        );
                        rows.push((row, response.clone()));
                        response
                    })
                    .inner;
                let after = ui.add(Button::new("播放营地消息").id(Id::new("camp-messages")));
                result = Some(ListFrame {
                    accessibility: None,
                    container: list.response,
                    active: list.active,
                    viewport: list.scroll.inner_rect,
                    offset: list.scroll.state.offset.y,
                    rows,
                    activated: list.activated.into_iter().collect(),
                    after,
                });
            });
        },
    );
    let mut result = result.unwrap();
    result.accessibility = output.platform_output.accesskit_update.take();
    output.drop_without_applying_deltas();
    result
}

fn assert_active_row(ctx: &Context, frame: &ListFrame, index: usize) {
    let (_, row) = frame
        .rows
        .iter()
        .find(|(row, _)| *row == index)
        .expect("the focused row must be rendered");
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(frame.container.id),
        "the list keeps the only keyboard focus while row {index} is active"
    );
    assert_eq!(frame.active, Some(index));
    assert!(
        frame
            .rows
            .iter()
            .all(|(_, row)| !row.sense.is_focusable() && !row.has_focus())
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
    assert_active_row(&ctx, &first, 0);
    for index in 1..24 {
        let output = frame(&ctx, vec![key(Key::ArrowDown)], 10_000, |_| true);
        assert_active_row(&ctx, &output, index);
        if index >= 6 {
            assert!(output.offset > 0.0);
        }
        let idle = frame(&ctx, vec![], 10_000, |_| true);
        assert_active_row(&ctx, &idle, index);
    }
    let output = frame(&ctx, vec![key(Key::Enter)], 10_000, |_| true);
    assert_eq!(output.activated, [23]);
    for _ in 0..4 {
        let held = frame(&ctx, vec![key(Key::Enter)], 10_000, |_| true);
        assert!(held.activated.is_empty());
    }
    for index in (0..23).rev() {
        let output = frame(&ctx, vec![key(Key::ArrowUp)], 10_000, |_| true);
        assert_active_row(&ctx, &output, index);
    }
}

#[test]
fn list_navigation_clamps_at_logical_ends_and_handles_page_jumps_and_shrinking_data() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    frame(&ctx, vec![], 10_000, |_| true);
    frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    let output = frame(&ctx, vec![key(Key::ArrowUp)], 10_000, |_| true);
    assert_active_row(&ctx, &output, 0);
    let output = frame(&ctx, vec![key(Key::PageDown)], 10_000, |_| true);
    assert_active_row(&ctx, &output, 6);
    let output = frame(&ctx, vec![key(Key::PageUp)], 10_000, |_| true);
    assert_active_row(&ctx, &output, 0);
    for event in [Key::End, Key::ArrowDown] {
        let output = frame(&ctx, vec![key(event)], 10_000, |_| true);
        assert_active_row(&ctx, &output, 9_999);
    }
    let output = frame(&ctx, vec![], 4, |_| true);
    assert_active_row(&ctx, &output, 3);
    let empty = frame(&ctx, vec![], 0, |_| true);
    assert_eq!(ctx.memory(|m| m.focused()), Some(empty.container.id));
    assert_eq!(empty.active, None);
    let disabled = frame(&ctx, vec![key(Key::Enter)], 20, |_| false);
    assert_eq!(ctx.memory(|m| m.focused()), Some(disabled.container.id));
    assert_eq!(disabled.active, None);
    assert!(disabled.activated.is_empty());
}

#[test]
fn disabled_offscreen_rows_are_skipped_without_rendering_them() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let enabled = |row| row != 0 && row != 9_999 && !(3..500).contains(&row);
    frame(&ctx, vec![], 10_000, enabled);
    let output = frame(&ctx, vec![key(Key::Tab)], 10_000, enabled);
    assert_active_row(&ctx, &output, 1);
    for expected in [2, 500] {
        let output = frame(&ctx, vec![key(Key::ArrowDown)], 10_000, enabled);
        assert_active_row(&ctx, &output, expected);
    }
    let output = frame(&ctx, vec![key(Key::ArrowUp)], 10_000, enabled);
    assert_active_row(&ctx, &output, 2);
    let output = frame(&ctx, vec![key(Key::Home)], 10_000, enabled);
    assert_active_row(&ctx, &output, 1);
    let output = frame(&ctx, vec![key(Key::End)], 10_000, enabled);
    assert_active_row(&ctx, &output, 9_998);
    let output = frame(&ctx, vec![key(Key::ArrowDown)], 10_000, enabled);
    assert_active_row(&ctx, &output, 9_998);
}

#[test]
fn tab_can_leave_the_list_and_external_clicks_are_not_overridden() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    frame(&ctx, vec![], 10_000, |_| true);
    frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    frame(&ctx, vec![key(Key::End)], 10_000, |_| true);
    // The list is a single native Tab stop, regardless of its active row.
    let output = frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    assert_eq!(ctx.memory(|m| m.focused()), Some(output.after.id));
    let output = frame(&ctx, vec![key(Key::Home)], 10_000, |_| true);
    assert_eq!(ctx.memory(|m| m.focused()), Some(output.after.id));
    output.container.request_focus();
    let pos = output.after.rect.center();
    for pressed in [true, false] {
        let output = frame(&ctx, pointer(pos, pressed), 10_000, |_| true);
        if !pressed {
            assert!(output.after.clicked());
        }
    }
    let output = frame(&ctx, vec![], 10_000, |_| true);
    assert_ne!(ctx.memory(|m| m.focused()), Some(output.container.id));
    assert!(output.activated.is_empty());
}

#[test]
fn accesskit_tracks_active_and_selected_rows_without_independent_row_focus() {
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};
    let ctx = Context::default();
    ctx.enable_accesskit();
    Theme::default().apply(&ctx);
    frame(&ctx, vec![], 10_000, |_| true);
    let first = frame(&ctx, vec![key(Key::Tab)], 10_000, |_| true);
    let tree = first.accessibility.as_ref().unwrap();
    let node = |id: Id| {
        &tree
            .nodes
            .iter()
            .find(|(node, _)| *node == id.accesskit_id())
            .unwrap()
            .1
    };
    let owner = node(first.container.id);
    assert_eq!(owner.role(), Role::ListBox);
    assert_eq!(owner.size_of_set(), Some(10_000));
    assert_eq!(tree.focus, first.container.id.accesskit_id());
    let active = first.rows.iter().find(|(row, _)| *row == 0).unwrap().1.id;
    let selected = first.rows.iter().find(|(row, _)| *row == 2).unwrap().1.id;
    assert_eq!(owner.active_descendant(), Some(active.accesskit_id()));
    assert_eq!(node(active).role(), Role::ListBoxOption);
    assert_eq!(node(active).is_selected(), Some(false));
    assert_eq!(node(selected).is_selected(), Some(true));
    assert!(!node(active).supports_action(Action::Focus));
    let clicked = frame(
        &ctx,
        vec![Event::AccessKitActionRequest(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: selected.accesskit_id(),
            data: None,
        })],
        10_000,
        |_| true,
    );
    assert_eq!(clicked.activated, [2]);
    assert_active_row(&ctx, &clicked, 2);
    let end = frame(&ctx, vec![key(Key::End)], 10_000, |_| true);
    let tree = end.accessibility.as_ref().unwrap();
    let last = end.rows.iter().find(|(row, _)| *row == 9_999).unwrap().1.id;
    let node = &tree
        .nodes
        .iter()
        .find(|(id, _)| *id == last.accesskit_id())
        .unwrap()
        .1;
    assert_eq!(node.position_in_set(), Some(10_000));
    assert_eq!(
        tree.nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::ListBoxOption)
            .count(),
        end.rows.len()
    );
}
