use egui::{InnerResponse, Ui};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Panel,
    Raised,
}

/// A full-width, content-sized surface using the native frame and stack metadata.
pub struct Panel<'a> {
    title: &'a str,
    surface: Surface,
}

impl<'a> Panel<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            surface: Surface::Panel,
        }
    }

    pub fn surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }
    pub fn show<R>(self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.show_contents(ui, None::<fn(&mut Ui)>, content)
    }

    /// Place actions at the trailing edge of the title row, including untitled panels.
    /// A long title truncates to the space remaining after the actions.
    pub fn show_with_header<R>(
        self,
        ui: &mut Ui,
        header: impl FnOnce(&mut Ui),
        content: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.show_contents(ui, Some(header), content)
    }

    fn show_contents<R>(
        self,
        ui: &mut Ui,
        header: Option<impl FnOnce(&mut Ui)>,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let fill = match self.surface {
            Surface::Panel => ui.visuals().window_fill(),
            Surface::Raised => ui.visuals().faint_bg_color,
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(ui.visuals().window_stroke)
            .corner_radius(ui.visuals().window_corner_radius)
            .inner_margin(ui.spacing().window_margin)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let title =
                    egui::Label::new(egui::RichText::new(self.title).strong()).selectable(false);
                if let Some(header) = header {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            // Reserve action width before laying out the title.
                            header(ui);
                            if !self.title.is_empty() {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(
                                        ui.available_width(),
                                        ui.text_style_height(&egui::TextStyle::Body),
                                    ),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| ui.add(title.truncate()),
                                );
                            }
                        },
                    );
                } else if !self.title.is_empty() {
                    // A text-only heading needs its text height, not an empty
                    // interaction row. Native item spacing supplies the gap.
                    ui.add(title);
                }
                content(ui)
            })
    }
}
