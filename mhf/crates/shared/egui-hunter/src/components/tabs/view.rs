use super::interaction::{Tab, TabsInteraction};
use crate::{NavigationState, theme::paint};
use egui::{
    Id, InnerResponse, Sense, TextStyle, TextWrapMode, Ui, WidgetInfo, WidgetText, WidgetType,
    pos2, vec2,
};

#[must_use = "Call show to render tabs and their selected content"]
pub struct Tabs {
    interaction: TabsInteraction,
}

impl Tabs {
    pub fn new(id: Id) -> Self {
        Self {
            interaction: TabsInteraction::new(id),
        }
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        state: &mut NavigationState,
        tabs: &[Tab<'_>],
        content: impl FnOnce(&mut Ui, Id) -> R,
    ) -> InnerResponse<Option<R>> {
        self.interaction.gap(ui.spacing().item_spacing.x).show(
            ui,
            state,
            tabs,
            |ui, tab, id, selected| {
                let galley = WidgetText::from(tab.label).into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    TextStyle::Button,
                );
                let padding = ui.spacing().button_padding;
                let size = (galley.size() + padding * 2.0).max(vec2(
                    0.0,
                    ui.spacing()
                        .interact_size
                        .y
                        .max(crate::Density::get(ui).tab_height()),
                ));
                let (_, rect) = ui.allocate_space(size);
                let response = ui.interact(rect, id, Sense::click());
                crate::primitives::focus::focus_on_click(&response);
                crate::primitives::focus::scroll_on_focus(&response);
                response.widget_info(|| {
                    WidgetInfo::selected(
                        WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        selected,
                        tab.label,
                    )
                });
                if ui.is_rect_visible(rect) {
                    if response.hovered() || response.is_pointer_button_down_on() {
                        ui.painter_at(rect).rect_filled(
                            rect,
                            ui.visuals().widgets.inactive.corner_radius,
                            ui.visuals().faint_bg_color,
                        );
                    }
                    let color = if selected {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.painter_at(rect)
                        .galley(rect.center() - galley.size() * 0.5, galley, color);
                    if paint::focused(ui, &response) {
                        paint::focus_border(ui, rect, ui.visuals().widgets.inactive.corner_radius);
                    }
                    if selected {
                        ui.painter_at(rect).line_segment(
                            [
                                pos2(rect.left() + padding.x, rect.bottom() - 1.0),
                                pos2(rect.right() - padding.x, rect.bottom() - 1.0),
                            ],
                            egui::Stroke::new(2.0, color),
                        );
                    }
                }
                response
            },
            content,
        )
    }
}
