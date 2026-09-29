use super::{DebuggerUi, session};
use crate::provider::{AiTarget, DebugSnapshot};
use egui_hunter::Button;
use mhf_ai_debug::{Recording, ReplaySession};

impl DebuggerUi {
    pub(in super::super) fn replay_controls(&mut self, ui: &mut egui::Ui) {
        if let Some(replay) = self.replay.as_mut() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(replay.position() > 0, Button::new("起点"))
                    .clicked()
                    && let Err(error) = replay.seek(0)
                {
                    self.error = Some(error.to_string());
                }
                if ui
                    .add_enabled(replay.position() > 0, Button::new("后退"))
                    .clicked()
                    && let Err(error) = replay.step_back()
                {
                    self.error = Some(error.to_string());
                }
                if ui
                    .add_enabled(
                        replay.position() < replay.recording().entries.len(),
                        Button::new("前进"),
                    )
                    .clicked()
                    && let Err(error) = replay.step_forward()
                {
                    self.error = Some(error.to_string());
                }
                ui.weak(format!(
                    "{} / {}",
                    replay.position(),
                    replay.recording().entries.len()
                ));
            });
            self.selected_event = replay.current_entry().map(|entry| entry.sequence);
        } else {
            ui.weak("从更多菜单载入或导入录制");
        }
    }

    pub(in super::super) fn replay_actions(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        target: Option<AiTarget>,
    ) {
        if ui.button("导入录制…").clicked() {
            self.import_open = true;
            self.export_json = None;
            ui.close();
        }
        if ui
            .add_enabled(
                session(snapshot, target).is_some(),
                Button::new("载入现场轨迹"),
            )
            .clicked()
            && let Some(debug) = session(snapshot, target)
        {
            self.load_recording(debug.recording.clone());
            self.replay_requested = true;
            ui.close();
        }
        if ui
            .add_enabled(self.replay.is_some(), Button::new("保存录制…"))
            .clicked()
            && let Some(replay) = &self.replay
        {
            let json = replay.recording().to_json();
            self.open_export(json);
            ui.close();
        }
    }

    pub(super) fn open_export(&mut self, json: Result<String, mhf_ai_debug::Error>) {
        match json {
            Ok(json) => {
                self.export_json = Some(json);
                self.import_open = true;
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
        egui::Window::new(if self.export_json.is_some() {
            "保存录制"
        } else {
            "导入录制"
        })
        .id(egui::Id::new("ai-recording-import"))
        .open(&mut open)
        .default_width(480.0)
        .show(context, |ui| {
            ui.label("录制文件路径");
            ui.add(
                egui::TextEdit::singleline(&mut self.recording_path)
                    .desired_width(f32::INFINITY)
                    .hint_text("C:\\recordings\\ai.json"),
            );
            if let Some(json) = &self.export_json {
                if ui.button("复制 JSON").clicked() {
                    ui.ctx().copy_text(json.clone());
                }
                if ui
                    .add_enabled(
                        !self.recording_path.trim().is_empty(),
                        Button::new("保存文件"),
                    )
                    .on_hover_text("已有同名文件会被覆盖")
                    .clicked()
                {
                    match std::fs::write(self.recording_path.trim(), json) {
                        Ok(()) => {
                            self.import_open = false;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(format!("保存失败：{error}")),
                    }
                }
                self.show_error(ui);
                return;
            }
            if ui
                .add_enabled(
                    !self.recording_path.trim().is_empty(),
                    Button::new("打开文件"),
                )
                .clicked()
            {
                let result = (|| -> Result<Recording, String> {
                    use std::io::Read;
                    let file = std::fs::File::open(self.recording_path.trim())
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
