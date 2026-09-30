use crate::provider::{
    AiDebugOperation, AiOperation, AiTarget, DebugCommand, DebugControl, DebugSnapshot, monsters,
};
use egui_hunter::{Button, ButtonKind, Icon, IconButton, NoticeKind, Notifications, SelectField};

mod debugger;
mod management;
mod source;
mod workspace;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Page {
    #[default]
    Live,
    Replay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum View {
    #[default]
    Source,
    Inspector,
    Trace,
}

pub(super) struct Editor {
    selected: Option<AiTarget>,
    page: Page,
    debugger: debugger::DebuggerUi,
    inspector: usize,
    show_inspector: bool,
    show_trace: bool,
    compact_view: View,
    reveal_line: Option<usize>,
    source_id: Option<egui::Id>,
    last_stop: Option<(AiTarget, mhf_ai_debug::ProgramLocation)>,
    show_status: bool,
    draft: Draft,
    request: u64,
    pending: Option<Pending>,
    replacement: Option<(AiTarget, u8)>,
    drafts: Vec<(Option<AiTarget>, Draft)>,
    error: Option<String>,
    notifications: Notifications,
    replacement_species: Option<u8>,
    species_filter: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            selected: None,
            page: Page::default(),
            debugger: debugger::DebuggerUi::new(),
            inspector: 0,
            show_inspector: true,
            show_trace: true,
            compact_view: View::Source,
            reveal_line: None,
            source_id: None,
            last_stop: None,
            show_status: false,
            draft: Draft::default(),
            request: 0,
            pending: None,
            replacement: None,
            drafts: Vec::new(),
            error: None,
            notifications: Notifications::with_capacity(egui::Id::new("monster-ai-errors"), 1),
            replacement_species: None,
            species_filter: String::new(),
        }
    }
}

struct Pending {
    request: u64,
    target: AiTarget,
    replacement: Option<u8>,
    preserve_draft: bool,
    attach: bool,
}

struct SourceMarkers {
    path: String,
    current_line: Option<usize>,
    breakpoint_lines: Vec<usize>,
    interactive: bool,
}

#[derive(Default)]
struct Draft {
    loaded: Option<(AiTarget, u32)>,
    source: String,
    project: Option<mhf_monster::ai::dsl::Project>,
    file: usize,
    modified: bool,
}

impl Draft {
    fn set_project(&mut self, project: mhf_monster::ai::dsl::Project) {
        let previous = self
            .project
            .as_ref()
            .map(|project| &project.files[self.file].path);
        self.file = project
            .files
            .iter()
            .position(|f| Some(&f.path) == previous)
            .unwrap_or(0);
        self.source = project.files[self.file].source.clone();
        self.project = Some(project);
        self.modified = false;
    }

    fn is_modified(&self) -> bool {
        self.modified
            || self.loaded.is_none() && self.project.is_some()
            || self
                .project
                .as_ref()
                .map_or(!self.source.is_empty(), |project| {
                    project.files[self.file].source != self.source
                })
    }

    fn select_file(&mut self, selected: usize) {
        if selected == self.file {
            return;
        }
        let project = self.project.as_mut().unwrap();
        self.modified |= project.files[self.file].source != self.source;
        project.files[self.file].source = std::mem::take(&mut self.source);
        self.file = selected;
        self.source.clone_from(&project.files[selected].source);
    }

    fn project_snapshot(&self) -> Option<mhf_monster::ai::dsl::Project> {
        let mut project = self.project.clone()?;
        project.files[self.file].source.clone_from(&self.source);
        Some(project)
    }
}

pub(super) fn show_hud(context: &egui::Context, snapshot: &DebugSnapshot, target: AiTarget) {
    let status = if snapshot.ready {
        snapshot
            .monster_statuses
            .iter()
            .find(|status| status.target == target)
    } else {
        None
    };
    egui::Area::new(egui::Id::new("debug-monster-status-hud"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
        .order(egui::Order::Middle)
        .movable(false)
        .interactable(false)
        .show(context, |ui| {
            ui.set_max_width((context.content_rect().width() - 24.0).max(80.0));
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(170))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.visuals_mut().override_text_color = Some(egui::Color32::from_gray(230));
                    ui.strong(label(target));
                    if let Some(status) = status {
                        ui.label(format!("AI 主状态 {}", status.ai_state));
                        ui.label(format!(
                            "动作 {}:{} / {} · 动画 {} · 帧 {:.1}",
                            status.action_group,
                            status.action_id,
                            status.action_stage,
                            status.animation,
                            status.frame
                        ));
                        ui.small(format!(
                            "位置  X {:.1}  Y {:.1}  Z {:.1}",
                            status.position[0], status.position[1], status.position[2]
                        ));
                    } else {
                        ui.label("目标已卸载或任务未就绪，请重新选择怪物。");
                    }
                });
        });
}

