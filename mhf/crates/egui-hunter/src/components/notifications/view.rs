use super::Notifications;
use crate::notice;
use egui::{Context, InnerResponse};

impl Notifications {
    /// Present the queue after the main UI. egui owns positioning and hit testing.
    pub fn show(&mut self, ctx: &Context) -> Option<InnerResponse<()>> {
        let id = self.id;
        let pass_through = self.pass_through;
        let current = self.advance(ctx)?;
        // Reuse a single Area ID so frequent messages do not accumulate areas.
        let output = egui::Area::new(id)
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0])
            .order(egui::Order::Tooltip)
            .interactable(!pass_through)
            .show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(120.0, 520.0));
                notice(ui, current.kind, &current.text);
            });
        Some(output)
    }
}
