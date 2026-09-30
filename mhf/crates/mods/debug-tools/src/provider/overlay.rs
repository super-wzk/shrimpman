use std::sync::Arc;

use egui_hunter::Theme;
use mhf_ui::{Overlay, egui};

use super::{DebugControl, frontend::WindowState, input, ui};

pub(super) fn create(control: Arc<DebugControl>, window: Arc<WindowState>) -> Box<dyn Overlay> {
    Box::new(DebugOverlay { window, control })
}

struct DebugOverlay {
    control: Arc<DebugControl>,
    window: Arc<WindowState>,
}

impl Overlay for DebugOverlay {
    fn initialize(&mut self, context: &egui::Context) {
        mhf_font::install(context);
        Theme::default().apply(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx();
        let snapshot = self.control.snapshot();
        if input::toggle_panel(context) {
            self.window.toggle();
        }
        let settings = self.control.ui_settings();
        ui::show_hud(context, &snapshot, settings.hud_target);
        settings.input.update(context, &self.control, &snapshot);
    }
}
