use crate::Icon;
use crate::primitives::focus::{focus_on_click, scroll_on_focus};
use crate::theme::paint;
use egui::{
    Align2, Color32, FontSelection, Rect, Response, Sense, TextStyle, TextureId, Ui, Vec2, Widget,
    WidgetInfo, WidgetType, pos2, vec2,
};

pub struct ItemSlot<'a> {
    label: &'a str,
    icon: Option<Icon>,
    image: Option<TextureId>,
    quantity: Option<u32>,
    selected: bool,
    size: Option<f32>,
    tint: Option<Color32>,
    hover_text: bool,
}

impl<'a> ItemSlot<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            image: None,
            quantity: None,
            selected: false,
            size: None,
            tint: None,
            hover_text: true,
        }
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn image(mut self, texture: TextureId) -> Self {
        self.image = Some(texture);
        self
    }
    pub fn quantity(mut self, quantity: u32) -> Self {
        self.quantity = Some(quantity);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size.max(32.0));
        self
    }
    pub fn tint(mut self, color: Color32) -> Self {
        self.tint = Some(color);
        self
    }
    /// Disable the plain label when attaching a rich tooltip to the response.
    pub fn hover_text(mut self, show: bool) -> Self {
        self.hover_text = show;
        self
    }
}

impl Widget for ItemSlot<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = self.size.unwrap_or(ui.spacing().interact_size.y * 2.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        focus_on_click(&response);
        scroll_on_focus(&response);
        response.widget_info(|| {
            WidgetInfo::selected(
                WidgetType::SelectableLabel,
                ui.is_enabled(),
                self.selected,
                self.label,
            )
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let mut visuals = paint::visuals(ui, &response, self.selected);
            if paint::focused(ui, &response) {
                visuals.bg_stroke = paint::focus_stroke(ui, false);
            }
            paint::control(ui, rect, &visuals);
            let color = ui
                .visuals()
                .override_text_color
                .unwrap_or_else(|| visuals.text_color());
            let art = rect.shrink(size * 0.22);
            if let Some(texture) = self.image {
                painter.image(
                    texture,
                    art,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else if let Some(icon) = self.icon {
                icon.paint(&painter, art, self.tint.unwrap_or(color));
            } else {
                painter.circle_filled(art.center(), 3.0, ui.visuals().weak_text_color());
            }
            if self.selected {
                let side = ui.spacing().icon_width_inner;
                paint::selection_mark(
                    ui,
                    Rect::from_center_size(rect.right_top() + vec2(-side, side), Vec2::splat(side)),
                );
            }
            if let Some(quantity) = self.quantity {
                painter.text(
                    rect.right_bottom() - ui.spacing().button_padding * 0.5,
                    Align2::RIGHT_BOTTOM,
                    quantity.to_string(),
                    FontSelection::Default
                        .resolve_with_fallback(ui.style(), TextStyle::Small.into()),
                    color,
                );
            }
        }
        if self.hover_text {
            response.on_hover_text(self.label)
        } else {
            response
        }
    }
}
