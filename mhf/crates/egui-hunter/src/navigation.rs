use std::hash::Hash;

use egui::{Context, Id, InnerResponse, Key, Modifiers, Response, Ui, UiBuilder};

use crate::{Direction, Theme, paint};

impl Theme {
    /// Call at the end of a native vertical ScrollArea's content closure. Makes
    /// the viewport focusable by Tab or clicking blank space, without covering
    /// child controls. Held arrows scroll continuously; PageUp/Down scroll a page.
    /// Only the viewport's own focus consumes keys, so text editing is unaffected.
    pub fn scroll_focus(&self, ui: &mut Ui, id: Id) -> Response {
        let mut rect = ui.clip_rect();
        rect.max.x = rect.max.x.min(ui.max_rect().right());
        rect.max.y = rect.max.y.min(ui.min_rect().bottom());
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("scroll-focus", id)),
            egui::Sense::focusable_noninteractive(),
        );
        if !navigation_allowed(ui) {
            return response;
        }
        if ui.input(|i| i.pointer.primary_clicked())
            && ui.ctx().interaction_snapshot(|i| i.clicked.is_none())
            && response.contains_pointer()
        {
            response.request_focus();
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
            let (up, down, step_up, step_down, page_up, page_down, dt) = ui.input_mut(|i| {
                (
                    i.key_down(Key::ArrowUp),
                    i.key_down(Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::PageUp),
                    i.consume_key(Modifiers::NONE, Key::PageDown),
                    i.stable_dt.min(0.05),
                )
            });
            if up || down || step_up || step_down {
                ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            }
            let distance = |held, pressed| {
                if held {
                    480.0 * dt
                } else if pressed {
                    36.0
                } else {
                    0.0
                }
            };
            let delta = distance(up, step_up) - distance(down, step_down)
                + (i32::from(page_up) - i32::from(page_down)) as f32 * rect.height() * 0.9;
            if delta != 0.0 {
                ui.scroll_with_delta_animation(
                    egui::vec2(0.0, delta),
                    egui::style::ScrollAnimation::none(),
                );
            }
            if up || down {
                ui.ctx().request_repaint();
            }
            paint::corners(
                ui.painter(),
                rect.shrink(2.0),
                egui::Stroke::new(1.5, self.palette.brass),
            );
        }
        response
    }
}

pub(crate) fn navigation_allowed(ui: &Ui) -> bool {
    ui.is_enabled()
        && ui.memory(|m| m.allows_interaction(ui.layer_id()))
        && (!egui::Popup::is_any_open(ui.ctx()) || ui.layer_id().order == egui::Order::Foreground)
}

/// Directional rules for a rendered list or row-major grid. Call `navigate` after
/// drawing its controls, including disabled cells. Only registered controls take
/// part, so text editors and sliders retain their native arrow-key behavior.
#[derive(Clone, Copy, Debug)]
pub struct FocusGroup {
    columns: usize,
    wrap: bool,
}

impl FocusGroup {
    pub fn vertical() -> Self {
        Self::grid(1)
    }

    pub fn grid(columns: usize) -> Self {
        Self {
            columns: columns.max(1),
            wrap: false,
        }
    }

    /// Wrap within the current row/column; by default edges keep the focus.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Returns the new focused ID only when it moves. Tab still leaves the group.
    /// Use [`crate::ScrollPanel::show_list`] for a virtualized list, where rows
    /// outside the rendered range have no response to register here.
    pub fn navigate(self, ui: &Ui, controls: &[Response]) -> Option<Id> {
        if !navigation_allowed(ui) {
            return None;
        }
        let current = controls.iter().find(|response| response.has_focus())?;
        let direction = ui.input_mut(|input| {
            [
                Direction::Up,
                Direction::Right,
                Direction::Down,
                Direction::Left,
            ]
            .into_iter()
            .find(|direction| input.consume_key(Modifiers::NONE, direction.key()))
        })?;
        self.move_focus(ui, controls, current.id, direction)
    }

    /// Move from an explicit item, e.g. for mouse navigation buttons. This does
    /// not synthesize input events or change the detected input device.
    pub fn move_focus(
        self,
        ui: &Ui,
        controls: &[Response],
        from: Id,
        direction: Direction,
    ) -> Option<Id> {
        if !navigation_allowed(ui) {
            return None;
        }
        let current = controls
            .iter()
            .position(|response| response.id == from && response.enabled())?;
        let ctx = ui.ctx();
        // egui already saw keyboard arrows at begin_pass. Suppress its geometric
        // move, including at a boundary, or focus would move twice at end_pass.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        controls[current].request_focus();
        let horizontal = matches!(direction, Direction::Left | Direction::Right);
        let forward = matches!(direction, Direction::Down | Direction::Right);
        let (start, stride, count, position) = if horizontal {
            let start = current / self.columns * self.columns;
            (
                start,
                1,
                self.columns.min(controls.len() - start),
                current - start,
            )
        } else {
            let start = current % self.columns;
            (
                start,
                self.columns,
                (controls.len() - start).div_ceil(self.columns),
                current / self.columns,
            )
        };
        for distance in 1..count {
            let next = if forward {
                position + distance
            } else {
                position + count - distance
            };
            if !self.wrap && ((forward && next >= count) || (!forward && distance > position)) {
                break;
            }
            let response = &controls[start + (next % count) * stride];
            if response.enabled() && response.sense.is_focusable() {
                response.request_focus();
                response.scroll_to_me(Some(egui::Align::Center));
                ctx.request_repaint();
                return Some(response.id);
            }
        }
        None
    }
}

