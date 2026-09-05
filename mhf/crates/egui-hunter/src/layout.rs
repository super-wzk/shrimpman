use egui::{Id, InnerResponse, Key, Modifiers, Ui, UiBuilder};

use crate::Theme;

/// Responsive, equal-width columns backed by egui's native row/column layout.
/// Give each instance a globally unique ID. Item IDs survive width-driven reflow.
#[must_use = "Call show to lay out the items"]
pub struct ResponsiveColumns {
    id: Id,
    min_width: f32,
    max_columns: usize,
    gap: f32,
}

impl Theme {
    pub fn columns(&self, id: Id) -> ResponsiveColumns {
        ResponsiveColumns {
            id,
            min_width: 400.0,
            max_columns: 2,
            gap: self.metrics.gap * 2.0,
        }
    }

    pub fn tabs(&self, id: Id) -> Tabs<'_> {
        Tabs { theme: self, id }
    }
}

impl ResponsiveColumns {
    pub fn min_column_width(mut self, width: f32) -> Self {
        self.min_width = width.max(1.0);
        self
    }
    pub fn max_columns(mut self, count: usize) -> Self {
        self.max_columns = count.max(1);
        self
    }
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// Indices must identify the same items across frames. For reordered data,
    /// use stable IDs inside each item instead of relying on its position.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        count: usize,
        mut content: impl FnMut(&mut Ui, usize) -> R,
    ) -> InnerResponse<Vec<R>> {
        ui.scope(|ui| {
            let item_spacing = ui.spacing().item_spacing;
            let columns = (((ui.available_width() + self.gap) / (self.min_width + self.gap))
                as usize)
                .clamp(1, self.max_columns)
                .min(count.max(1));
            ui.spacing_mut().item_spacing = egui::vec2(self.gap, self.gap);
            let mut result = Vec::with_capacity(count);
            for start in (0..count).step_by(columns) {
                ui.columns(columns, |cols| {
                    for (column, ui) in cols.iter_mut().enumerate().take(count - start) {
                        let index = start + column;
                        let inner =
                            ui.scope_builder(UiBuilder::new().id(self.id.with(index)), |ui| {
                                ui.spacing_mut().item_spacing = item_spacing;
                                content(ui, index)
                            });
                        result.push(inner.inner);
                    }
                });
            }
            result
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tab<'a> {
    pub id: Id,
    pub label: &'a str,
    pub enabled: bool,
}

impl<'a> Tab<'a> {
    pub fn new(id: Id, label: &'a str) -> Self {
        Self {
            id,
            label,
            enabled: true,
        }
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Selection belongs to the host; content widget state is scoped by tab ID.
#[derive(Debug, Default)]
pub struct TabsState {
    selected: Option<Id>,
}

impl TabsState {
    pub fn selected(&self) -> Option<Id> {
        self.selected
    }
    pub fn select(&mut self, id: Id) {
        self.selected = Some(id);
    }
}

#[must_use = "Call show to render tabs and their selected content"]
pub struct Tabs<'a> {
    theme: &'a Theme,
    id: Id,
}

impl Tabs<'_> {
    /// Left/Right/Home/End navigate enabled tabs only while a tab header has
    /// focus. Arrow keys in the page's inputs keep their native behavior.
    /// Empty/all-disabled lists render no content. Removed selections fall back
    /// to the first enabled tab. IDs must be unique within this container.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        state: &mut TabsState,
        tabs: &[Tab<'_>],
        content: impl FnOnce(&mut Ui, Id) -> R,
    ) -> InnerResponse<Option<R>> {
        let enabled = tabs.iter().filter(|tab| tab.enabled);
        if !enabled.clone().any(|tab| Some(tab.id) == state.selected) {
            state.selected = enabled.clone().next().map(|tab| tab.id);
        }
        let header_id = |id| self.id.with(("header", id));
        let focused = ui.ctx().memory(|m| m.focused());
        if ui.is_enabled()
            && let Some(index) = enabled
                .clone()
                .position(|tab| Some(header_id(tab.id)) == focused)
        {
            let count = enabled.clone().count();
            let next = ui.input_mut(|input| {
                if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
                    Some((index + 1) % count)
                } else if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                    Some((index + count - 1) % count)
                } else if input.consume_key(Modifiers::NONE, Key::Home) {
                    Some(0)
                } else if input.consume_key(Modifiers::NONE, Key::End) {
                    Some(count - 1)
                } else {
                    None
                }
            });
            if let Some(tab) = next.and_then(|index| enabled.clone().nth(index)) {
                state.selected = Some(tab.id);
                ui.ctx().memory_mut(|m| m.request_focus(header_id(tab.id)));
            }
        }
        ui.scope(|ui| {
            ui.horizontal_wrapped(|ui| {
                for tab in tabs {
                    if ui
                        .add_enabled(
                            tab.enabled,
                            self.theme
                                .button(tab.label)
                                .id(header_id(tab.id))
                                .selected(state.selected == Some(tab.id)),
                        )
                        .clicked()
                    {
                        state.selected = Some(tab.id);
                    }
                }
            });
            ui.add_space(self.theme.metrics.gap);
            state.selected.map(|id| {
                ui.scope_builder(UiBuilder::new().id(self.id.with(("content", id))), |ui| {
                    content(ui, id)
                })
                .inner
            })
        })
    }
}
