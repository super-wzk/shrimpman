use std::sync::Arc;

use super::interaction::PopupInteraction;
use crate::{Panel, theme::Tokens};
use egui::{Id, InnerResponse, Response, Style, Ui};

#[must_use = "Call show each frame, including while closed, to restore focus"]
pub struct Popup<'a> {
    anchor: &'a Response,
    /// Configure IDs, placement, default width and click-close behavior directly.
    /// Keep egui's Memory open state: `open_bool` and `open` do not support this
    /// component's focus return policy. The preset frame leaves painting to Panel.
    pub native: egui::Popup<'a>,
    title: &'a str,
    interaction: PopupInteraction,
    style: Option<Arc<Style>>,
    tokens: Option<Tokens>,
}

impl<'a> Popup<'a> {
    pub fn new(anchor: &'a Response) -> Self {
        Self {
            anchor,
            native: egui::Popup::from_toggle_button_response(anchor)
                .frame(egui::Frame::NONE)
                .gap(anchor.ctx.global_style().spacing.item_spacing.y)
                .width(240.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
            title: "",
            interaction: PopupInteraction::default(),
            style: None,
            tokens: None,
        }
    }

    /// An anchor Response has no parent Ui style. Pass a local style explicitly
    /// when this popup should use it instead of the native Area's global style.
    pub fn style(mut self, style: impl Into<Arc<Style>>) -> Self {
        self.style = Some(style.into());
        self
    }
    /// Pass the anchor Ui's custom tokens across the detached Area boundary.
    pub fn tokens(mut self, tokens: Tokens) -> Self {
        self.tokens = Some(tokens);
        self
    }
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }
    pub fn initial_focus(mut self, id: Id) -> Self {
        self.interaction = self.interaction.initial_focus(id);
        self
    }
    pub fn show<R>(self, content: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<R>> {
        let ctx = &self.anchor.ctx;
        let tokens = self.tokens.unwrap_or_else(|| Tokens::from_context(ctx));
        self.interaction.show(self.anchor, self.native, |ui| {
            if let Some(style) = self.style {
                ui.set_style(style);
            }
            ui.set_width(
                ui.available_width()
                    .min((ctx.content_rect().width() - 32.0).max(80.0)),
            );
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
