#[cfg(test)]
mod tests;
mod view;

use crate::manager::{CATALOG, Manager, Snapshot};
use egui_hunter::{DialogState, NoticeKind, notice};
use mhf_mod_package::{
    DependencyIssue, DependencyIssueKind, ModDiagnostic, Resolved, Selection, VersionReq,
    diagnose_resolution,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(crate) fn run(manager: Manager) -> Result<(), String> {
    eframe::run_native(
        "Mod 管理",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                        .expect("valid embedded Mod manager icon"),
                )
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([420.0, 440.0]),
            centered: true,
            ..Default::default()
        },
        Box::new(move |context| {
            mhf_font::install(&context.egui_ctx);
            egui_hunter::Theme::default().apply(&context.egui_ctx);
            Ok(Box::new(App::new(manager, &context.egui_ctx)))
        }),
    )
    .map_err(|error| format!("无法打开 Mod 管理器：{error}"))
}

#[derive(Default)]
struct Draft {
    enabled: Option<bool>,
    version: String,
}

enum Completion {
    Loaded(Snapshot, Option<String>),
    Exported(PathBuf, usize),
}

struct Pending {
    label: &'static str,
    result: Receiver<Result<Completion, String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArchiveAction {
    Import,
    Export,
}

impl ArchiveAction {
    fn label(self) -> &'static str {
        match self {
            Self::Import => "导入 ZIP",
            Self::Export => "导出 ZIP",
        }
    }
}

struct App {
    manager: Manager,
    snapshot: Option<Snapshot>,
    draft: BTreeMap<String, Draft>,
    preview: Result<Resolved, String>,
    diagnostics: BTreeMap<String, ModDiagnostic>,
    selected: Option<String>,
    filter: String,
    pending: Option<Pending>,
    feedback: Option<(NoticeKind, String)>,
    archive_action: ArchiveAction,
    archive_path: String,
    archive_error: Option<String>,
    archive_dialog: DialogState,
    close_dialog: DialogState,
    close_after_save: bool,
    allow_close: bool,
}

impl App {
    fn new(manager: Manager, context: &egui::Context) -> Self {
        let mut app = Self {
            manager,
            snapshot: None,
            draft: BTreeMap::new(),
            preview: Err("正在读取配置与已安装 Mod…".into()),
            diagnostics: BTreeMap::new(),
            selected: None,
            filter: String::new(),
            pending: None,
            feedback: None,
            archive_action: ArchiveAction::Import,
            archive_path: String::new(),
            archive_error: None,
            archive_dialog: DialogState::default(),
            close_dialog: DialogState::default(),
            close_after_save: false,
            allow_close: false,
        };
        app.refresh(context);
        app
    }

    fn dirty(&self) -> bool {
        self.draft.iter().any(|(id, draft)| self.changed(id, draft))
    }

