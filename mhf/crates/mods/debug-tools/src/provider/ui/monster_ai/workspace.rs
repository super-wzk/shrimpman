use super::{Editor, Page, debugger};
use crate::provider::{AiOperation, DebugControl, DebugSnapshot};
use egui_hunter::Button;

impl Editor {
    pub(super) fn run_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        if self.page == Page::Live {
            let active = snapshot.ready
                && self
                    .selected
                    .is_some_and(|target| snapshot.ai_targets.contains(&target));
            self.debugger.controls(
                ui,
                control,
                self.selected,
                active && self.pending.is_none() && self.replacement.is_none(),
                debugger::session(snapshot, self.selected),
            );
            let editable = active
                && self.replacement.is_none()
                && self.pending.is_none()
                && self.draft.project.is_some()
                && self
                    .draft
                    .loaded
                    .is_some_and(|(target, _)| Some(target) == self.selected);
            if ui.add_enabled(editable, Button::new("应用更改")).clicked() {
                let (_, descriptor) = self.draft.loaded.unwrap();
                let source = self.draft.project_snapshot().unwrap();
                self.submit(control, AiOperation::Apply { descriptor, source });
            }
        } else {
            self.debugger.replay_controls(ui);
        }
    }

    pub(super) fn more_menu(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        fill_height: bool,
    ) {
        ui.checkbox(&mut self.show_status, "状态 HUD");
        if ui
            .button(if fill_height {
                "返回面板"
            } else {
                "独立窗口"
            })
            .clicked()
        {
            self.detached = !fill_height;
            ui.close();
        }
        let active = snapshot.ready
            && self.replacement.is_none()
            && self
                .selected
                .is_some_and(|target| snapshot.ai_targets.contains(&target));
        ui.separator();
        if self.page == Page::Live {
            ui.add_enabled_ui(active && self.pending.is_none(), |ui| {
                if ui
                    .button("重新反编译")
                    .on_hover_text("覆盖当前编辑草稿")
                    .clicked()
                {
                    self.submit(control, AiOperation::Inspect);
                    ui.close();
                }
                if ui
                    .button("加载工程")
                    .on_hover_text("从磁盘加载草稿，不会立即应用")
                    .clicked()
                {
                    self.submit(control, AiOperation::Load);
                    ui.close();
                }
                if let Some((target, descriptor)) = self.draft.loaded
                    && Some(target) == self.selected
                    && ui.button("恢复替换前 AI").clicked()
                {
                    self.submit(control, AiOperation::Restore { descriptor });
                    ui.close();
                }
            });
        }
        if ui.button("复制 DSL").clicked() {
            ui.ctx().copy_text(self.draft.source.clone());
            ui.close();
        }
        if self.page == Page::Live {
            self.debugger.recording_actions(
                ui,
                control,
                self.selected,
                active,
                debugger::session(snapshot, self.selected),
            );
        } else {
            self.debugger.replay_actions(ui, snapshot, self.selected);
        }
        ui.separator();
        if self.page == Page::Live && ui.button("怪物管理").clicked() {
            self.manage_monster = true;
            ui.close();
        }
        if !self.draft.message.is_empty() {
            ui.separator();
            ui.set_max_width(360.0);
            ui.label(&self.draft.message);
        }
    }

    pub(super) fn workspace(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        let wide = ui.available_width() >= 720.0;
        let height = ui
            .available_height()
            .min(ui.ctx().content_rect().height() - 140.0)
            .max(240.0);
        let trace_height = if self.show_trace {
            self.trace_height.clamp(80.0, (height * 0.6).max(80.0))
        } else {
            0.0
        };
        let main_height = (height - trace_height - 36.0).max(120.0);
        ui.horizontal(|ui| {
            if !wide {
                ui.selectable_value(&mut self.show_inspector, false, "源码");
                ui.selectable_value(&mut self.show_inspector, true, "检查");
            }
            ui.checkbox(&mut self.show_trace, "轨迹");
            if self.page == Page::Replay {
                ui.weak("录制状态 · 只读");
            } else if let Some(debug) = debugger::session(snapshot, self.selected) {
                ui.weak(if !debug.attached {
                    "已分离"
                } else if debug.paused {
                    "AI 暂停 · 世界继续"
                } else {
                    "AI 运行中"
                });
            } else {
                ui.weak(if active {
                    "未附加"
                } else {
                    "实例不可用"
                });
            }
        });
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), main_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_height(main_height);
                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                if wide {
                    ui.horizontal_top(|ui| {
                        let source_width = (ui.available_width() - 292.0).max(200.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(source_width, main_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_max_width(source_width);
                                self.workspace_source(ui, snapshot, control, active);
                            },
                        );
                        ui.separator();
                        ui.allocate_ui_with_layout(
                            egui::vec2(280.0, main_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                self.inspector(ui, snapshot, control, active);
                            },
                        );
                    });
                } else if self.show_inspector {
                    self.inspector(ui, snapshot, control, active);
                } else {
                    self.workspace_source(ui, snapshot, control, active);
                }
            },
        );
        if self.show_trace {
            let (rect, drag) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::drag());
            ui.painter().hline(
                rect.x_range(),
                rect.center().y,
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            drag.clone()
                .on_hover_cursor(egui::CursorIcon::ResizeVertical);
            if drag.dragged() {
                self.trace_height = (trace_height - ui.input(|i| i.pointer.delta().y))
                    .clamp(80.0, (height * 0.6).max(80.0));
            }
            egui::ScrollArea::vertical()
                .id_salt("ai-bottom-trace")
                .max_height(trace_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.page == Page::Live {
                        self.debugger.trace_panel(ui, snapshot, self.selected);
                    } else {
                        self.debugger.replay_trace(ui);
                    }
                });
        }
        self.debugger.show_workspace_error(ui);
    }

    fn inspector(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        ui.horizontal(|ui| {
            for (index, name) in ["状态", "指令", "断点"].iter().enumerate() {
                if index != 2 || self.page == Page::Live {
                    ui.selectable_value(&mut self.inspector, index, *name);
                }
            }
        });
        ui.separator();
        egui::ScrollArea::both()
            .id_salt("ai-inspector-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.page != Page::Live {
                    self.debugger.replay_inspector(ui, self.inspector == 1);
                    return;
                }
                match self.inspector {
                    2 => self
                        .debugger
                        .breakpoints(ui, snapshot, control, self.selected, active),
                    _ => self.debugger.inspect(
                        ui,
                        snapshot,
                        self.selected,
                        self.inspector == 1,
                        control,
                        active,
                    ),
                }
            });
    }

    fn workspace_source(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        if self.page == Page::Live
            && (self.replacement.is_some()
                || self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.replacement.is_some()))
        {
            ui.weak("等待替换后的怪物加载…");
        } else if self.page == Page::Replay {
            self.debugger
                .recorded_source(ui, snapshot, self.selected, true);
        } else {
            self.source_page(ui, snapshot, control, active);
        }
    }
}
