use egui::{Color32, InnerResponse, Shape, Stroke, Ui};

use crate::{Theme, paint};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Leather,
    Parchment,
}

/// A full-width, content-sized panel with a stitched, chamfered frame.
pub struct Panel<'a> {
    theme: &'a Theme,
    title: &'a str,
    surface: Surface,
}

impl Theme {
    pub fn panel<'a>(&'a self, title: &'a str) -> Panel<'a> {
        Panel {
            theme: self,
            title,
            surface: Surface::Leather,
        }
    }
}

impl Panel<'_> {
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
        let p = self.theme.palette;
        let light = self.surface == Surface::Parchment;
        let background = ui.painter().add(Shape::Noop);
        let response = egui::Frame::new()
            .inner_margin(self.theme.metrics.padding)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if light {
                    let visuals = ui.visuals_mut();
                    visuals.override_text_color = Some(p.ink);
                    visuals.weak_text_color = Some(p.ink.gamma_multiply(0.75));
                    for widget in [
                        &mut visuals.widgets.noninteractive,
                        &mut visuals.widgets.inactive,
                        &mut visuals.widgets.hovered,
                        &mut visuals.widgets.active,
                        &mut visuals.widgets.open,
                    ] {
                        widget.fg_stroke.color = p.ink;
                        widget.bg_fill = p.parchment;
                        widget.weak_bg_fill = p.parchment;
                    }
                }
                if !self.title.is_empty() {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 24.0), egui::Sense::hover());
                        paint::diamond(
                            ui.painter(),
                            rect.center(),
                            4.0,
                            if light { p.ink } else { p.brass },
                        );
                        ui.label(
                            egui::RichText::new(self.title)
                                .size(self.theme.metrics.heading_size)
                                .color(if light { p.ink } else { p.text }),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), header);
                    });
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(4.0);
                }
                content(ui)
            });
        let rect = response.response.rect.shrink(0.5);
        if ui.is_rect_visible(rect) {
            ui.painter().set(
                background,
                Shape::Vec(vec![
                    paint::chamfer(
                        rect,
                        9.0,
                        if light { p.parchment } else { p.panel },
                        Stroke::new(1.0, p.border),
                    ),
                    paint::chamfer(
                        rect.shrink(4.0),
                        6.0,
                        Color32::TRANSPARENT,
                        Stroke::new(0.5, p.border.gamma_multiply(0.65)),
                    ),
                    paint::grain(rect.shrink(8.0), light),
                ]),
            );
            paint::corners(ui.painter(), rect.shrink(3.0), Stroke::new(1.5, p.brass));
        }
        response
    }
}
