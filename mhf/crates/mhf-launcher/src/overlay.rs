mod ime;
mod input;
mod validation;

use egui_hunter::Theme;
use mhf_overlay::{
    InputPolicy, Overlay,
    dx9::D3d9Hook,
    egui::{self, Context},
};

pub(crate) unsafe fn install(
    module: windows::Win32::Foundation::HMODULE,
    #[cfg(feature = "debug")] debug: Option<std::sync::Arc<crate::debug::DebugControl>>,
) -> Result<OverlayHook, String> {
    let adapter = unsafe { ime::GameIme::new(module) }?;
    let ui = GameOverlay {
        validation: Default::default(),
        #[cfg(feature = "debug")]
        debug: debug.map(crate::debug::DebugWindow::new),
    };
    let renderer = unsafe { D3d9Hook::install_with_ime(ui, adapter.clone()) }
        .map_err(|error| format!("failed to install D3D9 overlay: {error}"))?;
    let input = unsafe { input::install(renderer.input_capture()) }?;
    let ime = unsafe { adapter.install(renderer.input_capture()) }?;
    Ok(OverlayHook {
        input,
        renderer,
        ime,
    })
}

pub(crate) struct OverlayHook {
    input: mhf_hooks::HookGuard<input::HookState>,
    renderer: D3d9Hook,
    ime: mhf_hooks::HookGuard<ime::HookState>,
}

impl OverlayHook {
    pub(crate) fn uninstall(mut self) -> Result<(), String> {
        let input = self.input.uninstall();
        let renderer = self.renderer.uninstall().map_err(|error| error.to_string());
        let ime = self.ime.uninstall();
        let errors = [input.err(), renderer.err(), ime.err()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

struct GameOverlay {
    validation: validation::ValidationPage,
    #[cfg(feature = "debug")]
    debug: Option<crate::debug::DebugWindow>,
}

impl Overlay for GameOverlay {
    fn initialize(&mut self, context: &Context) {
        crate::font::install(context);
        Theme::default().apply(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx();
        self.validation.show(context);
        #[cfg(feature = "debug")]
        if let Some(debug) = &mut self.debug {
            debug.show(
                context,
                self.validation.input_policy().keyboard != mhf_overlay::InputCapture::Block,
            );
        }
        draw_cursor(context);
    }

    fn input_policy(&self, _context: &Context) -> InputPolicy {
        let policy = self.validation.input_policy();
        #[cfg(feature = "debug")]
        if self.debug.is_some() && policy.keyboard != mhf_overlay::InputCapture::Block {
            return InputPolicy::default();
        }
        policy
    }
}

fn draw_cursor(context: &Context) {
    if !context.egui_wants_pointer_input() {
        return;
    }
    let Some(position) = context.pointer_latest_pos() else {
        return;
    };

    // MHF draws its cursor before Present, so egui windows otherwise cover it.
    let painter = context.debug_painter();
    let outline = egui::Stroke::new(4.0, egui::Color32::BLACK);
    let fill = egui::Stroke::new(2.0, egui::Color32::WHITE);
    let tail = [
        position + egui::vec2(5.0, 10.0),
        position + egui::vec2(10.0, 20.0),
    ];
    painter.line_segment(tail, outline);
    painter.line_segment(tail, fill);
    painter.add(egui::Shape::convex_polygon(
        vec![
            position,
            position + egui::vec2(0.0, 16.0),
            position + egui::vec2(11.0, 11.0),
        ],
        egui::Color32::WHITE,
        egui::Stroke::new(1.5, egui::Color32::BLACK),
    ));
}
