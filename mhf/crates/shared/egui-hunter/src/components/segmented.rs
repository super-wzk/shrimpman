use egui::{Id, Key, Modifiers, Response, Ui};

use crate::primitives::focus::{focus_on_click, navigation_allowed, scroll_on_focus};

/// One choice in a segmented single-selection control.
pub struct Segment<'a, T> {
    value: T,
    label: &'a str,
    enabled: bool,
}

impl<'a, T> Segment<'a, T> {
    pub fn new(value: T, label: &'a str) -> Self {
        Self {
            value,
            label,
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Adjacent single-choice buttons. Tab/Enter use native egui focus; Left/Right
/// select the next enabled segment. A missing selection stays empty until input.
#[must_use = "Call show to render the segments"]
pub struct SegmentedControl {
    id: Id,
}

impl SegmentedControl {
    pub fn new(id: Id) -> Self {
        Self { id }
    }

    /// Updates `selected` on activation and reports it through `Response::changed`.
    /// Disabled choices remain visible and never change the selection.
    pub fn show<T: Copy + PartialEq>(
        self,
        ui: &mut Ui,
        selected: &mut Option<T>,
        segments: &[Segment<'_, T>],
    ) -> Response {
        let before = *selected;
        let output = ui.push_id(self.id, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let radius = ui.visuals().widgets.inactive.corner_radius;
                let mut responses = Vec::with_capacity(segments.len());
                for (index, segment) in segments.iter().enumerate() {
                    let mut corners = radius;
                    if index != 0 {
                        corners.nw = 0;
                        corners.sw = 0;
                    }
                    if index + 1 != segments.len() {
                        corners.ne = 0;
                        corners.se = 0;
                    }
                    let response = ui
                        .push_id(index, |ui| {
                            ui.add_enabled(
                                segment.enabled,
                                egui::Button::new(segment.label)
                                    .selected(*selected == Some(segment.value))
                                    .corner_radius(corners),
                            )
                        })
                        .inner;
                    if response.clicked() {
                        *selected = Some(segment.value);
                    }
                    focus_on_click(&response);
                    scroll_on_focus(&response);
                    responses.push(response);
                }
                if navigation_allowed(ui)
                    && let Some(index) = responses
                        .iter()
                        .position(|response| response.enabled() && response.has_focus())
                {
                    let forward = ui.input_mut(|input| {
                        if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
                            Some(true)
                        } else if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                            Some(false)
                        } else {
                            None
                        }
                    });
                    if let Some(forward) = forward {
                        for step in 1..segments.len() {
                            let next = if forward {
                                (index + step) % segments.len()
                            } else {
                                (index + segments.len() - step) % segments.len()
                            };
                            if responses[next].enabled() {
                                *selected = Some(segments[next].value);
                                ui.memory_mut(|memory| {
                                    memory.move_focus(egui::FocusDirection::None)
                                });
                                responses[next].request_focus();
                                ui.ctx().request_repaint();
                                break;
                            }
                        }
                    }
                }
            })
            .response
        });
        let mut response = output.inner;
        if *selected != before {
            response.mark_changed();
        }
        response
    }
}
