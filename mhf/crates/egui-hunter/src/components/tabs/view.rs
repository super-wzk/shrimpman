use super::interaction::{Tab, TabsInteraction};
use crate::{Button, NavigationState};
use egui::{Id, InnerResponse, Ui};

#[must_use = "Call show to render tabs and their selected content"]
pub struct Tabs {
    interaction: TabsInteraction,
}

impl Tabs {
    pub fn new(id: Id) -> Self {
        Self {
            interaction: TabsInteraction::new(id),
        }
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        state: &mut NavigationState,
        tabs: &[Tab<'_>],
        content: impl FnOnce(&mut Ui, Id) -> R,
    ) -> InnerResponse<Option<R>> {
        self.interaction.gap(ui.spacing().item_spacing.x).show(
            ui,
            state,
            tabs,
            |ui, tab, id, selected| ui.add(Button::new(tab.label).id(id).selected(selected)),
            content,
        )
    }
}
