use crate::input::discard_escape_repeats;
use crate::primitives::focus::FocusRestore;
use egui::{Context, Id, InnerResponse, Response, Ui};

/// Dialog open state and return focus. Popups use egui's native memory instead.
/// Call the dialog's `show` each frame, including while closed, so deferred
/// focus restoration runs after the opener is rendered again.
#[derive(Debug, Default)]
pub struct DialogState {
    open: bool,
    just_opened: bool,
    restore: FocusRestore,
}

impl DialogState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The next `show` is the opening pass. Use this when the initial focus
    /// target's native response is only available while drawing the contents.
    pub fn just_opened(&self) -> bool {
        self.just_opened
    }

    /// Programmatic opening captures the currently focused control.
    pub fn open(&mut self, ctx: &Context) {
        if !self.open {
            self.open = true;
            self.just_opened = true;
            self.restore.pending = ctx.memory(|m| m.focused());
            ctx.request_repaint();
        }
    }

    /// Mouse and keyboard opening both return to this specific control.
    pub fn open_from(&mut self, opener: &Response) {
        if !self.open {
            self.open(&opener.ctx);
            self.restore.pending = Some(opener.id);
        }
    }

    pub fn close(&mut self, ctx: &Context) {
        if !self.open {
            return;
        }
        self.open = false;
        self.just_opened = false;
        ctx.request_repaint();
    }

    fn prepare(&mut self, ctx: &Context) {
        if !self.open {
            self.restore.apply(ctx);
        }
    }
}

/// Dialog policy around a native Modal; usable without a hunter theme.
#[derive(Clone, Copy, Debug)]
pub struct DialogInteraction {
    initial_focus: Option<Id>,
    dismiss_on_backdrop: bool,
}
impl Default for DialogInteraction {
    fn default() -> Self {
        Self {
            initial_focus: None,
            dismiss_on_backdrop: true,
        }
    }
}
impl DialogInteraction {
    pub fn initial_focus(mut self, id: Id) -> Self {
        self.initial_focus = Some(id);
        self
    }
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }
    pub fn show<R>(
        self,
        ctx: &Context,
        state: &mut DialogState,
        native: egui::Modal,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<R>> {
        state.prepare(ctx);
        if !state.open {
            return None;
        }
        discard_escape_repeats(ctx);
        let modal = native.show(ctx, content);
        let close = if self.dismiss_on_backdrop {
            modal.should_close()
        } else {
            // Native Modal always dismisses on backdrop clicks.
            modal.response.should_close()
                || (modal.is_top_modal && !modal.any_popup_open && crate::consume_escape(ctx))
        };
        if close {
            state.close(ctx);
        } else if state.just_opened
            && !egui::Popup::is_any_open(ctx)
            && let Some(id) = self.initial_focus
        {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
        state.just_opened = false;
        Some(InnerResponse::new(modal.inner, modal.response))
    }
}
