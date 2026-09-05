use egui::{Id, Key, Modifiers, Response, Ui};

use crate::navigation::navigation_allowed;

/// Cached per list in egui. Only the logical focus and previous viewport are
/// needed; unrendered rows never need widgets or cached responses.
#[derive(Clone, Default)]
pub(crate) struct ListNavigation {
    focused: Option<(usize, Id)>,
    pending: Option<usize>,
    pub offset: f32,
    pub viewport_height: f32,
}

impl ListNavigation {
    pub fn prepare(
        &mut self,
        ui: &Ui,
        row_height: f32,
        count: usize,
        enabled: &impl Fn(usize) -> bool,
    ) -> Option<f32> {
        if !navigation_allowed(ui)
            || ui.input(|i| {
                i.pointer.any_pressed() || i.pointer.any_click() || i.key_pressed(Key::Tab)
            })
        {
            self.pending = None;
            return None;
        }
        let (previous, id) = self.focused?;
        let focus = ui.memory(|m| m.focused());
        if focus != Some(id) && !(focus.is_none() && self.pending.is_some()) {
            self.pending = None;
            return None;
        }
        if count == 0 {
            ui.memory_mut(|m| m.surrender_focus(id));
            *self = Self::default();
            return None;
        }
        let current = self.pending.unwrap_or(previous).min(count - 1);
        let pitch = row_height + ui.spacing().item_spacing.y;
        let page = (self.viewport_height / pitch).floor().max(1.0) as usize;
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
        if key.is_some() || previous >= count || !enabled(current) {
            // Stop egui's geometric navigation from jumping to a different panel.
            ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            let (start, forward) = match key {
                Some(Key::ArrowUp) => (current.saturating_sub(1), false),
                Some(Key::ArrowDown) => (current.saturating_add(1).min(count - 1), true),
                Some(Key::PageUp) => (current.saturating_sub(page), false),
                Some(Key::PageDown) => (current.saturating_add(page).min(count - 1), true),
                Some(Key::Home) => (0, true),
                _ => (count - 1, false),
            };
            self.pending = if forward {
                (start..count).find(|&row| enabled(row))
            } else {
                (0..=start).rev().find(|&row| enabled(row))
            }
            .or_else(|| enabled(current).then_some(current));
            if self.pending.is_none() {
                ui.memory_mut(|m| m.surrender_focus(id));
                self.focused = None;
            }
        }
        self.pending.map(|row| {
            let top = row as f32 * pitch;
            let bottom = top + row_height;
            if top < self.offset {
                top
            } else if bottom > self.offset + self.viewport_height {
                (bottom - self.viewport_height).max(0.0)
            } else {
                self.offset
            }
        })
    }

    pub fn observe(&mut self, ui: &Ui, row: usize, response: &Response) {
        if self.pending == Some(row)
            && response.enabled()
            && response.sense.is_focusable()
            && navigation_allowed(ui)
        {
            response.request_focus();
            response.scroll_to_me(None);
            ui.ctx().request_repaint();
            self.pending = None;
        }
        if response.has_focus() {
            self.focused = Some((row, response.id));
        }
    }
}