    fn changed(&self, id: &str, draft: &Draft) -> bool {
        let original = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config.modules.get(id));
        draft.enabled != original.and_then(|settings| settings.enabled)
            || match original.and_then(|settings| settings.version.as_ref()) {
                Some(version) => draft.version != version.to_string(),
                None => !draft.version.is_empty(),
            }
    }

    fn reset_draft(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        self.draft = snapshot
            .config
            .modules
            .iter()
            .map(|(id, settings)| {
                (
                    id.clone(),
                    Draft {
                        enabled: settings.enabled,
                        version: settings
                            .version
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    },
                )
            })
            .collect();
        for candidate in &snapshot.candidates {
            self.draft.entry(candidate.manifest.id.clone()).or_default();
        }
        self.update_preview();
    }

    fn can_edit(&self) -> bool {
        self.snapshot.is_some() && self.pending.is_none()
    }

    fn selections(&self) -> Result<BTreeMap<String, Selection>, String> {
        self.draft
            .iter()
            .filter(|(id, draft)| self.changed(id, draft))
            .map(|(id, draft)| {
                let version =
                    parse_version(&draft.version).map_err(|error| format!("{id}：{error}"))?;
                Ok((
                    id.clone(),
                    Selection {
                        enabled: draft.enabled,
                        version,
                    },
                ))
            })
            .collect()
    }

    fn update_preview(&mut self) {
        self.diagnostics.clear();
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let edits = match self.selections() {
            Ok(edits) => edits,
            Err(error) => {
                self.preview = Err(error);
                for (id, draft) in &self.draft {
                    if let Err(message) = parse_version(&draft.version) {
                        self.diagnostics.insert(
                            id.clone(),
                            ModDiagnostic {
                                candidate: None,
                                issues: vec![DependencyIssue {
                                    dependency: None,
                                    requirement: None,
                                    kind: DependencyIssueKind::InvalidVersion,
                                    message,
                                }],
                            },
                        );
                    }
                }
                return;
            }
        };
        self.preview = self.manager.preview(snapshot, &edits);
        let mut selections = snapshot.config.selections();
        selections.extend(edits);
        // Diagnose the launch selection. Save/export still use explicit roots.
        self.diagnostics = diagnose_resolution(
            &snapshot.candidates,
            &selections,
            &CATALOG.defaults(),
            &BTreeSet::new(),
        );
        self.diagnostics.retain(|id, _| {
            self.draft
                .get(id)
                .is_none_or(|draft| draft.enabled != Some(false))
        });
    }

    fn start(
        &mut self,
        context: &egui::Context,
        label: &'static str,
        work: impl FnOnce() -> Result<Completion, String> + Send + 'static,
    ) {
        if self.pending.is_some() {
            return;
        }
        let (sender, result) = mpsc::channel();
        let context = context.clone();
        match std::thread::Builder::new()
            .name("mhf-mods-io".into())
            .spawn(move || {
                let _ = sender.send(work());
                context.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(Pending { label, result });
                self.feedback = None;
            }
            Err(error) => {
                self.feedback = Some((NoticeKind::Danger, format!("无法开始操作：{error}")));
            }
        }
    }

    fn poll(&mut self, context: &egui::Context) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("后台操作意外中断，请重试。".into()),
        };
        self.pending = None;
        match result {
            Ok(Completion::Loaded(snapshot, message)) => {
                self.snapshot = Some(snapshot);
                self.reset_draft();
                if self
                    .selected
                    .as_ref()
                    .is_none_or(|id| !self.draft.contains_key(id))
                {
                    self.selected = self.draft.keys().next().cloned();
                }
                self.feedback = message.map(|message| (NoticeKind::Success, message));
                if self.close_after_save {
                    self.allow_close = true;
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Ok(Completion::Exported(path, count)) => {
                self.feedback = Some((
                    NoticeKind::Success,
                    format!("已导出 {count} 个 Mod：{}", path.display()),
                ));
            }
            Err(error) => self.feedback = Some((NoticeKind::Danger, error)),
        }
        self.close_after_save = false;
    }

    fn refresh(&mut self, context: &egui::Context) {
        if self.dirty() || self.pending.is_some() {
            return;
        }
        let manager = self.manager.clone();
        self.start(
            context,
            "正在读取配置与已安装 Mod…",
            move || {
                let snapshot = manager.load()?;
                Ok(Completion::Loaded(snapshot, None))
            },
        );
    }

    fn save(&mut self, context: &egui::Context) {
        if !self.can_edit() || !self.dirty() || self.preview.is_err() {
            return;
        }
        let selections = match self.selections() {
            Ok(selections) => selections,
            Err(error) => {
                self.feedback = Some((NoticeKind::Danger, error));
                return;
            }
        };
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let baseline = snapshot.config.clone();
        let manager = self.manager.clone();
        self.start(context, "正在保存设置…", move || {
            let snapshot = manager.save(&baseline, &selections)?;
            Ok(Completion::Loaded(
                snapshot,
                Some("已保存设置，下次启动游戏生效。".into()),
            ))
        });
    }

    fn open_archive(&mut self, action: ArchiveAction, context: &egui::Context) {
        self.archive_action = action;
        self.archive_path.clear();
        self.archive_error = None;
        self.archive_dialog.open(context);
    }

    fn archive(&mut self, context: &egui::Context) {
        if !self.can_edit()
            || self.dirty()
            || (self.archive_action == ArchiveAction::Export && self.preview.is_err())
        {
            return;
        }
        if self.archive_path.trim().is_empty() {
            self.archive_error = Some("请填写 ZIP 文件路径。".into());
            return;
        }
        let path = PathBuf::from(&self.archive_path);
        let manager = self.manager.clone();
        let action = self.archive_action;
        self.start(
            context,
            match action {
                ArchiveAction::Import => "正在导入 ZIP…",
                ArchiveAction::Export => "正在导出 ZIP…",
            },
            move || match action {
                ArchiveAction::Import => {
                    let (snapshot, count) = manager.import(&path)?;
                    Ok(Completion::Loaded(
                        snapshot,
                        Some(format!("已导入 {count} 个包；可在列表中选择启用。")),
                    ))
                }
                ArchiveAction::Export => {
                    let count = manager.export(&path)?;
                    Ok(Completion::Exported(path, count))
                }
            },
        );
        self.archive_dialog.close(context);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll(ui.ctx());
        if ui.input(|input| input.viewport().close_requested()) && !self.allow_close {
            if self.pending.is_some() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.dirty() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_dialog.open(ui.ctx());
            }
        }
        egui::Panel::bottom("save_bar")
            .exact_size(64.0)
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .fill(ui.visuals().window_fill())
                    .stroke(ui.visuals().window_stroke),
            )
            .show(ui, |ui| self.footer(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(16)
                    .fill(ui.visuals().panel_fill),
            )
            .show(ui, |ui| {
                self.header(ui);
                if let Some((kind, text)) = &self.feedback {
                    egui::ScrollArea::vertical()
                        .id_salt("operation_feedback")
                        .max_height(75.0)
                        .show(ui, |ui| {
                            notice(ui, *kind, text);
                        });
                }
                ui.add_space(12.0);
                self.content(ui);
            });
        self.dialogs(ui.ctx());
    }
}

fn parse_version(text: &str) -> Result<Option<VersionReq>, String> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        text.trim()
            .parse()
            .map(Some)
            .map_err(|error| format!("无效版本要求：{error}"))
    }
}
