use crate::theme::{Tokens, paint};
use egui::{InnerResponse, Shape, Ui};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Leather,
    Parchment,
}

/// A full-width, content-sized surface for layout and decorative grouping.
pub struct Panel<'a> {
    title: &'a str,
    surface: Surface,
}

impl<'a> Panel<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            surface: Surface::Leather,
        }
    }

    pub fn surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }
    pub fn show<R>(self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.show_with_header(ui, |_| {}, content)
    }

    /// Place actions at the trailing edge of the title row.
    pub fn show_with_header<R>(
        self,
        ui: &mut Ui,
        header: impl FnOnce(&mut Ui),
        content: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let tokens = Tokens::get(ui);
        let light = self.surface == Surface::Parchment;
        let fill = if light {
            tokens.parchment
        } else {
            ui.visuals().window_fill()
        };
        let mut stroke = ui.visuals().window_stroke;
        if light {
            stroke.color = tokens.ink.gamma_multiply(0.6);
        }
        let padding = ui.spacing().window_margin;
        let background = ui.painter().add(Shape::Noop);
        // Native descendants such as ScrollArea use this metadata for their
        // background-dependent painting. The chamfer below still paints it.
        let info = egui::UiStackInfo::default().with_frame(egui::Frame::new().fill(fill));
        let builder = egui::UiBuilder::new().ui_stack_info(info);
        let InnerResponse {
            inner: frame,
            response: container,
        } = ui.scope_builder(builder, |ui| {
            egui::Frame::new().inner_margin(padding).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let local_tokens = if light {
                    crate::theme::parchment(ui, tokens)
                } else {
                    tokens
                };
                local_tokens
                    .scope(ui, |ui| {
                        if !self.title.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(self.title).heading())
                                        .selectable(false),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    header,
                                );
                            });
                            let gap = ui.spacing().item_spacing.y;
                            ui.add_space(gap * 0.25);
                            ui.separator();
                            ui.add_space(gap * 0.5);
                        }
                        content(ui)
                    })
                    .inner
            })
        });
        let response = InnerResponse::new(frame.inner, container.union(frame.response));
        let rect = response.response.rect.shrink(0.5);
        if ui.is_rect_visible(rect) {
            ui.painter().set(
                background,
                Shape::Vec(vec![
                    paint::chamfer(rect, tokens.cut, fill, stroke),
                    paint::grain(rect.shrink(8.0), light),
                ]),
            );
        }
        response
    }
}
