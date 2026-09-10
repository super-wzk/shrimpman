mod field;
mod layout;

pub use field::Field;
pub use layout::{FormLayout, LabelPlacement};

use egui::{Color32, Ui};

/// Validation is supplied by the host. A message replaces ordinary help text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Validation<'a> {
    #[default]
    None,
    Warning(&'a str),
    Error(&'a str),
    Success(&'a str),
}

const VALIDATION_COLOR: &str = "egui-hunter-field-validation";

/// A nested field owns its validation; unadorned controls inherit their field.
pub(crate) fn validation_color(ui: &Ui) -> Option<Color32> {
    ui.stack()
        .iter()
        .find_map(|node| {
            node.tags()
                .get_downcast::<Option<Color32>>(VALIDATION_COLOR)
                .copied()
        })
        .flatten()
}