impl Editor {
    pub(super) fn hud_target(&self) -> Option<AiTarget> {
        self.selected.filter(|_| self.show_status)
    }

    pub(super) fn take_recording_save(&mut self) -> Option<String> {
        self.debugger.take_recording_save()
    }

    pub(super) fn recording_save_finished(&mut self, result: Result<bool, String>) {
        self.debugger.recording_save_finished(result);
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        self.sync_target(snapshot, control);
        let previous_page = self.page;
        let target_width = (ui.available_width() - 160.0).clamp(100.0, 280.0);
        let active = ui
            .horizontal_wrapped(|ui| {
                let mut mode = SelectField::new(
                    egui::Id::new("ai-session-mode"),
                    if self.page == Page::Live {
                        "现场"
                    } else {
                        "回放"
                    },
                );
                mode.native = mode.native.width(84.0);
                mode.show_ui(ui, |ui| {
                    for (page, label) in [(Page::Live, "现场"), (Page::Replay, "回放")] {
                        if ui
                            .add(
                                Button::new(label)
                                    .kind(ButtonKind::Quiet)
                                    .selected(self.page == page)
                                    .full_width(),
                            )
                            .clicked()
                        {
                            self.page = page;
                            ui.close();
                        }
                    }
                });
                if self.page == Page::Live {
                    self.target_picker(ui, snapshot, control, target_width);
                }
                let active = self.active(snapshot);
                self.run_toolbar(ui, snapshot, control, active);
                active
            })
            .inner;
        ui.separator();
        if self.debugger.take_replay_request() {
            self.page = Page::Replay;
        }
        if self.page != previous_page {
            self.debugger.set_follow(self.page != Page::Replay);
            self.inspector = 0;
        }
        ui.push_id(self.page, |ui| {
            self.workspace(ui, snapshot, control, active)
        });
        self.debugger.import_dialog(ui.ctx());
        self.show_notifications(ui);
    }

    fn active(&self, snapshot: &DebugSnapshot) -> bool {
        snapshot.ready
            && self
                .selected
                .is_some_and(|target| snapshot.ai_targets.contains(&target))
    }

    fn target_picker(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        width: f32,
    ) {
        let mut selected = self.selected;
        ui.add_enabled_ui(snapshot.ready, |ui| {
            let mut targets = SelectField::new(
                egui::Id::new("ai-target"),
                self.selected
                    .map(label)
                    .unwrap_or_else(|| "选择怪物".into()),
            );
            targets.native = targets.native.width(width);
            targets.show_ui(ui, |ui| {
                for &target in &snapshot.ai_targets {
                    if ui
                        .add(
                            Button::new(&label(target))
                                .kind(ButtonKind::Quiet)
                                .selected(selected == Some(target))
                                .full_width(),
                        )
                        .clicked()
                    {
                        selected = Some(target);
                        ui.close();
                    }
                }
            });
        });
        if selected != self.selected {
            self.select_target(selected, snapshot, control);
        }
    }

    fn show_notifications(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = self.error.take() {
            self.notifications.push_for(
                ui.ctx(),
                NoticeKind::Danger,
                format!("操作失败：{error}"),
                std::time::Duration::from_secs(8),
            );
        }
        self.notifications.show_in(ui);
    }

