use egui::{Id, Rect, Response, Sense, Ui, Vec2, Widget, WidgetInfo, WidgetType};

use super::{ButtonKind, tooltip::interaction::TooltipInteraction};
use crate::{Icon, primitives::focus, theme::paint};

/// A square, centered icon action. The label names the control for accessibility
/// and hover/keyboard-focus tooltips; it does not reserve visible text space.
pub struct IconButton<'a> {
    id: Option<Id>,
    icon: Icon,
    label: &'a str,
    kind: ButtonKind,
    selected: Option<bool>,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            id: None,
            icon,
            label,
            kind: ButtonKind::Default,
            selected: None,
        }
    }

    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    /// Selection changes the background while keeping the icon centered.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let icon_size = ui.spacing().icon_width;
        let side = ui
            .spacing()
            .interact_size
            .y
            .max(icon_size + ui.spacing().button_padding.y * 2.0);
        let size = Vec2::splat(side);
        let (auto_id, allocated) = ui.allocate_space(size);
        let rect = ui.layout().align_size_within_rect(size, allocated);
        let mut response = ui.interact(rect, self.id.unwrap_or(auto_id), Sense::click());
        response.set_intrinsic_size(size);
        focus::focus_on_click(&response);
        focus::scroll_on_focus(&response);
        response.widget_info(|| match self.selected {
            Some(selected) => WidgetInfo::selected(
                WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                self.label,
            ),
            None => WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), self.label),
        });
        if ui.is_rect_visible(rect) {
            let color = paint::button(ui, &response, self.selected == Some(true), self.kind);
            self.icon.paint(
                &ui.painter_at(rect),
                Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
                color,
            );
        }
        if let Some(tooltip) = TooltipInteraction::default().native(&response) {
            tooltip.show(|ui| {
                ui.add(egui::Label::new(self.label).selectable(false));
            });
        }
        response
    }
}
