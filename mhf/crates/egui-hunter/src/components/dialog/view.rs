use std::sync::Arc;

use super::interaction::{DialogInteraction, DialogState};
use crate::{Panel, theme::Tokens};
use egui::{Context, Id, InnerResponse, Style, Ui};

#[must_use = "Call show each frame, even when closed, to restore focus"]
pub struct Dialog<'a> {
    id: Id,
    title: &'a str,
    width: f32,
    interaction: DialogInteraction,
    style: Option<Arc<Style>>,
    tokens: Option<Tokens>,
}
impl<'a> Dialog<'a> {
    pub fn new(id: Id, title: &'a str) -> Self {
        Self {
            id,
            title,
            width: 390.0,
            interaction: DialogInteraction::default(),
            style: None,
            tokens: None,
        }
    }
    /// Inject a local style across the native Modal/Area boundary.
    pub fn style(mut self, style: impl Into<Arc<Style>>) -> Self {
        self.style = Some(style.into());
        self
    }
    /// Inject custom tokens; otherwise use the context's configured tokens.
    pub fn tokens(mut self, tokens: Tokens) -> Self {
        self.tokens = Some(tokens);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(120.0);
        self
    }
    pub fn initial_focus(mut self, id: Id) -> Self {
        self.interaction = self.interaction.initial_focus(id);
        self
    }
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.interaction = self.interaction.dismiss_on_backdrop(dismiss);
        self
    }
    pub fn show<R>(
        self,
        ctx: &Context,
        state: &mut DialogState,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        let tokens = self.tokens.unwrap_or_else(|| Tokens::from_context(ctx));
        self.interaction.show(
            ctx,
            state,
            egui::Modal::new(self.id).frame(egui::Frame::NONE),
            |ui| {
                if let Some(style) = self.style {
                    ui.set_style(style);
                }
                ui.set_width(
                    self.width
                        .min((ctx.content_rect().width() - 32.0).max(120.0)),
                );
                tokens
                    .scope(ui, |ui| Panel::new(self.title).show(ui, content).inner)
                    .inner
            },
        )
    }
}
