use super::{Editor, Page, View, debugger};
use crate::provider::{AiOperation, DebugControl, DebugSnapshot};
use egui_hunter::{
    ButtonKind, Checkbox, Icon, IconButton, NavigationState, SplitPane, Tab, Tabs, Tokens,
};

impl Editor {
    pub(super) fn run_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        if self.page == Page::Live {
            let idle = self.pending.is_none() && self.replacement.is_none();
            let debug = debugger::session(snapshot, self.selected);
            self.debugger
                .controls(ui, control, self.selected, active && idle, debug);
            let editable = active
                && idle
                && self.draft.project.is_some()
                && self
                    .draft
                    .loaded
                    .is_some_and(|(target, _)| Some(target) == self.selected);
            if ui
                .add_enabled(
                    editable,
                    IconButton::new(Icon::Apply, "应用更改")
                        .id(egui::Id::new("ai-apply"))
                        .kind(ButtonKind::Primary),
                )
                .on_hover_text("编译当前工程草稿，并应用到选中的怪物实例。")
                .clicked()
            {
                let (_, descriptor) = self.draft.loaded.unwrap();
                let source = self.draft.project_snapshot().unwrap();
                self.submit(control, AiOperation::Apply { descriptor, source });
            }
            toolbar_separator(ui);
            // Re-evaluate after a command above: an apply starts a pending request.
            let editable = active && self.pending.is_none() && self.replacement.is_none();
            if ui
                .add_enabled(
                    editable,
                    IconButton::new(Icon::Refresh, "重新反编译")
                        .id(egui::Id::new("ai-source-refresh")),
                )
                .on_hover_text("重新读取当前实例的 AI，覆盖编辑草稿。")
                .clicked()
            {
                self.submit(control, AiOperation::Inspect);
            }
            if ui
                .add_enabled(
                    active && self.pending.is_none() && self.replacement.is_none(),
                    IconButton::new(Icon::FolderOpen, "加载工程")
                        .id(egui::Id::new("ai-source-load")),
                )
                .on_hover_text("从磁盘加载工程为草稿，不会立即应用。")
                .clicked()
            {
                self.submit(control, AiOperation::Load);
            }
            let original = self
                .draft
                .loaded
                .filter(|(target, _)| Some(*target) == self.selected);
            if ui
                .add_enabled(
                    active
                        && self.pending.is_none()
                        && self.replacement.is_none()
                        && original.is_some(),
                    IconButton::new(Icon::Undo, "恢复替换前 AI")
                        .id(egui::Id::new("ai-source-restore")),
                )
                .clicked()
            {
                self.submit(
                    control,
                    AiOperation::Restore {
                        descriptor: original.unwrap().1,
                    },
                );
            }
            if ui
                .add_enabled(
                    self.draft.project.is_some(),
                    IconButton::new(Icon::Copy, "复制 DSL").id(egui::Id::new("ai-source-copy")),
                )
                .clicked()
            {
                ui.ctx().copy_text(self.draft.source.clone());
            }
            toolbar_separator(ui);
            self.debugger.recording_actions(
                ui,
                control,
                self.selected,
                active && self.pending.is_none() && self.replacement.is_none(),
                debug,
            );
        } else {
            self.debugger.replay_controls(ui);
            toolbar_separator(ui);
            self.debugger.replay_actions(ui, snapshot, self.selected);
        }
        toolbar_separator(ui);
        if ui
            .add(
                IconButton::new(Icon::Eye, "状态 HUD")
                    .id(egui::Id::new("ai-status-hud"))
                    .selected(self.show_status),
            )
            .clicked()
        {
            self.show_status = !self.show_status;
        }
    }

    pub(super) fn workspace(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        let wide = ui.available_width() >= 720.0
            && ui.available_height() >= ui.spacing().interact_size.y * 8.0;
        ui.horizontal_wrapped(|ui| {
            if wide {
                ui.add(Checkbox::new(&mut self.show_inspector, "检查"));
                ui.add(Checkbox::new(&mut self.show_trace, "轨迹"));
            }
            self.workspace_status(ui, snapshot);
        });
        self.debugger.show_workspace_error(ui);
        if !wide {
            let choices = [
                (View::Source, "源码"),
                (View::Inspector, "检查"),
                (View::Trace, "轨迹"),
            ];
            let tabs = choices.map(|(view, label)| {
                Tab::new(egui::Id::new(("ai-workspace-view", self.page, view)), label)
            });
            let mut navigation = NavigationState::default();
            navigation.select(egui::Id::new((
                "ai-workspace-view",
                self.page,
                self.compact_view,
            )));
            Tabs::new(egui::Id::new(("ai-workspace-views", self.page))).show(
                ui,
                &mut navigation,
                &tabs,
                |ui, selected| {
                    self.compact_view =
                        choices[tabs.iter().position(|tab| tab.id == selected).unwrap()].0;
                    match self.compact_view {
                        View::Source => self.workspace_source(ui, snapshot, control, active),
                        View::Inspector => self.inspector(ui, snapshot, control, active),
                        View::Trace => self.workspace_trace(ui, snapshot),
                    }
                },
            );
            return;
        }
        ui.separator();
        if self.show_trace {
            SplitPane::vertical(egui::Id::new(("ai-workspace-trace", self.page)))
                .default_ratio(0.7)
                .min_sizes(160.0, 96.0)
                .show(ui, |main, trace| {
                    self.workspace_main(main, snapshot, control, active);
                    self.workspace_trace(trace, snapshot);
                });
        } else {
            self.workspace_main(ui, snapshot, control, active);
        }
    }

    fn workspace_main(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        if self.show_inspector {
            SplitPane::horizontal(egui::Id::new(("ai-workspace-inspector", self.page)))
                .default_ratio(0.7)
                .min_sizes(280.0, 200.0)
                .show(ui, |source, inspector| {
                    self.workspace_source(source, snapshot, control, active);
                    self.inspector(inspector, snapshot, control, active);
                });
        } else {
            self.workspace_source(ui, snapshot, control, active);
        }
    }

    fn workspace_trace(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        ui.scope_builder(
            egui::UiBuilder::new().id(egui::Id::new(("ai-trace-pane", self.page))),
            |ui| {
                if self.page == Page::Live {
                    self.debugger.trace_panel(ui, snapshot, self.selected);
                } else {
                    self.debugger.replay_trace(ui);
                }
            },
        );
    }

    fn workspace_status(&self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        let tokens = Tokens::get(ui);
        let status = if self.page == Page::Replay {
            Some(("回放 · 只读", tokens.primary))
        } else if self.replacement.is_some()
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.replacement.is_some())
        {
            Some(("加载替换实例…", ui.visuals().weak_text_color()))
        } else if self.pending.is_some() {
            Some(("处理中…", ui.visuals().weak_text_color()))
        } else if debugger::session(snapshot, self.selected).is_some_and(|debug| debug.paused) {
            Some(("AI 暂停", tokens.primary))
        } else {
            None
        };
        if let Some((text, color)) = status {
            ui.label(egui::RichText::new(text).small().color(color));
        }
        if self.page == Page::Live && self.draft.is_modified() {
            ui.label(
                egui::RichText::new("草稿未应用")
                    .small()
                    .color(tokens.primary),
            );
        }
    }

    fn inspector(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        ui.scope_builder(
            egui::UiBuilder::new().id(egui::Id::new(("ai-inspector-pane", self.page))),
            |ui| {
                let tabs = [(0_usize, "监视"), (1, "指令"), (2, "断点")].map(|(index, name)| {
                    Tab::new(egui::Id::new(("ai-inspector-tab", self.page, index)), name)
                });
                let tabs = if self.page == Page::Live {
                    &tabs[..]
                } else {
                    &tabs[..2]
                };
                let mut navigation = NavigationState::default();
                navigation.select(tabs[self.inspector].id);
                Tabs::new(egui::Id::new(("ai-inspector-tabs", self.page))).show(
                    ui,
                    &mut navigation,
                    tabs,
                    |ui, selected| {
                        self.inspector = tabs.iter().position(|tab| tab.id == selected).unwrap();
                        egui::ScrollArea::both()
                            .id_salt("ai-inspector-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.page != Page::Live {
                                    self.debugger.replay_inspector(ui, self.inspector == 1);
                                    return;
                                }
                                match self.inspector {
                                    2 => self.debugger.breakpoints(
                                        ui,
                                        snapshot,
                                        control,
                                        self.selected,
                                        active,
                                    ),
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
                    },
                );
            },
        );
    }

    fn workspace_source(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        ui.scope_builder(
            egui::UiBuilder::new().id(egui::Id::new(("ai-source-pane", self.page))),
            |ui| {
                if self.page == Page::Live
                    && (self.replacement.is_some()
                        || self
                            .pending
                            .as_ref()
                            .is_some_and(|pending| pending.replacement.is_some()))
                {
                    return;
                }
                if self.page == Page::Replay {
                    self.debugger
                        .recorded_source(ui, snapshot, self.selected, true);
                } else {
                    self.source_page(ui, snapshot, control, active);
                }
            },
        );
    }
}

fn toolbar_separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            ui.spacing().item_spacing.x * 2.0,
            ui.spacing().interact_size.y,
        ),
        egui::Sense::hover(),
    );
    ui.painter().vline(
        rect.center().x,
        (rect.top() + 4.0)..=(rect.bottom() - 4.0),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}