/// Open state shared by dialogs and anchored popups. Keep one per overlay.
/// Call the container's `show` each frame, including while closed, so deferred
/// focus restoration runs after the opener is rendered again.
#[derive(Debug, Default)]
pub struct OverlayState {
    pub(crate) open: bool,
    pub(crate) just_opened: bool,
    pub(crate) popup_id: Option<Id>,
    return_focus: Option<Id>,
    restore: FocusRestore,
}

impl OverlayState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Programmatic opening captures the currently focused control.
    pub fn open(&mut self, ctx: &Context) {
        if !self.open {
            self.open = true;
            self.just_opened = true;
            self.return_focus = ctx.memory(|m| m.focused());
            self.restore = FocusRestore::default();
            ctx.request_repaint();
        }
    }

    /// Mouse and keyboard opening both return to this specific control.
    pub fn open_from(&mut self, opener: &Response) {
        if !self.open {
            self.open(&opener.ctx);
            self.return_focus = Some(opener.id);
        }
    }

    pub fn toggle_from(&mut self, opener: &Response) {
        if self.open {
            self.close(&opener.ctx);
        } else {
            self.open_from(opener);
        }
    }

    pub fn close(&mut self, ctx: &Context) {
        self.close_with_focus(ctx, true);
    }

    pub(crate) fn close_with_focus(&mut self, ctx: &Context, restore_focus: bool) {
        if !self.open {
            return;
        }
        self.open = false;
        self.just_opened = false;
        if let Some(id) = self.popup_id.take() {
            egui::Popup::close_id(ctx, id);
        }
        self.restore.pending = self.return_focus.take().filter(|_| restore_focus);
        ctx.request_repaint();
    }

    pub(crate) fn prepare(&mut self, ctx: &Context) {
        if !self.open {
            self.restore.apply(ctx);
        }
    }
}

#[derive(Debug, Default)]
struct FocusRestore {
    pending: Option<Id>,
}

impl FocusRestore {
    fn apply(&mut self, ctx: &Context) {
        let Some(id) = self.pending.take() else {
            return;
        };
        // Do not override an explicit new click/Tab or a newly opened popup.
        let navigating = ctx.input(|i| i.pointer.any_pressed() || i.key_pressed(Key::Tab));
        if !navigating
            && !egui::Popup::is_any_open(ctx)
            && ctx.read_response(id).is_some_and(|r| r.enabled())
        {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
    }
}

#[derive(Debug)]
struct MenuEntry<Page> {
    page: Page,
    return_focus: Option<Id>,
}

/// A root-preserving menu stack. Page keys also scope egui widget/scroll state.
/// Render through `show` each frame. Navigation requested after `show` takes
/// effect on the next frame; restored controls then exist again.
#[derive(Debug)]
pub struct MenuStack<Page> {
    id: Id,
    entries: Vec<MenuEntry<Page>>,
    restore: FocusRestore,
}

impl<Page: Eq + Hash + std::fmt::Debug> MenuStack<Page> {
    pub fn new(id: Id, root: Page) -> Self {
        Self {
            id,
            entries: vec![MenuEntry {
                page: root,
                return_focus: None,
            }],
            restore: FocusRestore::default(),
        }
    }

    pub fn current(&self) -> &Page {
        &self.entries.last().expect("menu always has a root").page
    }
    pub fn depth(&self) -> usize {
        self.entries.len()
    }
    pub fn can_go_back(&self) -> bool {
        self.entries.len() > 1
    }

    pub fn push(&mut self, ctx: &Context, page: Page) {
        self.push_with_focus(ctx, page, ctx.memory(|m| m.focused()));
    }

    pub fn push_from(&mut self, opener: &Response, page: Page) {
        self.push_with_focus(&opener.ctx, page, Some(opener.id));
    }

    fn push_with_focus(&mut self, ctx: &Context, page: Page, return_focus: Option<Id>) {
        if self.current() != &page {
            self.entries.push(MenuEntry { page, return_focus });
            self.restore = FocusRestore::default();
            ctx.request_repaint();
        }
    }

    /// Pop one page. The root is never removed. Also usable for a controller's B action.
    pub fn back(&mut self, ctx: &Context) -> bool {
        if !self.can_go_back() {
            return false;
        }
        self.restore.pending = self.entries.pop().and_then(|entry| entry.return_focus);
        ctx.request_repaint();
        true
    }

    /// Escape goes to native popups/modals first, then removes one menu page.
    /// Explicit `UiBuilder::id` keeps page controls stable across container moves.
    pub fn show<R>(
        &mut self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui, &Page) -> R,
    ) -> InnerResponse<R> {
        if ui.is_enabled()
            && self.can_go_back()
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none())
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.back(ui.ctx());
        }
        let result = ui.scope_builder(UiBuilder::new().id(self.id.with(self.current())), |ui| {
            content(ui, self.current())
        });
        self.restore.apply(ui.ctx());
        result
    }
}
