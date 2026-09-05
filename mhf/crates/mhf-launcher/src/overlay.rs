use mhf_overlay::{
    Overlay,
    dx9::D3d9Hook,
    egui::{self, Context},
};

pub(crate) fn install() -> Result<D3d9Hook, String> {
    unsafe { D3d9Hook::install(GameOverlay) }
        .map_err(|error| format!("failed to install D3D9 overlay: {error}"))
}

struct GameOverlay;

impl Overlay for GameOverlay {
    fn ui(&mut self, context: &Context) {
        egui::Window::new("Shrimpman Debug")
            .default_pos([16.0, 16.0])
            .resizable(false)
            .show(context, |ui| {
                ui.label("D3D9 overlay is active.");
            });

        draw_cursor(context);
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
