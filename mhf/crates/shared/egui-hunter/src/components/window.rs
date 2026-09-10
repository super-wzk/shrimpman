use std::sync::Arc;

use crate::{Button, Panel, theme::Tokens};
use egui::{Context, InnerResponse, Style, Ui};

/// Floating egui window with the same panel frame as the rest of the library.
/// Native egui owns movement, edge resizing, bounds and layer ordering.
#[must_use = "Call show to render the window"]
pub struct Window<'a> {
    title: &'a str,
    /// Configure IDs, position, size and native window behavior directly.
    /// The default frame and title bar are replaced by the themed panel.
    /// Use [`Self::open`] to include its themed close button.
    pub native: egui::Window<'a>,
    open: Option<&'a mut bool>,
    style: Option<Arc<Style>>,
    tokens: Option<Tokens>,
}

impl<'a> Window<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            native: egui::Window::new(title)
                .frame(egui::Frame::NONE)
                .title_bar(false)
                .collapsible(false)
                .fade_out(false),
            open: None,
            style: None,
            tokens: None,
        }
    }

    /// Inject a local style across the native Area boundary.
    pub fn style(mut self, style: impl Into<Arc<Style>>) -> Self {
        self.style = Some(style.into());
        self
    }
    /// Inject custom tokens; otherwise use the context's configured tokens.
    pub fn tokens(mut self, tokens: Tokens) -> Self {
        self.tokens = Some(tokens);
        self
    }
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }
    pub fn show<R>(
        self,
        ctx: &Context,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        let closable = self.open.is_some();
        let mut native = self.native;
        if let Some(value) = self.open {
            native = native.open(value);
        }
        let tokens = self.tokens.unwrap_or_else(|| Tokens::from_context(ctx));
        let close_button = Button::new("关闭")
            .kind(crate::ButtonKind::Quiet)
            .min_size(egui::vec2(60.0, 28.0));
        let response = native.show(ctx, |ui| {
            if let Some(style) = self.style {
                ui.set_style(style);
            }
            let result = ui.scope_builder(egui::UiBuilder::new().closable(), |ui| {
                tokens
                    .scope(ui, |ui| {
                        Panel::new(self.title)
                            .show_with_header(
                                ui,
                                |ui| {
                                    if closable && ui.add(close_button).clicked() {
                                        ui.close();
                                    }
                                },
                                |ui| {
                                    // Fill the native resize area so the frame follows both edges.
                                    ui.set_min_height(ui.available_height());
                                    content(ui)
                                },
                            )
                            .inner
                    })
                    .inner
            });
            // Native Window still contains a Collapsible even when its
            // title bar is hidden. Give ui.close() a direct window target.
            if result.response.should_close() {
                ui.close_kind(egui::UiKind::Window);
            }
            result.inner
        })?;
        Some(InnerResponse::new(response.inner?, response.response))
    }
}
