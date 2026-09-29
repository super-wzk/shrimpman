use crate::provider::{
    AiDebugOperation, AiOperation, AiTarget, DebugCommand, DebugControl, DebugSnapshot, monsters,
};
use egui_hunter::{Button, ButtonKind, NoticeKind, Notifications, Popup, Tokens};

mod debugger;
mod workspace;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Page {
    #[default]
    Live,
    Replay,
}

pub(super) struct Editor {
    selected: Option<AiTarget>,
    page: Page,
    debugger: debugger::DebuggerUi,
    inspector: usize,
    show_inspector: bool,
    show_trace: bool,
    trace_height: f32,
    reveal_line: Option<usize>,
    last_stop: Option<(AiTarget, mhf_ai_debug::ProgramLocation)>,
    show_status: bool,
    draft: Draft,
    request: u64,
    pending: Option<Pending>,
    replacement: Option<(AiTarget, u8)>,
    detached: bool,
    drafts: Vec<(Option<AiTarget>, Draft)>,
    error: Option<String>,
    notifications: Notifications,
    replacement_species: Option<u8>,
    manage_monster: bool,
    species_filter: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            selected: None,
            page: Page::default(),
            debugger: debugger::DebuggerUi::new(),
            inspector: 0,
            show_inspector: false,
            show_trace: true,
            trace_height: 160.0,
            reveal_line: None,
            last_stop: None,
            show_status: false,
            draft: Draft::default(),
            request: 0,
            pending: None,
            replacement: None,
            detached: false,
            drafts: Vec::new(),
            error: None,
            notifications: Notifications::with_capacity(egui::Id::new("monster-ai-errors"), 1),
            replacement_species: None,
            manage_monster: false,
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
    message: String,
    project: Option<mhf_monster::ai::dsl::Project>,
    file: usize,
    modified: bool,
}

