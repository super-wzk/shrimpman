use std::sync::Arc;

use super::interaction::TooltipInteraction;
use crate::{Panel, theme::Tokens};
use egui::{InnerResponse, Response, Style, Ui};

/// A native delayed tooltip with a styled panel and freely composed contents.
/// Keyboard focus also reveals it by default. No popup/menu state is opened.
pub struct RichTooltip<'a> {
    anchor: &'a Response,
    title: &'a str,
    width: Option<f32>,
    interaction: TooltipInteraction,
    style: Option<Arc<Style>>,
    tokens: Option<Tokens>,
}

impl<'a> RichTooltip<'a> {
    pub fn new(anchor: &'a Response, title: &'a str) -> Self {
        Self {
            anchor,
            title,
            width: None,
            interaction: TooltipInteraction::default(),
            style: None,
            tokens: None,
        }
    }
    /// An anchor Response has no parent Ui style. Pass a local style explicitly
    /// when this tooltip should use it instead of the native Area's global style.
    pub fn style(mut self, style: impl Into<Arc<Style>>) -> Self {
        self.style = Some(style.into());
        self
    }
    /// Pass the anchor Ui's custom tokens across the detached Area boundary.
    pub fn tokens(mut self, tokens: Tokens) -> Self {
        self.tokens = Some(tokens);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width.max(80.0));
        self
    }
    pub fn on_focus(mut self, on_focus: bool) -> Self {
        self.interaction = self.interaction.on_focus(on_focus);
        self
    }

    pub fn show<R>(self, content: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<R>> {
        let ctx = &self.anchor.ctx;
        let mut native = self.interaction.native(self.anchor)?;
        let style = self.style.unwrap_or_else(|| ctx.global_style());
        let tokens = self.tokens.unwrap_or_else(|| Tokens::from_context(ctx));
        let width = self
            .width
            .unwrap_or(style.spacing.tooltip_width)
            .min((ctx.content_rect().width() - 16.0).max(1.0));
        native.popup = native.popup.frame(egui::Frame::NONE);
        native.width(width).show(|ui| {
            ui.set_style(style);
            ui.set_width(width);
            tokens
                .scope(ui, |ui| {
                    Panel::new(self.title)
                        .surface(crate::Surface::Raised)
                        .show(ui, content)
                        .inner
                })
                .inner
        })
    }
}
