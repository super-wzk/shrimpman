use egui::{Id, InnerResponse, Ui, UiBuilder};

/// Responsive, equal-width columns backed by egui's native row/column layout.
/// Give each instance a globally unique ID. Item IDs survive width-driven reflow.
#[must_use = "Call show to lay out the items"]
pub struct ResponsiveColumns {
    id: Id,
    min_width: f32,
    max_columns: usize,
    gap: Option<f32>,
}

impl ResponsiveColumns {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            min_width: 400.0,
            max_columns: 2,
            gap: None,
        }
    }

    pub fn min_column_width(mut self, width: f32) -> Self {
        self.min_width = width.max(1.0);
        self
    }
    pub fn max_columns(mut self, count: usize) -> Self {
        self.max_columns = count.max(1);
        self
    }
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap.max(0.0));
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
            let gap = self.gap.unwrap_or(item_spacing.x * 2.0);
            let columns = (((ui.available_width() + gap) / (self.min_width + gap)) as usize)
                .clamp(1, self.max_columns)
                .min(count.max(1));
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
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
