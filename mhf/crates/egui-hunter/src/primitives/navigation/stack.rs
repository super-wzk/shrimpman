use crate::input::{consume_escape, discard_escape_repeats};
use crate::primitives::focus::{FocusRestore, navigation_allowed};
use egui::{Context, Id, InnerResponse, Response, Ui, UiBuilder};
use std::hash::Hash;

#[derive(Debug)]
struct NavigationEntry<Page> {
    page: Page,
    return_focus: Option<Id>,
}

/// A root-preserving navigation stack. Page keys also scope egui widget/scroll state.
/// Render through `show` each frame. Navigation requested after `show` takes
/// effect on the next frame; restored controls then exist again.
#[derive(Debug)]
pub struct NavigationStack<Page> {
    id: Id,
    entries: Vec<NavigationEntry<Page>>,
    restore: FocusRestore,
}

impl<Page: Eq + Hash + std::fmt::Debug> NavigationStack<Page> {
    pub fn new(id: Id, root: Page) -> Self {
        Self {
            id,
            entries: vec![NavigationEntry {
                page: root,
                return_focus: None,
            }],
            restore: FocusRestore::default(),
        }
    }

    pub fn current(&self) -> &Page {
        &self
            .entries
            .last()
            .expect("navigation always has a root")
            .page
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
            self.entries.push(NavigationEntry { page, return_focus });
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

    /// Children handle Escape first; the remaining press removes one page.
    /// The parent page and its restored focus appear on the next pass.
    /// Explicit `UiBuilder::id` keeps page controls stable across container moves.
    pub fn show<R>(
        &mut self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui, &Page) -> R,
    ) -> InnerResponse<R> {
        let popup_was_open = egui::Popup::is_any_open(ui.ctx());
        if navigation_allowed(ui) {
            discard_escape_repeats(ui.ctx());
        }
        let result = ui.scope_builder(UiBuilder::new().id(self.id.with(self.current())), |ui| {
            content(ui, self.current())
        });
        if navigation_allowed(ui) {
            self.restore.apply(ui.ctx());
        }
        if popup_was_open && !egui::Popup::is_any_open(ui.ctx()) {
            // Native popups observe Escape without consuming it.
            consume_escape(ui.ctx());
        } else if navigation_allowed(ui)
            && self.can_go_back()
            && !egui::Popup::is_any_open(ui.ctx())
            && consume_escape(ui.ctx())
        {
            self.back(ui.ctx());
        }
        result
    }
}
