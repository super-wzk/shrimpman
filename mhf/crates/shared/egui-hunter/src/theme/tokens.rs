use egui::{Color32, Context, Id, InnerResponse, Ui, UiBuilder, UiStackInfo};

use super::Density;

const TOKENS: &str = "egui-hunter-tokens";

/// Hunter-specific colors and control minimum sizes without native Style fields.
/// General colors, fonts, spacing and interaction states belong to the native style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// Hunter-only minimum sizes accompanying the native spacing preset.
    /// Prefer `Density::scope` so native spacing and these sizes change together.
    pub density: Density,
    /// Persistent fill for the main action in a group.
    pub primary: Color32,
    pub on_primary: Color32,
    pub success: Color32,
    pub success_fill: Color32,
    pub danger_fill: Color32,
    /// Keyboard/controller focus color for the control's existing inside border.
    pub focus: Color32,
}

impl Default for Tokens {
    fn default() -> Self {
        Self {
            density: Density::Standard,
            primary: Color32::from_rgb(0xD8, 0xB8, 0x78),
            on_primary: Color32::from_rgb(0x1C, 0x1A, 0x15),
            success: Color32::from_rgb(0x93, 0xCB, 0xA8),
            success_fill: Color32::from_rgb(0x1D, 0x2A, 0x23),
            danger_fill: Color32::from_rgb(0x34, 0x23, 0x22),
            focus: Color32::from_rgb(0xEC, 0xE4, 0xD2),
        }
    }
}

impl Tokens {
    /// Read the closest local override, falling back to the host's installed tokens.
    pub fn get(ui: &Ui) -> Self {
        ui.stack()
            .iter()
            .find_map(|node| node.tags().get_downcast::<Self>(TOKENS).copied())
            .unwrap_or_else(|| Self::from_context(ui.ctx()))
    }

    /// Read the host default. Independent Areas start with this value.
    pub fn from_context(ctx: &Context) -> Self {
        ctx.data(|data| data.get_temp(Id::new(TOKENS)))
            .unwrap_or_default()
    }

    pub(crate) fn install(self, ctx: &Context) {
        ctx.data_mut(|data| data.insert_temp(Id::new(TOKENS), self));
    }

    /// Override only hunter-specific values for this scope and its descendants.
    /// Native styles still inherit through egui. Independent Areas need an explicit
    /// copy of both `ui.style().clone()` and `Tokens::get(ui)` to share this scope.
    pub fn scope<R>(self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        ui.scope_builder(
            UiBuilder::new().ui_stack_info(UiStackInfo::default().with_tag_value(TOKENS, self)),
            content,
        )
    }
}
