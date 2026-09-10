use crate::input::{Direction, consume_escape};
use egui::{Context, Id, Key, Modifiers, Response, Ui};

pub(crate) mod engagement;
pub use engagement::{EngagementPlugin, FocusEngagement};

pub(crate) fn navigation_allowed(ui: &Ui) -> bool {
    ui.is_enabled()
        && ui.memory(|m| m.allows_interaction(ui.layer_id()))
        && (!egui::Popup::is_any_open(ui.ctx()) || ui.layer_id().order == egui::Order::Foreground)
        && engagement::navigation_allowed(ui)
}

/// Explicit row/column and edge rules for a rendered list or grid. Ordinary
/// controls already have egui's geometric arrow navigation and need no group.
/// Call `navigate` after drawing registered controls, including disabled cells.
/// Single-column groups handle Up/Down only; Left/Right and Tab stay native.
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
    /// Use [`super::layout::VirtualList::show`] for a virtualized list, where rows
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
            .filter(|direction| self.handles(*direction))
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
        if !navigation_allowed(ui) || !self.handles(direction) {
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

    fn handles(self, direction: Direction) -> bool {
        self.columns > 1 || matches!(direction, Direction::Up | Direction::Down)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FocusRestore {
    pub(crate) pending: Option<Id>,
}

impl FocusRestore {
    pub(crate) fn apply(&mut self, ctx: &Context) {
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

/// Make primary pointer activation use egui's existing keyboard focus.
pub fn focus_on_click(response: &Response) {
    if response.sense.is_focusable() && response.clicked_by(egui::PointerButton::Primary) {
        response.request_focus();
    }
}

/// Reveal a newly keyboard-focused control through its native ScrollArea.
/// Call while rendering scroll contents, after the control's rect is known.
pub fn scroll_on_focus(response: &Response) {
    if response.enabled()
        && response.gained_focus()
        && !response
            .ctx
            .input(|input| input.pointer.any_pressed() || input.pointer.any_click())
    {
        response.scroll_to_me(None);
    }
}

/// A native editor's Escape blur consumes this press before its parent acts.
/// This does not change the value or install a second focus owner.
pub fn consume_escape_on_blur(response: &Response) {
    let ctx = &response.ctx;
    if response.enabled()
        && ctx
            .memory(|memory| memory.focused().is_none() && memory.had_focus_last_frame(response.id))
        && !ctx.input(|input| input.pointer.any_pressed())
    {
        consume_escape(ctx);
    }
}
