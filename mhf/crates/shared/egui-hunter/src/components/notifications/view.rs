use super::Notifications;
use crate::{Tokens, notice};
use egui::{Align2, Context, InnerResponse, Style, Ui, Vec2, vec2};
use std::sync::Arc;

impl Notifications {
    /// Present the queue after the main UI. egui owns positioning and hit testing.
    pub fn show(&mut self, ctx: &Context) -> Option<InnerResponse<()>> {
        self.show_at(ctx, Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
    }

    /// Present using this Ui's local style and density across the Area boundary.
    pub fn show_in(&mut self, ui: &Ui) -> Option<InnerResponse<()>> {
        self.show_at_in(ui, Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
    }

    pub fn show_at_in(
        &mut self,
        ui: &Ui,
        anchor: Align2,
        offset: Vec2,
    ) -> Option<InnerResponse<()>> {
        self.show_styled(
            ui.ctx(),
            anchor,
            offset,
            Some(ui.style().clone()),
            Tokens::get(ui),
        )
    }

    /// Present a floating notification at a host-selected viewport anchor.
    pub fn show_at(
        &mut self,
        ctx: &Context,
        anchor: Align2,
        offset: Vec2,
    ) -> Option<InnerResponse<()>> {
        self.show_styled(ctx, anchor, offset, None, Tokens::from_context(ctx))
    }

    fn show_styled(
        &mut self,
        ctx: &Context,
        anchor: Align2,
        offset: Vec2,
        style: Option<Arc<Style>>,
        tokens: Tokens,
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
                if let Some(style) = style {
                    ui.set_style(style);
                }
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(120.0, 520.0));
                tokens.scope(ui, |ui| notice(ui, current.kind, &current.text));
            });
        Some(output)
    }
}
