use egui::scroll_area::ScrollAreaOutput;
use egui::{Id, Key, Modifiers, Response, ScrollArea, Sense, Ui, UiBuilder};

use crate::input::consume_press;
use crate::primitives::focus::{focus_on_click, navigation_allowed, scroll_on_focus};

/// A virtualized selection widget with one native keyboard focus target.
/// Rows sense pointer clicks; active-row navigation is local widget state.
#[must_use = "Call show to render the list"]
pub struct VirtualList {
    id: Id,
}

/// Native interaction/scroll output plus the selection widget's local state.
/// `activated` reports both pointer activation and keyboard confirmation.
pub struct ListOutput {
    /// The list's single native focus target; rows have pointer responses only.
    pub response: Response,
    pub scroll: ScrollAreaOutput<()>,
    /// Remembered row, independent of both keyboard focus and the selected value.
    pub active: Option<usize>,
    pub activated: Option<usize>,
    /// The active row's pointer response, when that row is rendered.
    pub active_response: Option<Response>,
}

impl VirtualList {
    pub fn new(id: Id) -> Self {
        Self { id }
    }

    /// Render equal-height rows that use `Sense::CLICK`, not `Sense::click()`.
    /// Rows must not register independent keyboard controls. Use a plain native
    /// ScrollArea for forms containing buttons, editors or other Tab stops.
    /// The row callback receives whether this is the current keyboard-active
    /// part, so its view can render that state without a separate focus owner.
    /// Handle activation through the returned `activated` row index.
    pub fn show(
        self,
        ui: &mut Ui,
        mut scroll: ScrollArea,
        row_height: f32,
        count: usize,
        enabled: impl Fn(usize) -> bool,
        mut row_content: impl FnMut(&mut Ui, usize, bool) -> Response,
    ) -> ListOutput {
        let id = ui.make_persistent_id(("virtual-list", self.id));
        let output = ui.scope_builder(UiBuilder::new().id(id).sense(Sense::click()), |ui| {
            let state_id = id.with("navigation");
            let mut state =
                ui.data_mut(|data| data.get_temp::<ListState>(state_id).unwrap_or_default());
            let (mut activated, reveal) = state.input(ui, id, row_height, count, &enabled);
            if reveal {
                scroll = scroll.vertical_scroll_offset(state.visible_offset(ui, row_height));
            }
            let mut active_response = None;
            let mut clicked = None;
            let scroll = scroll.show_rows(ui, row_height, count, |ui, rows| {
                for row in rows {
                    let current =
                        state.active == Some(row) && ui.memory(|memory| memory.has_focus(id));
                    let response = ui
                        .add_enabled_ui(enabled(row), |ui| row_content(ui, row, current))
                        .inner;
                    debug_assert!(
                        !response.sense.is_focusable(),
                        "Selection-list rows must use Sense::CLICK; the list owns keyboard focus"
                    );
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.set_role(egui::accesskit::Role::ListBoxOption);
                        node.set_position_in_set(row + 1);
                        if let Some(toggled) = node.toggled() {
                            node.set_selected(toggled == egui::accesskit::Toggled::True);
                            node.clear_toggled();
                        }
                    });
                    if response.enabled() && response.clicked() {
                        state.active = Some(row);
                        activated = Some(row);
                        response.scroll_to_me(None);
                        clicked = Some(response.clone());
                    }
                    if state.active == Some(row) {
                        active_response = Some(response);
                    }
                }
                ui.set_min_height(ui.spacing().interact_size.y);
            });
            state.offset = scroll.state.offset.y;
            state.viewport_height = scroll.inner_rect.height();
            let active = state.active;
            ui.data_mut(|data| data.insert_temp(state_id, state));
            (scroll, active, activated, active_response, clicked)
        });
        let (scroll, active, activated, active_response, clicked) = output.inner;
        let mut response = output.response;
        if let Some(clicked) = clicked {
            response = response.union(clicked);
            ui.ctx().request_repaint();
        }
        focus_on_click(&response);
        let rect = active_response
            .as_ref()
            .map_or(response.rect, |row| row.rect);
        scroll_on_focus(&response.clone().with_new_rect(rect));
        let focused = response.has_focus();
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::ListBox);
            node.set_size_of_set(count);
            if focused && let Some(row) = &active_response {
                node.set_active_descendant(row.id.accesskit_id());
            } else {
                node.clear_active_descendant();
            }
        });
        ListOutput {
            response,
            scroll,
            active,
            activated,
            active_response,
        }
    }
}

#[derive(Clone, Default)]
struct ListState {
    active: Option<usize>,
    /// Previous rendered focus, used only to reveal the remembered row on entry.
    focused: bool,
    offset: f32,
    viewport_height: f32,
}

impl ListState {
    fn input(
        &mut self,
        ui: &Ui,
        owner: Id,
        row_height: f32,
        count: usize,
        enabled: &impl Fn(usize) -> bool,
    ) -> (Option<usize>, bool) {
        let previous = self.active;
        let was_focused = self.focused;
        self.focused = navigation_allowed(ui) && ui.memory(|memory| memory.has_focus(owner));
        self.active = if count == 0 {
            None
        } else {
            let start = self.active.unwrap_or(0).min(count - 1);
            (start..count)
                .find(|&row| enabled(row))
                .or_else(|| (0..start).rev().find(|&row| enabled(row)))
        };
        if !self.focused {
            return (None, false);
        }
        let mut activated = None;
        let mut reveal = !was_focused || self.active != previous;
        if !ui.input(|i| i.pointer.any_pressed() || i.pointer.any_click()) {
            if consume_press(ui.ctx(), &[Key::Enter, Key::Space]) {
                activated = self.active;
            }
            let key = ui.input_mut(|input| {
                [
                    Key::ArrowUp,
                    Key::ArrowDown,
                    Key::PageUp,
                    Key::PageDown,
                    Key::Home,
                    Key::End,
                ]
                .into_iter()
                .find(|key| input.consume_key(Modifiers::NONE, *key))
            });
            if let (Some(key), Some(current)) = (key, self.active) {
                // As with a native editor/slider, these keys belong to this
                // widget, including the first pass after it receives focus.
                ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
                let page = (self.viewport_height / (row_height + ui.spacing().item_spacing.y))
                    .floor()
                    .max(1.0) as usize;
                let (start, forward) = match key {
                    Key::ArrowUp => (current.saturating_sub(1), false),
                    Key::ArrowDown => (current.saturating_add(1).min(count - 1), true),
                    Key::PageUp => (current.saturating_sub(page), false),
                    Key::PageDown => (current.saturating_add(page).min(count - 1), true),
                    Key::Home => (0, true),
                    _ => (count - 1, false),
                };
                self.active = if forward {
                    (start..count).find(|&row| enabled(row))
                } else {
                    (0..=start).rev().find(|&row| enabled(row))
                }
                .or(Some(current));
                reveal = true;
            }
        }
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                owner,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        (activated, reveal)
    }

    fn visible_offset(&self, ui: &Ui, row_height: f32) -> f32 {
        let Some(row) = self.active else {
            return self.offset;
        };
        let top = row as f32 * (row_height + ui.spacing().item_spacing.y);
        let bottom = top + row_height;
        if top < self.offset {
            top
        } else if bottom > self.offset + self.viewport_height.max(row_height) {
            (bottom - self.viewport_height.max(row_height)).max(0.0)
        } else {
            self.offset
        }
    }
}
