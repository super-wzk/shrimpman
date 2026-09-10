use crate::input::discard_escape_repeats;
use crate::primitives::focus::FocusRestore;
use egui::{Id, InnerResponse, Response, Ui};

/// Native popup closure and focus restoration, independent of its contents.
/// The popup must use egui's Memory open state (`open_memory` or
/// `from_toggle_button_response`) so programmatic closures can be detected.
#[derive(Clone, Copy, Debug, Default)]
pub struct PopupInteraction {
    initial_focus: Option<Id>,
}
impl PopupInteraction {
    pub fn initial_focus(mut self, id: Id) -> Self {
        self.initial_focus = Some(id);
        self
    }
    /// Call every frame so native and programmatic closure share the same return policy.
    pub fn show<R>(
        self,
        anchor: &Response,
        native: egui::Popup<'_>,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        let ctx = &anchor.ctx;
        discard_escape_repeats(ctx);
        let id = native.get_id();
        // Capture visibility before native.show for the initial-focus policy.
        let was_visible = ctx.read_response(id).is_some();
        let output = native.show(content);
        let closed_last_pass =
            Self::after_show(anchor, id, output.as_ref().map(|output| &output.response));
        if egui::Popup::is_id_open(ctx, id)
            && (!was_visible || closed_last_pass)
            && let Some(id) = self.initial_focus
        {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
        output
    }

    /// Finish a native popup rendered by another control, such as ComboBox.
    /// `output` is present only when the popup was shown in the current pass.
    pub(crate) fn after_show(anchor: &Response, id: Id, output: Option<&Response>) -> bool {
        let ctx = &anchor.ctx;
        let closed_id = id.with("hunter-popup-closed");
        let pass = ctx.cumulative_pass_nr();
        let closed_last_pass = ctx
            .data_mut(|data| data.remove_temp::<u64>(closed_id))
            .is_some_and(|closed_at| closed_at.saturating_add(1) == pass);
        if !egui::Popup::is_id_open(ctx, id) {
            let another_popup_open = egui::Popup::is_any_open(ctx);
            // Outside clicks may focus a different control. A replacement popup
            // owns focus too; neither case should return it to the old anchor.
            let clicked_elsewhere = output.map_or_else(
                || ctx.input(|input| input.pointer.any_click()) && !anchor.clicked(),
                Response::clicked_elsewhere,
            );
            // With no current output, read_response still sees the previous
            // Area pass and detects a programmatically closed popup.
            if !closed_last_pass && (output.is_some() || ctx.read_response(id).is_some()) {
                // The native Area has finished. Return now, before the next
                // Tab/Enter is routed from a disappearing popup item.
                FocusRestore {
                    pending: (!clicked_elsewhere && !another_popup_open).then_some(anchor.id),
                }
                .apply(ctx);
                // Its previous Area response survives one pass; remember that
                // closure was already handled without queuing another focus request.
                ctx.data_mut(|data| data.insert_temp(closed_id, pass));
                ctx.request_repaint();
            }
            // Native Popup observes Escape without consuming it. Stop that same
            // key from reaching a parent dialog or menu later in this frame.
            if output.is_some() && !another_popup_open {
                crate::consume_escape(ctx);
            }
        }
        closed_last_pass
    }
}
