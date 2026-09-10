//! Content navigation independent of its controls and visual presentation.
mod stack;
pub use stack::NavigationStack;

use egui::{Id, Ui, UiBuilder};

/// Caller-owned selection of a content destination, separate from widget focus.
#[derive(Debug, Default)]
pub struct NavigationState {
    selected: Option<Id>,
}

impl NavigationState {
    pub fn selected(&self) -> Option<Id> {
        self.selected
    }
    pub fn select(&mut self, id: Id) {
        self.selected = Some(id);
    }

    /// Keep the current destination when available, otherwise use the first one.
    pub fn reconcile(&mut self, available: impl IntoIterator<Item = Id>) {
        let mut available = available.into_iter();
        let first = available.next();
        if self.selected != first && !available.any(|id| Some(id) == self.selected) {
            self.selected = first;
        }
    }

    /// Destination IDs preserve native widget state across presentation changes.
    pub fn show<R>(
        &self,
        ui: &mut Ui,
        id: Id,
        content: impl FnOnce(&mut Ui, Id) -> R,
    ) -> Option<R> {
        self.selected.map(|selected| {
            ui.scope_builder(UiBuilder::new().id(id.with(("content", selected))), |ui| {
                content(ui, selected)
            })
            .inner
        })
    }
}
