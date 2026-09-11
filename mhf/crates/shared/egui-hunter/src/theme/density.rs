use egui::{InnerResponse, Margin, Style, Ui, vec2};

use super::Tokens;

/// Control density in logical points. Fonts, DPI, colors and input stay intact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Density {
    /// The original hunter layout: 36-point controls and 40-point fields.
    #[default]
    Standard,
    /// Tighter spacing, 24-point controls and 28-point fields.
    Compact,
}

impl Density {
    /// Read the closest density scope or the host's installed theme default.
    pub fn get(ui: &Ui) -> Self {
        Tokens::get(ui).density
    }

    /// Apply density to this subtree without changing Context or sibling Uis.
    /// Native egui widgets inherit the spacing; hunter widgets also inherit
    /// their density-specific minimum sizes through the existing token scope.
    ///
    /// Detached Areas need an explicit copy of both `ui.style().clone()` and
    /// `Tokens::get(ui)`, as with other local hunter styles. Alternatively, call
    /// this method inside the new Area's content closure.
    pub fn scope<R>(self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        ui.scope(|ui| {
            self.apply_spacing(ui.style_mut());
            let tokens = Tokens {
                density: self,
                ..Tokens::get(ui)
            };
            tokens.scope(ui, content).inner
        })
    }

    pub(crate) fn apply_spacing(self, style: &mut Style) {
        let spacing = &mut style.spacing;
        match self {
            Self::Standard => {
                spacing.item_spacing = vec2(8.0, 6.0);
                spacing.window_margin = Margin::same(12);
                spacing.button_padding = vec2(12.0, 7.0);
                spacing.icon_width = 20.0;
                spacing.icon_width_inner = 16.0;
                spacing.icon_spacing = 8.0;
                spacing.interact_size = vec2(36.0, 36.0);
                spacing.indent = 18.0;
                spacing.slider_width = 180.0;
                spacing.slider_rail_height = 10.0;
            }
            Self::Compact => {
                spacing.item_spacing = vec2(6.0, 4.0);
                spacing.window_margin = Margin::same(8);
                spacing.button_padding = vec2(8.0, 3.0);
                spacing.icon_width = 16.0;
                spacing.icon_width_inner = 12.0;
                spacing.icon_spacing = 6.0;
                spacing.interact_size = vec2(24.0, 24.0);
                spacing.indent = 14.0;
                spacing.slider_width = 160.0;
                spacing.slider_rail_height = 6.0;
            }
        }
    }

    pub(crate) fn field_height(self) -> f32 {
        match self {
            Self::Standard => 40.0,
            Self::Compact => 28.0,
        }
    }

    pub(crate) fn primary_button_height(self) -> f32 {
        match self {
            Self::Standard => 44.0,
            Self::Compact => 28.0,
        }
    }

    pub(crate) fn tab_height(self) -> f32 {
        match self {
            Self::Standard => 36.0,
            Self::Compact => 24.0,
        }
    }

    pub(crate) fn password_button_width(self) -> f32 {
        match self {
            Self::Standard => 28.0,
            Self::Compact => 24.0,
        }
    }
}