    fn source_page(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        let debug = debugger::session(snapshot, self.selected);
        let execution_mapping = debug.and_then(|debug| {
            let pc = debug.state.pc?;
            debug
                .debug_info
                .lookup(pc.script as usize, pc.offset as usize)
        });
        let following = self.debugger.follows_live();
        if following
            && let Some(debug) = debug.filter(|debug| debug.attached && debug.paused)
            && let Some(pc) = debug.state.pc
            && self.last_stop != Some((debug.target, pc))
        {
            self.last_stop = Some((debug.target, pc));
            if !self
                .source_id
                .is_some_and(|id| ui.memory(|memory| memory.has_focus(id)))
                && let Some(mapping) = execution_mapping
                && let Some(project) = &self.draft.project
                && let Some(index) = project
                    .files
                    .iter()
                    .position(|file| file.path == mapping.source.path)
                && debug.debug_info.files.iter().any(|compiled| {
                    compiled.path == project.files[index].path
                        && compiled.source
                            == if index == self.draft.file {
                                self.draft.source.as_str()
                            } else {
                                project.files[index].source.as_str()
                            }
                })
            {
                self.draft.select_file(index);
                self.reveal_line = Some(mapping.source.line);
            }
        }
        let mut selected_file = self.draft.file;
        let mut locate = false;
        ui.horizontal_wrapped(|ui| {
            if !following {
                ui.weak("历史轨迹");
            } else if let Some(project) = &self.draft.project {
                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                    let mut files = SelectField::new(
                        egui::Id::new("ai-source-file"),
                        project.files[self.draft.file].path.as_str(),
                    );
                    files.native = files
                        .native
                        .width((ui.available_width() - 120.0).clamp(120.0, 400.0));
                    files
                        .show_ui(ui, |ui| {
                            for (index, file) in project.files.iter().enumerate() {
                                if ui
                                    .add(
                                        Button::new(&file.path)
                                            .kind(ButtonKind::Quiet)
                                            .selected(index == selected_file)
                                            .full_width(),
                                    )
                                    .clicked()
                                {
                                    selected_file = index;
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text(project.files[self.draft.file].path.as_str());
                });
            }
            let execution_file = execution_mapping.and_then(|mapping| {
                self.draft
                    .project
                    .as_ref()?
                    .files
                    .iter()
                    .position(|file| file.path == mapping.source.path)
            });
            if ui
                .add_enabled(
                    self.pending.is_none() && (execution_file.is_some() || !following),
                    IconButton::new(Icon::ArrowUpRight, "定位执行")
                        .id(egui::Id::new("ai-locate-execution")),
                )
                .on_hover_text("恢复现场跟随，并定位到当前执行位置；未应用草稿不标记执行行。")
                .clicked()
            {
                locate = true;
                self.debugger.set_follow(true);
                self.last_stop = None;
                if let Some(file) = execution_file {
                    selected_file = file;
                }
            }
        });
        if !self.debugger.follows_live() {
            self.debugger
                .recorded_source(ui, snapshot, self.selected, false);
            return;
        }
        if self.draft.project.is_none() {
            if self.pending.is_none() {
                ui.weak("选择怪物读取脚本；读取失败时可从更多菜单重新反编译或加载工程。");
            }
            return;
        }
        self.draft.select_file(selected_file);
        let file = &self.draft.project.as_ref().unwrap().files[self.draft.file];
        let mapped =
            debug.filter(|debug| {
                debug.debug_info.files.iter().any(|compiled| {
                    compiled.path == file.path && compiled.source == self.draft.source
                })
            });
        if locate && mapped.is_some() {
            self.reveal_line = execution_mapping.map(|mapping| mapping.source.line);
        }
        let markers = mapped.map(|debug| SourceMarkers {
            path: file.path.clone(),
            current_line: execution_mapping
                .filter(|mapping| mapping.source.path == file.path)
                .map(|mapping| mapping.source.line),
            interactive: active && debug.attached,
            breakpoint_lines: debug
                .breakpoints
                .iter()
                .filter(|breakpoint| breakpoint.enabled)
                .filter_map(|breakpoint| {
                    let mhf_ai_debug::BreakpointKind::Location(pc) = breakpoint.kind else {
                        return None;
                    };
                    if debug
                        .state
                        .pc
                        .is_some_and(|current| current.revision != pc.revision)
                    {
                        return None;
                    }
                    debug
                        .debug_info
                        .lookup(pc.script as usize, pc.offset as usize)
                        .filter(|mapping| mapping.source.path == file.path)
                        .map(|mapping| mapping.source.line)
                })
                .collect(),
        });
        if debug.is_some() && mapped.is_none() {
            ui.weak("草稿未应用 · 行断点暂不可用")
                .on_hover_text("草稿与执行版本不同，应用后恢复源码断点和执行行标记。");
        }
        let viewport_height = ui.available_height().max(0.0);
        egui::ScrollArea::vertical()
            .id_salt("ai-source-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.source_editor(ui, markers.as_ref(), mapped, control, viewport_height);
            });
    }

    fn source_editor(
        &mut self,
        ui: &mut egui::Ui,
        markers: Option<&SourceMarkers>,
        debug: Option<&crate::provider::AiDebugSnapshot>,
        control: &DebugControl,
        viewport_height: f32,
    ) {
        ui.horizontal_top(|ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let color = ui.visuals().weak_text_color();
            let line_count = self.draft.source.bytes().filter(|&b| b == b'\n').count() + 1;
            let gutter_width = ui
                .painter()
                .layout_no_wrap(line_count.to_string(), font.clone(), color)
                .size()
                .x
                + 14.0;
            let (gutter, _) =
                ui.allocate_exact_size(egui::vec2(gutter_width, 0.0), egui::Sense::hover());
            let editable = self.pending.is_none();
            let output = ui
                .add_enabled_ui(editable, |ui| {
                    let mut read_only;
                    let text: &mut dyn egui::TextBuffer = if editable {
                        &mut self.draft.source
                    } else {
                        read_only = self.draft.source.as_str();
                        &mut read_only
                    };
                    source::show(ui, "ai-source", text, false, viewport_height)
                })
                .inner;

            self.source_id = Some(output.response.id);
            self.draft.modified |= editable && output.response.changed();

            // Match actual text rows, including margins and font scaling.
            // Wrapped continuation rows do not introduce a source line number.
            let clip = ui.clip_rect();
            let mut line_number = 1;
            let mut starts_line = true;
            for row in &output.galley.rows {
                let rect = row.rect().translate(output.galley_pos.to_vec2());
                if starts_line && self.reveal_line == Some(line_number) {
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    self.reveal_line = None;
                }
                if starts_line && rect.bottom() >= clip.top() && rect.top() <= clip.bottom() {
                    let row_rect = egui::Rect::from_min_max(
                        egui::pos2(gutter.left(), rect.top()),
                        egui::pos2(output.response.rect.right(), rect.bottom()),
                    );
                    if markers.is_some_and(|markers| markers.current_line == Some(line_number)) {
                        source::highlight_line(ui, row_rect);
                        ui.painter().text(
                            egui::pos2(gutter.left(), rect.top()),
                            egui::Align2::LEFT_TOP,
                            "▶",
                            font.clone(),
                            ui.visuals().selection.stroke.color,
                        );
                    }
                    if let Some(markers) = markers {
                        let path = markers.path.as_str();
                        if markers.breakpoint_lines.contains(&line_number) {
                            ui.painter().circle_filled(
                                egui::pos2(gutter.left() + 4.0, rect.center().y),
                                3.5,
                                ui.visuals().error_fg_color,
                            );
                        }
                        let hit = egui::Rect::from_min_max(
                            egui::pos2(gutter.left(), rect.top()),
                            egui::pos2(gutter.right(), rect.bottom()),
                        );
                        let response = ui
                            .interact(
                                hit,
                                egui::Id::new(("ai-source-breakpoint", path, line_number)),
                                if markers.interactive {
                                    egui::Sense::click()
                                } else {
                                    egui::Sense::hover()
                                },
                            )
                            .on_hover_text(if markers.interactive {
                                "点击切换断点，右键编辑条件"
                            } else {
                                "附加调试器后可切换源码行断点"
                            });
                        if markers.interactive
                            && let Some(debug) = debug
                        {
                            response.context_menu(|ui| {
                                self.debugger
                                    .source_context(ui, debug, control, path, line_number)
                            });
                        }
                        if response.clicked()
                            && let Some(target) = self.selected
                            && let Err(error) = debugger::send(
                                control,
                                target,
                                AiDebugOperation::SourceBreakpoint {
                                    path: path.to_owned(),
                                    line: line_number,
                                },
                            )
                        {
                            self.error = Some(error);
                        }
                    }
                    ui.painter().text(
                        egui::pos2(gutter.right(), rect.top()),
                        egui::Align2::RIGHT_TOP,
                        line_number.to_string(),
                        font.clone(),
                        color,
                    );
                }
                starts_line = row.ends_with_newline;
                if starts_line {
                    line_number += 1;
                }
            }
        });
    }

