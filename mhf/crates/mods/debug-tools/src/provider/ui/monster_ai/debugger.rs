use crate::provider::{
    AiDebugOperation, AiDebugSnapshot, AiTarget, DebugCommand, DebugControl, DebugSnapshot,
};
use egui_hunter::{ButtonKind, Icon, IconButton};
use mhf_ai_debug::ReplaySession;

mod breakpoints;
mod format;
mod replay;
mod trace;

#[derive(Default)]
pub(super) struct DebuggerUi {
    selected_event: Option<u64>,
    follow_latest: bool,
    error: Option<String>,
    import_json: String,
    import_open: bool,
    import_path: String,
    recording_save: Option<String>,
    replay: Option<ReplaySession>,
    replay_requested: bool,
    breakpoint_kind: usize,
    breakpoint_script: String,
    breakpoint_offset: String,
    breakpoint_opcode: String,
    breakpoint_field: String,
    breakpoint_value: String,
    breakpoint_condition: bool,
}

pub(super) fn session(
    snapshot: &DebugSnapshot,
    target: Option<AiTarget>,
) -> Option<&AiDebugSnapshot> {
    snapshot
        .ai_debug
        .as_deref()
        .filter(|debug| Some(debug.target) == target)
}

pub(super) fn send(
    control: &DebugControl,
    target: AiTarget,
    operation: AiDebugOperation,
) -> Result<(), String> {
    control.send(DebugCommand::AiDebug { target, operation })
}

impl DebuggerUi {
    pub(super) fn new() -> Self {
        Self {
            follow_latest: true,
            ..Default::default()
        }
    }

    pub(super) fn replay_inspector(&mut self, ui: &mut egui::Ui, instruction: bool) {
        if let Some(replay) = &self.replay {
            if instruction {
                self.selected_detail(ui, replay.recording());
            } else {
                format::fields(ui, replay.snapshot());
            }
        } else {
            ui.weak("尚未载入录制");
        }
    }

    pub(super) fn replay_trace(&mut self, ui: &mut egui::Ui) {
        if let Some(replay) = self.replay.as_mut() {
            let mut position = replay.position();
            if ui
                .add(
                    egui::Slider::new(&mut position, 0..=replay.recording().entries.len())
                        .text("已执行指令"),
                )
                .changed()
                && let Err(error) = replay.seek(position)
            {
                self.error = Some(error.to_string());
            }
            self.selected_event = replay.current_entry().map(|entry| entry.sequence);
            trace::header(ui, replay.recording());
            let previous = self.selected_event;
            trace::list(
                ui,
                replay.recording(),
                &mut self.selected_event,
                &mut self.follow_latest,
            );
            if previous != self.selected_event
                && let Some(index) = replay
                    .recording()
                    .entries
                    .iter()
                    .position(|entry| Some(entry.sequence) == self.selected_event)
                && let Err(error) = replay.seek(index)
            {
                self.error = Some(error.to_string());
            }
        }
    }

    pub(super) fn recorded_source(
        &self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        target: Option<AiTarget>,
        replay: bool,
    ) {
        if replay {
            if let Some(replay) = &self.replay {
                trace::recorded_source(ui, replay.recording(), replay.snapshot().pc);
            } else {
                ui.weak("导入录制后显示录制时的源码。");
            }
        } else if let Some(debug) = session(snapshot, target) {
            let pc = debug
                .recording
                .entries
                .iter()
                .find(|entry| Some(entry.sequence) == self.selected_event)
                .and_then(|entry| entry.pc_before);
            trace::recorded_source(ui, &debug.recording, pc);
        }
    }

    pub(super) fn reset_target(&mut self) {
        self.selected_event = None;
        self.follow_latest = true;
        self.error = None;
        self.breakpoint_script.clear();
        self.breakpoint_offset.clear();
        self.breakpoint_opcode.clear();
        self.breakpoint_field.clear();
        self.breakpoint_value.clear();
        self.breakpoint_condition = false;
    }

    pub(super) fn take_replay_request(&mut self) -> bool {
        std::mem::take(&mut self.replay_requested)
    }

    pub(super) fn take_recording_save(&mut self) -> Option<String> {
        self.recording_save.take()
    }

