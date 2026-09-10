mod input;

use crate::{
    HostIme, InputCaptureState, InputPolicy, Overlay, OverlayRegistry,
    dx9::D3d9Hook,
    egui::{self, Context},
};

pub(crate) unsafe fn install(
    registry: OverlayRegistry,
    adapter: Option<std::sync::Arc<dyn HostIme>>,
) -> Result<(OverlayHook, InputCaptureState), Box<InstallError>> {
    let mut hook = OverlayHook::default();
    let result = (|| {
        let ui = GameOverlay { registry };
        let renderer = match adapter {
            Some(adapter) => unsafe { D3d9Hook::install_with_ime(ui, adapter) },
            None => unsafe { D3d9Hook::install(ui) },
        };
        let renderer =
            renderer.map_err(|error| format!("failed to install D3D9 overlay: {error}"))?;
        let capture = renderer.input_capture();
        hook.renderer = Some(renderer);
        hook.input = Some(unsafe { input::install(capture.clone()) }?);
        Ok(capture)
    })();
    match result {
        Ok(capture) => Ok((hook, capture)),
        Err(error) => {
            let error = match hook.uninstall() {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; overlay rollback failed: {cleanup}"),
            };
            Err(Box::new(InstallError { error, hook }))
        }
    }
}

pub(crate) struct InstallError {
    pub(crate) error: String,
    // The owning Mod retains this even when rollback fails, so stop can retry.
    pub(crate) hook: OverlayHook,
}

#[derive(Default)]
pub(crate) struct OverlayHook {
    input: Option<mhf_hooks::HookGuard<input::HookState>>,
    renderer: Option<D3d9Hook>,
}

impl OverlayHook {
    pub(crate) fn uninstall(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Some(input) = &mut self.input {
            match input.uninstall() {
                Ok(()) => self.input = None,
                Err(error) => errors.push(error),
            }
        }
        if let Some(renderer) = &mut self.renderer {
            match renderer.uninstall() {
                Ok(()) => self.renderer = None,
                Err(error) => errors.push(error.to_string()),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

struct GameOverlay {
    registry: OverlayRegistry,
}

impl Overlay for GameOverlay {
    fn initialize(&mut self, context: &Context) {
        self.registry.initialize(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.registry.ui(ui);
        draw_cursor(ui.ctx());
    }

    fn input_policy(&self, context: &Context) -> InputPolicy {
        self.registry.input_policy(context)
    }

    fn platform_output(&mut self, context: &Context, output: &egui::PlatformOutput) {
        self.registry.platform_output(context, output);
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
