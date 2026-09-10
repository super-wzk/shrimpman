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
        let closed_id = id.with("hunter-popup-closed");
        let pass = ctx.cumulative_pass_nr();
        let closed_last_pass = ctx
            .data_mut(|data| data.remove_temp::<u64>(closed_id))
            .is_some_and(|closed_at| closed_at.saturating_add(1) == pass);
        // Native Popup uses the previous Area response to detect its first pass.
        let was_visible = ctx.read_response(id).is_some();
        let output = native.show(content);
        if !egui::Popup::is_id_open(ctx, id) {
            let another_popup_open = egui::Popup::is_any_open(ctx);
            // Outside clicks may focus a different control. A replacement popup
            // owns focus too; neither case should return it to the old anchor.
            let clicked_elsewhere = output.as_ref().map_or_else(
                || ctx.input(|input| input.pointer.any_click()) && !anchor.clicked(),
                |output| output.response.clicked_elsewhere(),
            );
            if !closed_last_pass && (output.is_some() || was_visible) {
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
        } else if (!was_visible || closed_last_pass)
            && let Some(id) = self.initial_focus
        {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
        output
    }
}