    fn sync_target(&mut self, snapshot: &DebugSnapshot, control: &DebugControl) {
        if let Some(reply) = &snapshot.ai_reply
            && self.pending.as_ref().is_some_and(|pending| {
                pending.request == reply.request && pending.target == reply.target
            })
            && self.selected == Some(reply.target)
        {
            let pending = self.pending.take().unwrap();
            match &reply.result {
                Ok(document) => {
                    if let Some(species) = pending.replacement {
                        // The reply describes the old instance. Wait for this spawn slot,
                        // not the first monster enumerated while the quest loads.
                        self.replacement = Some((reply.target, species));
                    } else {
                        self.draft.loaded = Some((reply.target, document.descriptor));
                        if !pending.preserve_draft
                            && let Some(source) = &document.source
                        {
                            self.draft.set_project(source.clone());
                        }
                        if pending.attach
                            && snapshot.ready
                            && snapshot.ai_targets.contains(&reply.target)
                        {
                            self.error =
                                debugger::send(control, reply.target, AiDebugOperation::Attach)
                                    .err();
                        }
                    }
                }
                Err(error) => {
                    self.replacement = None;
                    self.error = Some(error.clone());
                }
            }
        }
        let replacement = self.replacement.or_else(|| {
            self.pending
                .as_ref()
                .and_then(|pending| pending.replacement.map(|species| (pending.target, species)))
        });
        if snapshot.ready {
            if let Some((previous, species)) = replacement {
                if let Some(target) = snapshot.ai_targets.iter().copied().find(|target| {
                    target.slot == previous.slot && target.species == species && *target != previous
                }) {
                    self.select_target(Some(target), snapshot, control);
                }
            } else if self.selected.is_none()
                && self.page == Page::Live
                && let Some(target) = snapshot.ai_targets.first().copied()
            {
                self.select_target(Some(target), snapshot, control);
            }
        }
        if replacement.is_none()
            && (!snapshot.ready
                || self
                    .selected
                    .is_some_and(|target| !snapshot.ai_targets.contains(&target)))
        {
            self.pending = None;
        }
    }

