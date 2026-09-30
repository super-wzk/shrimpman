use super::{DebuggerUi, session};
use crate::provider::{AiTarget, DebugSnapshot};
use egui_hunter::{Button, Icon, IconButton};
use mhf_ai_debug::{Recording, ReplaySession};

impl DebuggerUi {
    pub(in super::super) fn replay_controls(&mut self, ui: &mut egui::Ui) {
        let (position, count) = self.replay.as_ref().map_or((0, 0), |replay| {
            (replay.position(), replay.recording().entries.len())
        });
        let reset = ui
            .add_enabled(
                position > 0,
                IconButton::new(Icon::Refresh, "回到起点").id(egui::Id::new("ai-replay-start")),
            )
            .clicked();
        let previous = ui
            .add_enabled(
                position > 0,
                IconButton::new(Icon::Undo, "后退一步").id(egui::Id::new("ai-replay-previous")),
            )
            .clicked();
        let next = ui
            .add_enabled(
                position < count,
                IconButton::new(Icon::Step, "前进一步").id(egui::Id::new("ai-replay-next")),
            )
            .clicked();
        if let Some(replay) = self.replay.as_mut() {
            if reset && let Err(error) = replay.seek(0) {
                self.error = Some(error.to_string());
            }
            if previous && let Err(error) = replay.step_back() {
                self.error = Some(error.to_string());
            }
            if next && let Err(error) = replay.step_forward() {
                self.error = Some(error.to_string());
            }
            ui.weak(format!("{} / {}", replay.position(), count));
            self.selected_event = replay.current_entry().map(|entry| entry.sequence);
        }
    }

    pub(in super::super) fn replay_actions(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        target: Option<AiTarget>,
    ) {
        let debug = session(snapshot, target);
        if ui
            .add(
                IconButton::new(Icon::FolderOpen, "导入录制").id(egui::Id::new("ai-replay-import")),
            )
            .clicked()
        {
            self.import_open = true;
        }
        if ui
            .add_enabled(
                debug.is_some(),
                IconButton::new(Icon::Refresh, "载入现场轨迹")
                    .id(egui::Id::new("ai-replay-load-live")),
            )
            .clicked()
            && let Some(debug) = debug
        {
            self.load_recording(debug.recording.clone());
            self.replay_requested = true;
        }
        if ui
            .add_enabled(
                self.replay.is_some(),
                IconButton::new(Icon::Save, "保存录制").id(egui::Id::new("ai-replay-save")),
            )
            .clicked()
            && let Some(replay) = &self.replay
        {
            self.request_recording_save(replay.recording().to_json());
        }
    }

    pub(super) fn request_recording_save(&mut self, json: Result<String, mhf_ai_debug::Error>) {
        match json {
            Ok(json) => {
                self.recording_save = Some(json);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(in super::super) fn import_dialog(&mut self, context: &egui::Context) {
        if !self.import_open {
            return;
        }
        let mut open = true;
        egui::Window::new("导入录制")
            .id(egui::Id::new("ai-recording-import"))
            .open(&mut open)
            .default_width(480.0)
            .show(context, |ui| {
                ui.label("录制文件路径");
                ui.add(
                    egui::TextEdit::singleline(&mut self.import_path)
                        .desired_width(f32::INFINITY)
                        .hint_text("C:\\recordings\\ai.json"),
                );
                if ui
                    .add_enabled(!self.import_path.trim().is_empty(), Button::new("打开文件"))
                    .clicked()
                {
                    let result = (|| -> Result<Recording, String> {
                        use std::io::Read;
                        let file = std::fs::File::open(self.import_path.trim())
                            .map_err(|error| error.to_string())?;
                        let mut json = String::new();
                        file.take(mhf_ai_debug::MAX_RECORDING_BYTES as u64 + 1)
                            .read_to_string(&mut json)
                            .map_err(|error| error.to_string())?;
                        Recording::from_json(&json).map_err(|error| error.to_string())
                    })();
                    match result {
                        Ok(recording) => {
                            self.load_recording(recording);
                            if self.error.is_none() {
                                self.import_open = false;
                                self.replay_requested = true;
                            }
                        }
                        Err(error) => self.error = Some(format!("打开失败：{error}")),
                    }
                }
                ui.separator();
                ui.label("粘贴录制 JSON");
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.import_json)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(8)
                                .char_limit(mhf_ai_debug::MAX_RECORDING_BYTES),
                        );
                    });
                if ui
                    .add_enabled(
                        !self.import_json.trim().is_empty(),
                        Button::new("验证并导入"),
                    )
                    .clicked()
                {
                    match Recording::from_json(&self.import_json) {
                        Ok(recording) => {
                            self.load_recording(recording);
                            if self.error.is_none() {
                                self.import_open = false;
                                self.replay_requested = true;
                            }
                        }
                        Err(error) => self.error = Some(format!("录制导入失败：{error}")),
                    }
                }
                self.show_error(ui);
            });
        self.import_open &= open;
    }

    pub(super) fn load_recording(&mut self, recording: Recording) {
        match ReplaySession::new(recording) {
            Ok(replay) => {
                self.selected_event = replay
                    .recording()
                    .entries
                    .first()
                    .map(|entry| entry.sequence);
                self.follow_latest = false;
                self.replay = Some(replay);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("录制校验失败：{error}")),
        }
    }
}
