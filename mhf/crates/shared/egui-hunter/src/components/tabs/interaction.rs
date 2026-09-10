use crate::primitives::navigation::NavigationState;
use egui::{Id, InnerResponse, Key, Modifiers, Response, Ui};

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

/// Tab-specific keyboard and activation policy with caller-rendered headers.
pub struct TabsInteraction {
    id: Id,
    gap: f32,
}
impl TabsInteraction {
    pub fn new(id: Id) -> Self {
        Self { id, gap: 8.0 }
    }
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }
    pub fn show<R>(
        self,
        ui: &mut Ui,
        state: &mut NavigationState,
        tabs: &[Tab<'_>],
        mut render_header: impl FnMut(&mut Ui, &Tab<'_>, Id, bool) -> Response,
        content: impl FnOnce(&mut Ui, Id) -> R,
    ) -> InnerResponse<Option<R>> {
        state.reconcile(tabs.iter().filter(|tab| tab.enabled).map(|tab| tab.id));
        ui.scope(|ui| {
            let mut headers = Vec::with_capacity(tabs.len());
            ui.horizontal_wrapped(|ui| {
                for tab in tabs {
                    let response = ui
                        .add_enabled_ui(tab.enabled, |ui| {
                            render_header(
                                ui,
                                tab,
                                self.id.with(("header", tab.id)),
                                state.selected() == Some(tab.id),
                            )
                        })
                        .inner;
                    if response.clicked() {
                        state.select(tab.id);
                    }
                    headers.push((tab.id, response));
                }
            });
            // Observe actual native responses so headers can use any egui widget.
            let enabled = headers
                .iter()
                .filter(|(_, response)| response.enabled() && response.sense.is_focusable());
            if crate::primitives::focus::navigation_allowed(ui)
                && let Some(index) = enabled
                    .clone()
                    .position(|(_, response)| response.has_focus())
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
                if let Some((id, response)) = next.and_then(|index| enabled.clone().nth(index)) {
                    state.select(*id);
                    // egui already saw the arrow at begin_pass.
                    ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
                    response.request_focus();
                    ui.ctx().request_repaint();
                }
            }
            ui.add_space(self.gap);
            state.show(ui, self.id, content)
        })
    }
}
