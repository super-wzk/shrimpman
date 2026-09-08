#[cfg(feature = "translation")]
mod ime;
mod input;

#[cfg(feature = "debug")]
use egui_hunter::Theme;
use mhf_overlay::{
    InputCapture, InputPolicy, Overlay,
    dx9::D3d9Hook,
    egui::{self, Context},
};

pub(crate) unsafe fn install(
    _module: windows::Win32::Foundation::HMODULE,
    #[cfg(feature = "debug")] debug: Option<std::sync::Arc<crate::debug::DebugControl>>,
) -> Result<OverlayHook, String> {
    #[cfg(feature = "translation")]
    let adapter = unsafe { ime::GameIme::new(_module) }?;
    let ui = GameOverlay {
        #[cfg(feature = "debug")]
        debug: debug.map(crate::debug::DebugWindow::new),
    };
    #[cfg(feature = "translation")]
    let renderer = unsafe { D3d9Hook::install_with_ime(ui, adapter.clone()) };
    #[cfg(not(feature = "translation"))]
    let renderer = unsafe { D3d9Hook::install(ui) };
    let renderer = renderer.map_err(|error| format!("failed to install D3D9 overlay: {error}"))?;
    let input = unsafe { input::install(renderer.input_capture()) }?;
    #[cfg(feature = "translation")]
    let ime = unsafe { adapter.install(renderer.input_capture()) }?;
    Ok(OverlayHook {
        input,
        renderer,
        #[cfg(feature = "translation")]
        ime,
    })
}

pub(crate) struct OverlayHook {
    input: mhf_hooks::HookGuard<input::HookState>,
    renderer: D3d9Hook,
    #[cfg(feature = "translation")]
    ime: mhf_hooks::HookGuard<ime::HookState>,
}

impl OverlayHook {
    pub(crate) fn uninstall(mut self) -> Result<(), String> {
        let input = self.input.uninstall();
        let renderer = self.renderer.uninstall().map_err(|error| error.to_string());
        #[cfg(feature = "translation")]
        let ime = self.ime.uninstall();
        let errors = [
            input.err(),
            renderer.err(),
            #[cfg(feature = "translation")]
            ime.err(),
        ]
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
    #[cfg(feature = "debug")]
    debug: Option<crate::debug::DebugWindow>,
}

impl Overlay for GameOverlay {
    #[cfg(feature = "debug")]
    fn initialize(&mut self, context: &Context) {
        if self.debug.is_some() {
            crate::font::install(context);
            Theme::default().apply(context);
        }
    }

    fn ui(&mut self, _ui: &mut egui::Ui) {
        #[cfg(feature = "debug")]
        if let Some(debug) = &mut self.debug {
            let context = _ui.ctx();
            debug.show(context);
            draw_cursor(context);
        }
    }

    fn input_policy(&self, _context: &Context) -> InputPolicy {
        #[cfg(feature = "debug")]
        if self.debug.is_some() {
            return InputPolicy::default();
        }
        InputPolicy {
            pointer: InputCapture::PassThrough,
            keyboard: InputCapture::PassThrough,
        }
    }
}

#[cfg(feature = "debug")]
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
