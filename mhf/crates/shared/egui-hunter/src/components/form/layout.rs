use egui::{Align, Id, InnerResponse, Layout, Ui, UiBuilder};

use super::{Field, field::FieldLayout};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelPlacement {
    #[default]
    Above,
    Left,
}

/// Responsive form rows with shared label widths and aligned control starts.
/// Each field's stable ID survives a change in the number of columns.
#[must_use = "Call show to lay out the fields"]
pub struct FormLayout {
    id: Id,
    max_columns: usize,
    min_column_width: f32,
    label_placement: LabelPlacement,
    label_width: Option<f32>,
    label_align: Align,
}

impl FormLayout {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            max_columns: 1,
            min_column_width: 240.0,
            label_placement: LabelPlacement::Above,
            label_width: None,
            label_align: Align::Min,
        }
    }

    pub fn max_columns(mut self, count: usize) -> Self {
        self.max_columns = count.max(1);
        self
    }

    pub fn min_column_width(mut self, width: f32) -> Self {
        self.min_column_width = width.max(1.0);
        self
    }

    /// Left labels move above controls when less than 120 points remain for input.
    pub fn label_placement(mut self, placement: LabelPlacement) -> Self {
        self.label_placement = placement;
        self
    }

    /// Override the shared label width. Zero places existing labels above controls.
    /// The default is the measured width, limited to 40% of a column and 200 points.
    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = Some(width.max(0.0));
        self
    }

    pub fn label_align(mut self, align: Align) -> Self {
        self.label_align = align;
        self
    }

    /// The control closure runs once per field in display order for each egui pass.
    /// This layout does not run an extra sizing pass.
    /// Labels and feedback belong to `fields`; pass unadorned controls to the closure.
    pub fn show(
        self,
        ui: &mut Ui,
        fields: &[Field<'_>],
        mut control: impl FnMut(&mut Ui, usize) -> egui::Response,
    ) -> InnerResponse<Vec<egui::Response>> {
        ui.scope_builder(
            UiBuilder::new()
                .id(self.id)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                let gap = ui.spacing().item_spacing.x;
                let width = ui.available_width().max(0.0);
                let columns = (((width + gap) / (self.min_column_width + gap)) as usize)
                    .clamp(1, self.max_columns)
                    .min(fields.len().max(1));
                let column_width = ((width - gap * (columns - 1) as f32) / columns as f32).max(0.0);
                let label_width = self.label_width.unwrap_or_else(|| {
                    fields
                        .iter()
                        .filter_map(|field| field.label_galley(ui, f32::INFINITY))
                        .map(|label| label.size().x)
                        .fold(0.0, f32::max)
                        .min(column_width * 0.4)
                        .min(200.0)
                });
                let placement = if self.label_placement == LabelPlacement::Left
                    && (label_width > 0.0 || !fields.iter().any(Field::has_label))
                    && column_width >= label_width + gap + 120.0
                {
                    LabelPlacement::Left
                } else {
                    LabelPlacement::Above
                };
                let mut responses = Vec::with_capacity(fields.len());
                for start in (0..fields.len()).step_by(columns) {
                    let row = &fields[start..(start + columns).min(fields.len())];
                    let labels: Vec<_> = row
                        .iter()
                        .map(|field| {
                            field.label_galley(
                                ui,
                                match placement {
                                    LabelPlacement::Above => column_width,
                                    LabelPlacement::Left => label_width,
                                },
                            )
                        })
                        .collect();
                    let label_height = labels
                        .iter()
                        .flatten()
                        .map(|label| label.size().y)
                        .fold(0.0, f32::max);
                    let control_offset = row
                        .iter()
                        .zip(&labels)
                        .map(|(field, label)| {
                            field.control_offset(
                                label.as_ref().map_or(0.0, |label| label.size().y),
                                ui.spacing()
                                    .interact_size
                                    .y
                                    .max(crate::Density::get(ui).field_height()),
                            )
                        })
                        .fold(0.0, f32::max);
                    ui.columns(columns, |columns| {
                        for (index, (field, label)) in row.iter().zip(labels).enumerate() {
                            let layout = match placement {
                                LabelPlacement::Above => FieldLayout::Above {
                                    label_height,
                                    align: self.label_align,
                                },
                                LabelPlacement::Left => FieldLayout::Left {
                                    label_width,
                                    control_offset,
                                    align: self.label_align,
                                },
                            };
                            responses.push(field.show_with_layout(
                                &mut columns[index],
                                self.id.with(field.id),
                                label,
                                layout,
                                |ui| control(ui, start + index),
                            ));
                        }
                    });
                }
                responses
            },
        )
    }
}
