use crate::provider::{AiOperation, AiTarget, DebugCommand, DebugControl, DebugSnapshot, monsters};
use egui_hunter::{Button, ButtonKind, NoticeKind, Notifications, Popup, Tokens};

pub(super) struct Editor {
    selected: Option<AiTarget>,
    show_status: bool,
    draft: Draft,
    request: u64,
    pending: Option<u64>,
    detached: bool,
    drafts: Vec<(AiTarget, Draft)>,
    error: Option<String>,
    notifications: Notifications,
    replacement_species: Option<u8>,
    replacing: bool,
    species_filter: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            selected: None,
            show_status: false,
            draft: Draft::default(),
            request: 0,
            pending: None,
            detached: false,
            drafts: Vec::new(),
            error: None,
            notifications: Notifications::with_capacity(egui::Id::new("monster-ai-errors"), 1),
            replacement_species: None,
            replacing: false,
            species_filter: String::new(),
        }
    }
}

#[derive(Default)]
struct Draft {
    loaded: Option<(AiTarget, u32)>,
    source: String,
    message: String,
    project: Option<mhf_monster::ai::dsl::Project>,
    file: usize,
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
    }

    fn select_file(&mut self, selected: usize) {
        if selected == self.file {
            return;
        }
        let project = self.project.as_mut().unwrap();
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
            ui.label("AI 编辑器已在独立窗口打开。");
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
        let window = egui::Window::new("怪物 · 编辑器")
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
        if let Some(reply) = &snapshot.ai_reply
            && self.pending == Some(reply.request)
        {
            self.pending = None;
            match &reply.result {
                Ok(document) => {
                    if self.replacing {
                        self.selected = None;
                        self.draft = Draft::default();
                        self.drafts.clear();
                        self.replacement_species = None;
                    } else {
                        self.draft.loaded = Some((reply.target, document.descriptor));
                    }
                    if let Some(source) = &document.source {
                        self.draft.set_project(source.clone());
                    }
                    self.draft.message.clone_from(&document.message);
                }
                Err(error) => self.error = Some(error.clone()),
            }
            self.replacing = false;
        }
        let previous = self.selected;
        if self.selected.is_none() && snapshot.ready {
            self.selected = snapshot.ai_targets.first().copied();
        }
        ui.add_enabled_ui(snapshot.ready && self.pending.is_none(), |ui| {
            egui::ComboBox::from_id_salt("ai-target")
                .selected_text(
                    self.selected
                        .map(label)
                        .unwrap_or_else(|| "任务中没有已加载的怪物".into()),
                )
                .show_ui(ui, |ui| {
                    for &target in &snapshot.ai_targets {
                        ui.selectable_value(&mut self.selected, Some(target), label(target));
                    }
                });
        });
        if self.selected != previous {
            self.switch_target(previous, control);
        }
        ui.checkbox(&mut self.show_status, "悬浮显示怪物状态");
        let active = snapshot.ready
            && self
                .selected
                .is_some_and(|target| snapshot.ai_targets.contains(&target));
        if !active && self.selected.is_some() {
            ui.weak("原实例已卸载或任务尚未就绪；草稿保留，请重新选择怪物。");
            // A task transition can discard a reply while the UI was hidden.
            self.pending = None;
        }
        ui.add_enabled_ui(active && self.pending.is_none(), |ui| {
            self.species_picker(ui, snapshot, control);
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong("AI 脚本");
            if ui
                .add(
                    Button::new(if fill_height {
                        "返回面板编辑"
                    } else {
                        "独立窗口编辑"
                    })
                    .kind(ButtonKind::Quiet),
                )
                .clicked()
            {
                self.detached = !fill_height;
            }
            if !self.draft.message.is_empty() && self.error.is_none() {
                let result = ui.add(Button::new("查看结果").kind(ButtonKind::Quiet));
                let mut popup = Popup::new(&result)
                    .title("操作结果")
                    .style(ui.style().clone())
                    .tokens(Tokens::get(ui));
                popup.native = popup.native.width(420.0);
                popup.show(|ui| {
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(&self.draft.message)
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            let editable = active
                && self.pending.is_none()
                && self.draft.project.is_some()
                && self
                    .draft
                    .loaded
                    .is_some_and(|(target, _)| Some(target) == self.selected);
            if ui
                .add_enabled(editable, Button::new("应用热替换"))
                .clicked()
            {
                let (_, descriptor) = self.draft.loaded.unwrap();
                let source = self.draft.project_snapshot().unwrap();
                self.submit(control, AiOperation::Apply { descriptor, source });
            }
            if ui
                .add_enabled(active && self.pending.is_none(), Button::new("重新反编译"))
                .on_hover_text("重新反编译当前怪物内存中的 AI，覆盖编辑草稿。")
                .clicked()
            {
                self.submit(control, AiOperation::Inspect);
            }
            if ui
                .add_enabled(editable, Button::new("恢复替换前 AI"))
                .clicked()
            {
                let (_, descriptor) = self.draft.loaded.unwrap();
                self.submit(control, AiOperation::Restore { descriptor });
            }
        });
        if self.pending.is_some() {
            ui.label("正在等待游戏线程…");
        }
        if self.error.is_some() {
            self.draft.message.clear();
        }
        let mut selected_file = self.draft.file;
        ui.horizontal_wrapped(|ui| {
            if let Some(project) = &self.draft.project {
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
            if ui
                .add_enabled(active && self.pending.is_none(), Button::new("加载工程"))
                .on_hover_text("从磁盘加载工程并覆盖编辑草稿，不会立即应用到怪物。")
                .clicked()
            {
                self.submit(control, AiOperation::Load);
            }
            if ui
                .add_enabled(!self.draft.source.is_empty(), Button::new("复制 DSL"))
                .clicked()
            {
                ui.ctx().copy_text(self.draft.source.clone());
            }
        });
        if self.draft.project.is_some() {
            self.draft.select_file(selected_file);
            egui::ScrollArea::both()
                .id_salt("ai-source-scroll")
                .max_height(
                    ui.available_height()
                        .max(if fill_height { 80.0 } else { 160.0 }),
                )
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_enabled(
                        self.pending.is_none(),
                        egui::TextEdit::multiline(&mut self.draft.source)
                            .id_salt("ai-source")
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(if fill_height { 1 } else { 18 }),
                    );
                });
        }
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

    fn switch_target(&mut self, previous: Option<AiTarget>, control: &DebugControl) {
        self.replacement_species = None;
        if let Some(target) = previous {
            self.drafts.push((target, std::mem::take(&mut self.draft)));
        }
        if let Some(index) = self
            .drafts
            .iter()
            .position(|(target, _)| Some(*target) == self.selected)
        {
            let (_, draft) = self.drafts.swap_remove(index);
            self.draft = draft;
        } else {
            self.submit(control, AiOperation::Inspect);
        }
    }

    fn submit(&mut self, control: &DebugControl, operation: AiOperation) {
        let Some(target) = self.selected else {
            return;
        };
        self.request = self.request.wrapping_add(1);
        let replacing = matches!(operation, AiOperation::ReplaceSpecies(_));
        match control.send(DebugCommand::MonsterAi {
            request: self.request,
            target,
            operation,
        }) {
            Ok(()) => {
                self.pending = Some(self.request);
                self.replacing = replacing;
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
mod tests {
    use super::*;

    #[test]
    fn status_hud_tracks_exact_instance_and_does_not_capture_pointer() {
        let target = AiTarget {
            epoch: 1,
            pool: 0x1000,
            slot: 2,
            serial: 7,
            model: 0,
            species: 6,
        };
        let mut editor = Editor {
            selected: Some(target),
            show_status: true,
            ..Default::default()
        };
        let mut snapshot = DebugSnapshot {
            ready: true,
            monster_statuses: vec![crate::provider::MonsterStatus {
                target,
                ai_state: 12,
                action_group: 3,
                action_id: 4,
                action_stage: 1,
                animation: 9,
                frame: 8.5,
                position: [1.0, 2.0, 3.0],
            }],
            ..Default::default()
        };
        let context = egui::Context::default();
        let draw = |editor: &Editor, snapshot: &DebugSnapshot| {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events: vec![egui::Event::PointerMoved(egui::pos2(770.0, 570.0))],
                    ..Default::default()
                },
                |ui| editor.show_hud(ui.ctx(), snapshot),
            );
            let texts = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            output.drop_without_applying_deltas();
            assert!(!context.egui_wants_pointer_input());
            texts
        };
        for _ in 0..3 {
            draw(&editor, &snapshot);
        }
        assert!(draw(&editor, &snapshot).contains("AI 主状态 12"));
        snapshot.monster_statuses[0].ai_state = 13;
        assert!(draw(&editor, &snapshot).contains("AI 主状态 13"));
        snapshot.monster_statuses[0].target.serial += 1;
        let texts = draw(&editor, &snapshot);
        assert!(texts.contains("目标已卸载"));
        assert!(!texts.contains("AI 主状态"));
        editor.show_status = false;
        assert!(!draw(&editor, &snapshot).contains("AI 主状态"));
    }

    #[test]
    fn file_switches_and_apply_snapshot_preserve_all_edited_files() {
        use mhf_monster::ai::dsl::{Project, SourceFile};
        let mut project = Project::single(Some(31), 6, "entry".into());
        project.files.push(SourceFile {
            path: "common/6/combat.mhai".into(),
            source: "helper".into(),
        });
        let mut draft = Draft::default();
        draft.set_project(project);
        draft.source = "edited entry".into();
        draft.select_file(1);
        assert_eq!(draft.source, "helper");
        draft.source = "edited helper".into();
        let snapshot = draft.project_snapshot().unwrap();
        assert_eq!(snapshot.files[0].source, "edited entry");
        assert_eq!(snapshot.files[1].source, "edited helper");
        draft.set_project(snapshot);
        assert_eq!(draft.file, 1);
        draft.select_file(0);
        assert_eq!(draft.source, "edited entry");
        draft.select_file(1);
        assert_eq!(draft.source, "edited helper");
    }

    #[test]
    fn switching_instances_restores_their_own_drafts_and_descriptors() {
        let first = AiTarget {
            epoch: 1,
            pool: 0x1000,
            slot: 0,
            serial: 1,
            model: 0,
            species: 6,
        };
        let second = AiTarget { slot: 1, ..first };
        let control = DebugControl::new();
        let mut editor = Editor {
            selected: Some(second),
            draft: Draft {
                loaded: Some((first, 0x2000)),
                source: "unsaved first draft".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        editor.switch_target(Some(first), &control);
        assert!(
            matches!(control.commands().as_slice(), [DebugCommand::MonsterAi { target, operation: AiOperation::Inspect, .. }] if *target == second)
        );
        assert!(editor.draft.source.is_empty());
        assert!(editor.draft.loaded.is_none());
        editor.pending = None;
        editor.draft.loaded = Some((second, 0x3000));
        editor.draft.source = "second draft".into();
        editor.selected = Some(first);
        editor.switch_target(Some(second), &control);
        assert_eq!(editor.draft.source, "unsaved first draft");
        assert_eq!(editor.draft.loaded, Some((first, 0x2000)));
        assert!(control.commands().is_empty());
        editor.selected = Some(second);
        editor.switch_target(Some(first), &control);
        assert_eq!(editor.draft.source, "second draft");
        assert_eq!(editor.draft.loaded, Some((second, 0x3000)));
        assert!(control.commands().is_empty());
    }
}
