use egui::Response;

/// Focus/hover visibility policy; egui handles delay and placement.
#[derive(Clone, Copy, Debug)]
pub struct TooltipInteraction {
    on_focus: bool,
}
impl Default for TooltipInteraction {
    fn default() -> Self {
        Self { on_focus: true }
    }
}
impl TooltipInteraction {
    pub fn on_focus(mut self, on_focus: bool) -> Self {
        self.on_focus = on_focus;
        self
    }
    pub fn native(self, anchor: &Response) -> Option<egui::Tooltip<'_>> {
        let ctx = &anchor.ctx;
        if !anchor.interact_rect.is_positive()
            || !ctx.memory(|m| m.allows_interaction(anchor.layer_id))
            || (egui::Popup::is_any_open(ctx) && anchor.layer_id.order != egui::Order::Foreground)
        {
            return None;
        }
        let native = if self.on_focus && anchor.has_focus() {
            egui::Tooltip::for_widget(anchor)
        } else if anchor.enabled() {
            egui::Tooltip::for_enabled(anchor)
        } else {
            egui::Tooltip::for_disabled(anchor)
        };
        Some(native)
    }
}