impl Draft {
    fn set_project(&mut self, project: mhf_monster::ai::dsl::Project) {
        let previous = self
            .project
            .as_ref()
            .and_then(|p| p.files.get(self.file))
            .map(|f| &f.path);
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
                .and_then(|project| project.files.get(self.file))
                .map_or(!self.source.is_empty(), |file| file.source != self.source)
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

impl Editor {
    pub(super) fn show_hud(&self, context: &egui::Context, snapshot: &DebugSnapshot) {
        if !self.show_status {
            return;
        }
        let Some(target) = self.selected else {
            return;
        };
        let status = snapshot
            .ready
            .then(|| {
                snapshot
                    .monster_statuses
                    .iter()
                    .find(|status| status.target == target)
            })
            .flatten();
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

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        if self.detached {
            ui.label("AI 调试器已在独立窗口打开。");
            if ui.add(Button::new("返回面板编辑")).clicked() {
                self.detached = false;
            } else {
                return;
            }
        }
        self.content(ui, snapshot, control, false);
    }

    pub(super) fn show_window(
        &mut self,
        context: &egui::Context,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) -> Option<egui::Rect> {
        if !self.detached {
            return None;
        }
        let viewport = context.content_rect().shrink(8.0);
        let mut open = true;
        let window = egui::Window::new("怪物 · AI 调试器")
            .id(egui::Id::new("debug-monster-ai-editor"))
            .open(&mut open)
            .resizable(true)
            .default_pos(viewport.min + egui::vec2(24.0, 24.0))
            .default_size(egui::vec2(900.0, 640.0).min(viewport.size()))
            .min_size(egui::vec2(300.0, 240.0).min(viewport.size()))
            .max_size(viewport.size())
            .constrain_to(viewport)
            .vscroll(viewport.height() < 400.0)
            .show(context, |ui| self.content(ui, snapshot, control, true));
        self.detached &= open;
        window.map(|window| window.response.rect)
    }

    fn content(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        fill_height: bool,
    ) {
        self.sync_target(snapshot, control);
        let previous_page = self.page;
        let mut selected = self.selected;
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.spacing_mut().interact_size.y = 24.0;
        let compact = ui.available_width() < 900.0;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("ai-session-mode")
                .width(62.0)
                .selected_text(match self.page {
                    Page::Live => "现场",
                    Page::Replay => "回放",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.page, Page::Live, "现场");
                    ui.selectable_value(&mut self.page, Page::Replay, "回放");
                });
            if self.page == Page::Live {
                ui.add_enabled_ui(snapshot.ready, |ui| {
                    egui::ComboBox::from_id_salt("ai-target")
                        .width(140.0)
                        .selected_text(
                            self.selected
                                .map(label)
                                .unwrap_or_else(|| "选择怪物".into()),
                        )
                        .show_ui(ui, |ui| {
                            for &target in &snapshot.ai_targets {
                                ui.selectable_value(&mut selected, Some(target), label(target));
                            }
                        });
                });
            }
            if selected != self.selected {
                self.select_target(selected, snapshot, control);
            }
            if !compact {
                self.run_toolbar(ui, snapshot, control);
            }
            ui.menu_button("更多", |ui| {
                self.more_menu(ui, snapshot, control, fill_height)
            });
        });
        if compact {
            egui::ScrollArea::horizontal()
                .id_salt("ai-run-toolbar")
                .show(ui, |ui| {
                    ui.horizontal(|ui| self.run_toolbar(ui, snapshot, control));
                });
        }
        ui.separator();
        let active = snapshot.ready
            && self
                .selected
                .is_some_and(|target| snapshot.ai_targets.contains(&target));
        if self.page != previous_page {
            self.debugger.set_follow(self.page != Page::Replay);
            self.inspector = 0;
        }
        if self.debugger.take_replay_request() {
            self.page = Page::Replay;
        }
        ui.push_id(self.page, |ui| {
            self.workspace(ui, snapshot, control, active)
        });
        self.debugger.import_dialog(ui.ctx());
        if self.manage_monster {
            let mut open = true;
            egui::Window::new("怪物管理")
                .id(egui::Id::new("ai-monster-management"))
                .open(&mut open)
                .default_width(340.0)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.add_enabled_ui(active && self.pending.is_none(), |ui| {
                        if let Some(target) = self.selected {
                            ui.label(label(target));
                        }
                        self.species_picker(ui, snapshot, control)
                    });
                });
            self.manage_monster = open
                && !self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.replacement.is_some());
        }
        if self.pending.is_some() {
            ui.weak("正在等待游戏线程…");
        }
        if let Some(error) = self.error.take() {
            self.draft.message.clear();
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
            if !ui.memory(|memory| memory.has_focus(egui::Id::new("ai-source")))
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
                    egui::ComboBox::from_id_salt("ai-source-file")
                        .width((ui.available_width() - 180.0).clamp(120.0, 360.0))
                        .selected_text(project.files[self.draft.file].path.as_str())
                        .show_ui(ui, |ui| {
                            for (index, file) in project.files.iter().enumerate() {
                                ui.selectable_value(&mut selected_file, index, &file.path);
                            }
                        });
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
                    Button::new("定位执行").kind(ButtonKind::Quiet),
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
        egui::ScrollArea::both()
            .id_salt("ai-source-scroll")
            .max_height(ui.available_height().max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.source_editor(ui, markers.as_ref(), mapped, control);
            });
    }

    fn source_editor(
        &mut self,
        ui: &mut egui::Ui,
        markers: Option<&SourceMarkers>,
        debug: Option<&crate::provider::AiDebugSnapshot>,
        control: &DebugControl,
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
            let output = ui
                .add_enabled_ui(self.pending.is_none(), |ui| {
                    egui::TextEdit::multiline(&mut self.draft.source)
                        .id(egui::Id::new("ai-source"))
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(1)
                        .show(ui)
                })
                .inner;

            self.draft.modified |= output.response.changed();

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
                        ui.painter().rect_filled(
                            row_rect,
                            0.0,
                            ui.visuals().selection.bg_fill.gamma_multiply(0.22),
                        );
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

    fn species_picker(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label("种类");
            let selected = self
                .replacement_species
                .or(self.selected.map(|target| target.species));
            let name = selected
                .and_then(|species| monsters::NAMES.get(usize::from(species)))
                .copied()
                .unwrap_or("选择种类");
            let picker = ui.add_sized(
                [
                    180.0_f32.min(ui.available_width()),
                    ui.spacing().interact_size.y,
                ],
                Button::new(&format!("{name} ▾")).id(egui::Id::new("replacement-species")),
            );
            let enabled = ui.is_enabled();
            // Use viewport space, not the popup's previous measured height:
            // a short search result must not cap the next unfiltered list.
            let screen = ui.ctx().content_rect();
            let space =
                (screen.bottom() - picker.rect.bottom()).max(picker.rect.top() - screen.top());
            let list_height = (space - ui.spacing().interact_size.y - 64.0).clamp(60.0, 240.0);
            let mut popup = Popup::new(&picker)
                .style(ui.style().clone())
                .tokens(Tokens::get(ui));
            popup.native = popup.native.width(picker.rect.width().max(220.0));
            popup.show(|ui| {
                if !enabled {
                    ui.close();
                    return;
                }
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.species_filter)
                        .hint_text("搜索名称或编号")
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                let mut list = egui::ScrollArea::vertical()
                    .id_salt("replacement-species-list")
                    .max_height(list_height)
                    .min_scrolled_height(list_height)
                    .auto_shrink([false, true]);
                if search.changed() {
                    list = list.vertical_scroll_offset(0.0);
                }
                list.show(ui, |ui| {
                    let filter = self.species_filter.trim();
                    let mut found = false;
                    for monster in &snapshot.catalog.monsters {
                        if filter.is_empty()
                            || monster.name.contains(filter)
                            || monster.id.to_string().contains(filter)
                        {
                            found = true;
                            if ui
                                .selectable_value(
                                    &mut self.replacement_species,
                                    Some(monster.id),
                                    format!("{} · {}", monster.name, monster.id),
                                )
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    }
                    if !found {
                        ui.weak("没有匹配的种类");
                    }
                });
            });
            let selected = self.replacement_species.or(selected);
            if ui
                .add_enabled(
                    selected.is_some_and(|species| {
                        self.selected
                            .is_some_and(|target| target.species != species)
                    }),
                    Button::new("替换并重载任务"),
                )
                .on_hover_text(
                    "仅替换选中目标的种类。重载任务以加载模型和 AI，任务进度会重置，目标条件不变。",
                )
                .clicked()
            {
                self.submit(control, AiOperation::ReplaceSpecies(selected.unwrap()));
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
                        self.draft.message.clone_from(&document.message);
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
