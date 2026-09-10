use super::Notifications;
use crate::notice;
use egui::{Align2, Context, InnerResponse, Vec2, vec2};

impl Notifications {
    /// Present the queue after the main UI. egui owns positioning and hit testing.
    pub fn show(&mut self, ctx: &Context) -> Option<InnerResponse<()>> {
        self.show_at(ctx, Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
    }

    /// Present a floating notification at a host-selected viewport anchor.
    pub fn show_at(
        &mut self,
        ctx: &Context,
        anchor: Align2,
        offset: Vec2,
    ) -> Option<InnerResponse<()>> {
        let id = self.id;
        let pass_through = self.pass_through;
        let current = self.advance(ctx)?;
        // Reuse a single Area ID so frequent messages do not accumulate areas.
        let output = egui::Area::new(id)
            .anchor(anchor, offset)
            .order(egui::Order::Tooltip)
            .interactable(!pass_through)
            .show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(120.0, 520.0));
                notice(ui, current.kind, &current.text);
            });
        Some(output)
    }
}