    fn select_target(
        &mut self,
        selected: Option<AiTarget>,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        let previous = self.selected;
        let debug = debugger::session(snapshot, previous);
        let attach = debug.is_some_and(|debug| debug.attached)
            || self.pending.as_ref().is_some_and(|pending| pending.attach);
        if let Some(debug) = debug.filter(|debug| debug.attached)
            && let Err(error) = debugger::send(control, debug.target, AiDebugOperation::Detach)
        {
            self.error = Some(error);
            return;
        }
        self.drafts
            .push((previous, std::mem::take(&mut self.draft)));
        if let Some(index) = self
            .drafts
            .iter()
            .position(|(target, _)| *target == selected)
        {
            self.draft = self.drafts.swap_remove(index).1;
        }
        self.selected = selected;
        self.pending = None;
        self.replacement = None;
        self.replacement_species = None;
        self.last_stop = None;
        self.reveal_line = None;
        self.debugger.reset_target();
        let preserve_draft = self.draft.is_modified();
        if selected.is_some() {
            self.submit(control, AiOperation::Inspect);
            if let Some(pending) = &mut self.pending {
                pending.preserve_draft = preserve_draft;
                pending.attach = attach;
            }
        }
    }

    fn submit(&mut self, control: &DebugControl, operation: AiOperation) {
        let Some(target) = self.selected else {
            return;
        };
        self.request = self.request.wrapping_add(1);
        let replacement = if let AiOperation::ReplaceSpecies(species) = &operation {
            Some(*species)
        } else {
            None
        };
        match control.send(DebugCommand::MonsterAi {
            request: self.request,
            target,
            operation,
        }) {
            Ok(()) => {
                self.pending = Some(Pending {
                    request: self.request,
                    target,
                    replacement,
                    preserve_draft: false,
                    attach: false,
                });
            }
            Err(error) => self.error = Some(error),
        }
    }
}

fn label(target: AiTarget) -> String {
    let name = monsters::NAMES
        .get(usize::from(target.species))
        .unwrap_or(&"未知怪物");
    format!(
        "#{slot} {name} · 物种 {species}",
        slot = target.slot,
        species = target.species
    )
}

#[cfg(test)]
mod tests;
