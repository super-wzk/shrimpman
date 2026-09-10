use std::sync::Arc;

use egui_hunter::Theme;
use mhf_ui::{Overlay, egui};

use super::{DebugControl, DebugWindow, InputController};

pub(crate) fn create(control: Arc<DebugControl>) -> Box<dyn Overlay> {
    Box::new(DebugOverlay {
        window: DebugWindow::new(Arc::clone(&control)),
        control,
        input: InputController::default(),
    })
}

struct DebugOverlay {
    control: Arc<DebugControl>,
    window: DebugWindow,
    input: InputController,
}

impl Overlay for DebugOverlay {
    fn initialize(&mut self, context: &egui::Context) {
        mhf_font::install(context);
        Theme::default().apply(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx();
        let snapshot = self.control.snapshot();
        let capture = self.window.show(context, &snapshot, &mut self.input);
        self.input
            .update(context, &self.control, &snapshot, capture);
    }
}