    pub(super) fn recording_save_finished(&mut self, result: Result<bool, String>) {
        match result {
            Ok(true) => self.error = None,
            Ok(false) => {}
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn follows_live(&self) -> bool {
        self.follow_latest
    }
    pub(super) fn set_follow(&mut self, follow: bool) {
        self.follow_latest = follow;
    }

    pub(super) fn inspect(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        target: Option<AiTarget>,
        instruction: bool,
        control: &DebugControl,
        active: bool,
    ) {
        let Some(debug) = session(snapshot, target) else {
            ui.weak("附加实例后查看状态与指令。");
            return;
        };
        if instruction {
            self.selected_detail(ui, &debug.recording);
        } else {
            if !self.follow_latest {
                ui.weak("现场状态 · 当前帧");
            }
            if !debug.reason.is_empty() {
                ui.label(&debug.reason);
            }
            ui.weak(format!(
                "帧 {} · {}",
                debug.state.frame,
                format::location(debug.state.pc.as_ref())
            ));
            egui::Grid::new("ai-live-fields")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for (field, value) in &debug.state.fields {
                        ui.monospace(field);
                        ui.monospace(value.to_string());
                        let has_field_breakpoint = debug.breakpoints.iter().any(|bp| {
                            matches!(&bp.kind, mhf_ai_debug::BreakpointKind::FieldChanged(name) if name == field)
                        });
                        let (icon, label) = if has_field_breakpoint {
                            (Icon::BreakpointOff, "移除字段断点")
                        } else {
                            (Icon::Breakpoint, "添加字段断点")
                        };
                        if ui
                            .add_enabled(
                                active && debug.attached,
                                IconButton::new(icon, label)
                                .id(egui::Id::new(("ai-field-breakpoint", field)))
                                .kind(ButtonKind::Danger)
                                .selected(has_field_breakpoint),
                            )
                            .on_hover_text("字段发生变化时暂停 AI；再次点击移除字段断点")
                            .clicked()
                        {
                            let mut breakpoints = debug.breakpoints.clone();
                            let kind = mhf_ai_debug::BreakpointKind::FieldChanged(field.clone());
                            if has_field_breakpoint {
                                breakpoints.retain(|bp| bp.kind != kind);
                            } else {
                                breakpoints.push(mhf_ai_debug::Breakpoint {
                                    id: breakpoints
                                        .iter()
                                        .map(|bp| bp.id)
                                        .max()
                                        .unwrap_or(0)
                                        .saturating_add(1),
                                    enabled: true,
                                    kind,
                                    condition: None,
                                });
                            }
                            self.error = send(
                                control,
                                debug.target,
                                AiDebugOperation::SetBreakpoints(breakpoints),
                            )
                            .err();
                        }
                        ui.end_row();
                    }
                });
        }
    }

    pub(super) fn trace_panel(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        target: Option<AiTarget>,
    ) {
        if let Some(debug) = session(snapshot, target) {
            if !debug.recording.entries.is_empty()
                && (self.follow_latest
                    || !debug
                        .recording
                        .entries
                        .iter()
                        .any(|entry| Some(entry.sequence) == self.selected_event))
            {
                self.selected_event = debug.recording.entries.last().map(|entry| entry.sequence);
            }
            trace::header(ui, &debug.recording);
            trace::list(
                ui,
                &debug.recording,
                &mut self.selected_event,
                &mut self.follow_latest,
            );
        } else {
            ui.weak("附加调试器后记录执行轨迹");
        }
    }

    pub(super) fn controls(
        &mut self,
        ui: &mut egui::Ui,
        control: &DebugControl,
        target: Option<AiTarget>,
        active: bool,
        debug: Option<&AiDebugSnapshot>,
    ) {
        let attached = debug.is_some_and(|debug| debug.attached);
        let paused = debug.is_some_and(|debug| debug.paused);
        let mut operation = None;
        if ui
            .add_enabled(
                active,
                IconButton::new(
                    if attached { Icon::Detach } else { Icon::Attach },
                    if attached {
                        "分离调试器"
                    } else {
                        "附加调试器"
                    },
                )
                .id(egui::Id::new("ai-attach"))
                .selected(attached),
            )
            .clicked()
        {
            operation = Some(if attached {
                AiDebugOperation::Detach
            } else {
                AiDebugOperation::Attach
            });
        }
        if ui
            .add_enabled(
                active && attached,
                IconButton::new(
                    if paused { Icon::Play } else { Icon::Pause },
                    if paused { "继续" } else { "暂停" },
                )
                .id(egui::Id::new("ai-pause")),
            )
            .clicked()
        {
            operation = Some(if paused {
                AiDebugOperation::Continue
            } else {
                AiDebugOperation::Pause
            });
        }
        if ui
            .add_enabled(
                active && attached && paused,
                IconButton::new(Icon::Step, "单步").id(egui::Id::new("ai-step")),
            )
            .clicked()
        {
            operation = Some(AiDebugOperation::StepInstruction);
        }
        if ui
            .add_enabled(
                active && attached && paused,
                IconButton::new(Icon::StepOut, "至让出").id(egui::Id::new("ai-run-yield")),
            )
            .clicked()
        {
            operation = Some(AiDebugOperation::RunUntilYield);
        }

        if let Some(operation) = operation
            && let Some(target) = target
            && let Err(error) = send(control, target, operation)
        {
            self.error = Some(error);
        }
    }

    pub(super) fn recording_actions(
        &mut self,
        ui: &mut egui::Ui,
        control: &DebugControl,
        target: Option<AiTarget>,
        active: bool,
        debug: Option<&AiDebugSnapshot>,
    ) {
        if ui
            .add_enabled(
                debug.is_some(),
                IconButton::new(Icon::Save, "保存录制").id(egui::Id::new("ai-recording-save")),
            )
            .clicked()
            && let Some(debug) = debug
        {
            self.request_recording_save(debug.recording.to_json());
        }
        let attached = debug.is_some_and(|debug| debug.attached);
        if ui
            .add_enabled(
                active && attached,
                IconButton::new(Icon::Trash, "清空轨迹")
                    .id(egui::Id::new("ai-trace-clear"))
                    .kind(ButtonKind::Danger),
            )
            .clicked()
            && let Some(target) = target
        {
            self.error = send(control, target, AiDebugOperation::ClearTrace).err();
        }
    }

    pub(super) fn show_workspace_error(&self, ui: &mut egui::Ui) {
        if !self.import_open {
            self.show_error(ui);
        }
    }

    pub(super) fn show_error(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}

#[cfg(test)]
mod tests;
