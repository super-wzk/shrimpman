use mhf_ai_debug::{ProgramLocation, Snapshot};

pub(super) fn location(pc: Option<&ProgramLocation>) -> String {
    match pc {
        Some(pc) => format!(
            "v{} · 脚本 {} + 0x{:04X}",
            pc.revision, pc.script, pc.offset
        ),
        None => "无续行位置".into(),
    }
}

pub(super) fn fields(ui: &mut egui::Ui, snapshot: &Snapshot) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!("帧 {}", snapshot.frame));
        ui.monospace(location(snapshot.pc.as_ref()));
    });
    egui::Grid::new("ai-debug-fields")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (name, value) in &snapshot.fields {
                ui.monospace(name);
                ui.monospace(value.to_string());
                ui.end_row();
            }
        });
}
