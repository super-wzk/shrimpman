use super::*;
use crate::dialogs;
use egui::{Align, Id, Layout, RichText};
use egui_hunter::{
    Button, ButtonKind, Dialog, Field, FormLayout, Panel, SelectField, TextField, Tokens,
    Validation, notice,
};
use mhf_mod_package::{Candidate, Kind, Source};

impl App {
    pub(super) fn header(&mut self, ui: &mut egui::Ui) {
        if ui.available_width() < 360.0 {
            ui.heading("Mod 管理");
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| self.header_actions(ui));
        } else {
            ui.horizontal(|ui| {
                ui.heading("Mod 管理");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.header_actions(ui);
                });
            });
        }
    }

    fn header_actions(&mut self, ui: &mut egui::Ui) {
        let dirty = self.dirty();
        let ready = self.can_edit() && !dirty;
        let has_selection = self
            .preview
            .as_ref()
            .is_ok_and(|resolved| !resolved.mods.is_empty());
        // The desktop action group flows right to left; keep its visual order
        // identical to the wrapped narrow layout.
        let actions = if ui.layout().main_dir() == egui::Direction::RightToLeft {
            ["导出 ZIP", "导入 ZIP", "刷新"]
        } else {
            ["刷新", "导入 ZIP", "导出 ZIP"]
        };
        for action in actions {
            match action {
                "刷新" => {
                    if ui
                        .add_enabled(
                            !dirty && self.pending.is_none(),
                            Button::new(action)
                                .id(Id::new("refresh_mods"))
                                .kind(ButtonKind::Quiet),
                        )
                        .on_disabled_hover_text(if self.pending.is_some() {
                            "请等待当前操作完成。"
                        } else {
                            "请先保存或撤销修改，再刷新列表。"
                        })
                        .on_hover_ui(|ui| {
                            ui.label(format!("配置：{}", self.manager.config_path.display()));
                            if let Some(snapshot) = &self.snapshot {
                                ui.label(format!("Mod 目录：{}", snapshot.mods_dir.display()));
                            }
                        })
                        .clicked()
                    {
                        self.refresh(ui.ctx());
                    }
                }
                "导入 ZIP" => {
                    if ui
                        .add_enabled(ready, Button::new(action).id(Id::new("import_mods")))
                        .on_disabled_hover_text("请先完成当前操作，并保存或撤销修改。")
                        .clicked()
                    {
                        self.open_archive(ArchiveAction::Import, ui.ctx());
                    }
                }
                _ => {
                    if ui
                        .add_enabled(
                            ready && has_selection,
                            Button::new(action).id(Id::new("export_mods")),
                        )
                        .on_hover_text("导出已启用 Mod 及依赖")
                        .on_disabled_hover_text(if dirty {
                            "请先保存修改"
                        } else if self.preview.is_err() {
                            "请先解决依赖问题"
                        } else {
                            "请先启用需要导出的 Mod"
                        })
                        .clicked()
                    {
                        self.open_archive(ArchiveAction::Export, ui.ctx());
                    }
                }
            }
        }
    }

    pub(super) fn footer(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            self.save_actions(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if let Some(pending) = &self.pending {
                    ui.spinner();
                    ui.add(egui::Label::new(RichText::new("处理中…").small()).truncate())
                        .on_hover_text(pending.label);
                } else if self.snapshot.is_some()
                    && let Err(error) = &self.preview
                {
                    let summary = if ui.available_width() < 160.0 {
                        "检查未通过"
                    } else {
                        "检查未通过，无法保存"
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(summary)
                                .small()
                                .color(ui.visuals().error_fg_color),
                        )
                        .truncate(),
                    )
                    .on_hover_text(error);
                } else if self.dirty() {
                    let count = self
                        .draft
                        .iter()
                        .filter(|(id, draft)| self.changed(id, draft))
                        .count();
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{count} 项未保存"))
                                .small()
                                .color(Tokens::get(ui).primary),
                        )
                        .truncate(),
                    )
                    .on_hover_text(format!("{count} 个 Mod 有未保存修改"));
                } else {
                    ui.add(
                        egui::Label::new(RichText::new("下次启动游戏生效").small().weak())
                            .truncate(),
                    );
                }
            });
        });
    }

    fn save_actions(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.can_edit() && self.dirty() && self.preview.is_ok(),
                Button::new("保存设置")
                    .id(Id::new("save_mod_settings"))
                    .kind(ButtonKind::Primary)
                    .min_size(egui::vec2(112.0, 44.0)),
            )
            .on_disabled_hover_text(if self.pending.is_some() {
                "请等待当前操作完成。"
            } else if self.snapshot.is_none() {
                "请先读取配置与已安装 Mod。"
            } else if let Err(error) = &self.preview {
                error
            } else {
                "没有未保存的修改。"
            })
            .clicked()
        {
            self.save(ui.ctx());
        }
        if ui
            .add_enabled(
                self.can_edit() && self.dirty(),
                Button::new("撤销修改")
                    .id(Id::new("revert_mod_settings"))
                    .kind(ButtonKind::Quiet),
            )
            .clicked()
        {
            self.reset_draft();
            self.feedback = None;
        }
    }

    pub(super) fn content(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height().max(0.0);
        if ui.available_width() < 640.0 {
            egui::ScrollArea::vertical()
                .id_salt("mod_manager_narrow")
                .content_margin(egui::Margin {
                    right: 12,
                    ..egui::Margin::ZERO
                })
                .min_scrolled_height(0.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Panel::new("").show(ui, |ui| {
                        self.mod_list(ui, 160.0);
                    });
                    ui.add_space(12.0);
                    Panel::new("").show(ui, |ui| self.details(ui));
                });
            return;
        }
        let gap = 12.0;
        let width = ui.available_width();
        let list_width = (width * 0.28).clamp(248.0, 272.0);
        ui.spacing_mut().item_spacing.x = gap;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(list_width, height),
                Layout::top_down(Align::Min),
                |ui| {
                    Panel::new("").show(ui, |ui| {
                        let padding = ui.spacing().window_margin.sum().y
                            + ui.visuals().window_stroke.width * 2.0;
                        let content_height = (height - padding).max(0.0);
                        ui.set_min_height(content_height);
                        self.mod_list(ui, (content_height - 76.0).max(0.0));
                    });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(width - list_width - gap, height),
                Layout::top_down(Align::Min),
                |ui| {
                    Panel::new("").show(ui, |ui| {
                        let padding = ui.spacing().window_margin.sum().y
                            + ui.visuals().window_stroke.width * 2.0;
                        let content_height = (height - padding).max(0.0);
                        ui.set_min_height(content_height);
                        egui::ScrollArea::vertical()
                            .id_salt(("mod_details", self.selected.as_deref()))
                            .content_margin(egui::Margin {
                                right: 12,
                                ..egui::Margin::ZERO
                            })
                            .max_height(content_height)
                            .min_scrolled_height(0.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.details(ui));
                    });
                },
            );
        });
    }

    fn mod_list(&mut self, ui: &mut egui::Ui, list_height: f32) {
        ui.add(
            TextField::new(Id::new("mod_filter"), &mut self.filter)
                .label("搜索 Mod")
                .hint("名称或 ID"),
        );
        let filter = self.filter.to_lowercase();
        let Some(snapshot) = &self.snapshot else {
            ui.label(if self.pending.is_some() {
                "正在读取…"
            } else {
                "读取失败，请检查路径后刷新。"
            });
            return;
        };
        ui.add_space(4.0);
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("installed_mods")
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .max_height(list_height)
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let mut count = 0;
                for (id, draft) in &self.draft {
                    let candidate = snapshot
                        .candidates
                        .iter()
                        .find(|candidate| &candidate.manifest.id == id);
                    let name = candidate
                        .map(|candidate| candidate.manifest.name.as_str())
                        .unwrap_or(id);
                    if !id.to_lowercase().contains(&filter)
                        && !name.to_lowercase().contains(&filter)
                    {
                        continue;
                    }
                    count += 1;
                    let source = match candidate.map(|candidate| &candidate.source) {
                        Some(Source::Builtin) => "内置",
                        Some(Source::Directory(_)) => "外部",
                        None => "未安装",
                    };
                    let row = self.mod_row(ui, id, name, source, draft);
                    if row.clicked() {
                        clicked = Some(id.clone());
                    }
                }
                if count == 0 {
                    ui.add_space(12.0);
                    ui.label(RichText::new("没有匹配的 Mod").weak());
                }
            });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let Some(snapshot) = &self.snapshot else {
            ui.label(if self.pending.is_some() {
                "正在读取配置与已安装 Mod…"
            } else {
                "检查配置路径与 Mod 目录，然后刷新。"
            });
            return;
        };
        let Some(id) = self.selected.clone() else {
            ui.label("选择一个 Mod 查看设置与依赖。");
            return;
        };
        let mut candidates: Vec<_> = snapshot
            .candidates
            .iter()
            .filter(|candidate| candidate.manifest.id == id)
            .cloned()
            .collect();
        candidates.sort_by(|a, b| b.manifest.version.cmp(&a.manifest.version));
        ui.add(
            egui::Label::new(
                RichText::new(
                    candidates
                        .first()
                        .map(|candidate| candidate.manifest.name.as_str())
                        .unwrap_or(&id),
                )
                .size(20.0)
                .strong(),
            )
            .wrap(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Label::new(RichText::new(&id).small().weak()).wrap());
            if self
                .draft
                .get(&id)
                .is_some_and(|draft| self.changed(&id, draft))
            {
                ui.label(
                    RichText::new("未保存")
                        .small()
                        .color(Tokens::get(ui).primary),
                );
            }
        });
        ui.add_space(8.0);
        let can_edit = self.can_edit();
        let mut changed = false;
        ui.add_enabled_ui(can_edit, |ui| {
            let Some(draft) = self.draft.get_mut(&id) else {
                return;
            };
            ui.horizontal_wrapped(|ui| {
                ui.label("启用设置");
                ui.spacing_mut().item_spacing.x = 4.0;
                for (value, label) in [(None, "自动"), (Some(true), "启用"), (Some(false), "禁用")]
                {
                    if ui
                        .add(
                            Button::new(label)
                                .selected(draft.enabled == value)
                                .min_size(egui::vec2(80.0, 36.0)),
                        )
                        .clicked()
                    {
                        changed |= draft.enabled != value;
                        draft.enabled = value;
                    }
                }
            });
            ui.add(
                egui::Label::new(
                    RichText::new("自动：被依赖或默认启动规则需要时启用。")
                        .small()
                        .weak(),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            let error = parse_version(&draft.version).err();
            let fields = [
                Field::new(Id::new(("version", &id)))
                    .label("指定版本")
                    .validation(error.as_deref().map(Validation::Error).unwrap_or_default()),
                Field::new(Id::new(("installed_versions", &id))).label("已安装版本"),
            ];
            changed |= FormLayout::new(Id::new(("version_settings", &id)))
                .max_columns(2)
                .min_column_width(220.0)
                .show(ui, &fields, |ui, index| match index {
                    0 => ui.add(
                        TextField::new(Id::new(("version", &id)), &mut draft.version)
                            .hint("留空不限，如 ^1.2"),
                    ),
                    _ => installed_versions(ui, &id, draft, &candidates),
                })
                .inner
                .iter()
                .any(egui::Response::changed);
        });
        if changed {
            self.update_preview();
        }
        ui.add_space(12.0);
        let diagnostic = self.diagnostics.get(&id);
        let own_issues = diagnostic
            .into_iter()
            .flat_map(|diagnostic| &diagnostic.issues)
            .filter(|issue| {
                issue.dependency.is_none() && issue.kind != DependencyIssueKind::InvalidVersion
            })
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if !own_issues.is_empty() {
            notice(ui, NoticeKind::Danger, &own_issues);
        }
        let candidate = diagnostic
            .and_then(|diagnostic| diagnostic.candidate.as_ref())
            .or_else(|| {
                self.draft
                    .get(&id)
                    .and_then(|draft| parse_version(&draft.version).ok())
                    .and_then(|requirement| {
                        candidates.iter().find(|candidate| {
                            requirement.as_ref().is_none_or(|requirement| {
                                requirement.matches(&candidate.manifest.version)
                            })
                        })
                    })
            });
        if let Some(candidate) = candidate {
            dependency_details(ui, candidate, diagnostic);
        } else if diagnostic.is_none_or(|diagnostic| {
            diagnostic
                .issues
                .iter()
                .all(|issue| issue.dependency.is_some())
        }) {
            ui.label(
                RichText::new(if candidates.is_empty() {
                    "未安装"
                } else {
                    "没有满足版本要求的已安装包。"
                })
                .small()
                .weak(),
            );
        }
        if !candidates.is_empty() {
            ui.add_space(8.0);
            ui.separator();
            egui::CollapsingHeader::new("包详情")
                .id_salt(("package_details", &id))
                .show(ui, |ui| {
                    for candidate in &candidates {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {}",
                                candidate.manifest.version,
                                match candidate.source {
                                    Source::Builtin => "内置",
                                    Source::Directory(_) => "外部",
                                },
                                match candidate.manifest.kind {
                                    Kind::Native => "原生 Mod",
                                    Kind::Data => "数据包",
                                }
                            ))
                            .strong(),
                        );
                        candidate_details(ui, candidate);
                    }
                });
        }
    }

    pub(super) fn dialogs(&mut self, context: &egui::Context) {
        let mut confirm = false;
        Dialog::new(Id::new("archive_dialog"), self.archive_action.label())
            .width(540.0)
            .initial_focus(Id::new("archive_path"))
            .dismiss_on_backdrop(false)
            .show(context, &mut self.archive_dialog, |ui| {
                ui.add(
                    egui::Label::new(match self.archive_action {
                        ArchiveAction::Import => "导入包到当前 Mod 目录，随后可在列表中启用。",
                        ArchiveAction::Export => {
                            "导出已保存且明确启用的 Mod 及依赖；内置 Mod 记录版本，由游戏提供。"
                        }
                    })
                    .wrap(),
                );
                if ui
                    .add(
                        TextField::new(Id::new("archive_path"), &mut self.archive_path)
                            .label("ZIP 文件路径")
                            .hint("选择文件或输入完整路径"),
                    )
                    .changed()
                {
                    self.archive_error = None;
                }
                let output_exists = self.archive_action == ArchiveAction::Export
                    && PathBuf::from(&self.archive_path).exists();
                if output_exists {
                    notice(ui, NoticeKind::Warning, "文件已存在，请使用新文件名。");
                } else if let Some(error) = &self.archive_error {
                    notice(ui, NoticeKind::Danger, error);
                }
                ui.horizontal(|ui| {
                    if ui.add(Button::new("浏览…")).clicked() {
                        match match self.archive_action {
                            ArchiveAction::Import => dialogs::import_archive(),
                            ArchiveAction::Export => dialogs::export_archive(),
                        } {
                            Ok(Some(path)) => {
                                self.archive_path = path.display().to_string();
                                self.archive_error = None;
                            }
                            Ok(None) => {}
                            Err(error) => self.archive_error = Some(error),
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.archive_path.trim().is_empty() && !output_exists,
                                Button::new(self.archive_action.label()).kind(ButtonKind::Primary),
                            )
                            .clicked()
                        {
                            confirm = true;
                        }
                        if ui.add(Button::new("取消")).clicked() {
                            ui.close();
                        }
                    });
                });
            });
        if confirm {
            self.archive(context);
        }
        let mut save_and_close = false;
        Dialog::new(Id::new("close_dialog"), "还有未保存的修改")
            .width(460.0)
            .initial_focus(Id::new("continue_editing"))
            .dismiss_on_backdrop(false)
            .show(context, &mut self.close_dialog, |ui| {
                ui.label("退出前可以保存设置，或放弃本次修改。");
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(Button::new("继续编辑").id(Id::new("continue_editing")))
                        .clicked()
                    {
                        ui.close();
                    }
                    if ui
                        .add(Button::new("放弃并退出").kind(ButtonKind::Danger))
                        .clicked()
                    {
                        self.allow_close = true;
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.preview.is_ok(),
                            Button::new("保存并退出").kind(ButtonKind::Primary),
                        )
                        .on_disabled_hover_text(
                            self.preview
                                .as_ref()
                                .err()
                                .map(String::as_str)
                                .unwrap_or_default(),
                        )
                        .clicked()
                    {
                        save_and_close = true;
                        ui.close();
                    }
                });
            });
        if save_and_close {
            self.close_after_save = true;
            self.save(context);
            if self.pending.is_none() {
                self.close_after_save = false;
            }
        }
    }
}

fn installed_versions(
    ui: &mut egui::Ui,
    id: &str,
    draft: &mut Draft,
    candidates: &[Candidate],
) -> egui::Response {
    let versions = candidates
        .iter()
        .map(|candidate| candidate.manifest.version.to_string())
        .collect::<Vec<_>>()
        .join("、");
    let mut changed = false;
    let mut response = SelectField::new(
        Id::new(("installed_versions", id)),
        if versions.is_empty() {
            "未安装"
        } else {
            &versions
        },
    )
    .show_ui(ui, |ui| {
        if ui
            .selectable_label(draft.version.is_empty(), "任意兼容版本")
            .clicked()
        {
            changed |= !draft.version.is_empty();
            draft.version.clear();
            ui.close();
        }
        for candidate in candidates {
            let version = format!("={}", candidate.manifest.version);
            if ui
                .selectable_label(draft.version == version, &version)
                .clicked()
            {
                changed |= draft.version != version;
                draft.version = version;
                ui.close();
            }
        }
    })
    .response;
    if changed {
        response.mark_changed();
    }
    response
}

impl App {
    fn mod_row(
        &self,
        ui: &mut egui::Ui,
        id: &str,
        name: &str,
        source: &str,
        draft: &Draft,
    ) -> egui::Response {
        let changed = self.changed(id, draft);
        let selected = self.selected.as_deref() == Some(id);
        let issues = self
            .diagnostics
            .get(id)
            .map(|diagnostic| diagnostic.issues.as_slice())
            .unwrap_or_default();
        let problem = issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let has_problem = !issues.is_empty();
        let status = match draft.enabled {
            None => "自动",
            Some(true) => "启用",
            Some(false) => "禁用",
        };
        let state = if changed {
            format!("{status} · 已修改")
        } else {
            status.to_owned()
        };
        let response = ui.add(
            Button::new("")
                .id(Id::new(("mod_row", id)))
                .kind(ButtonKind::Quiet)
                .selected(selected)
                .min_size(egui::vec2(0.0, 64.0))
                .full_width(),
        );
        let description = if has_problem {
            format!("{name}，{id}，{source}，{state}，{problem}")
        } else {
            format!("{name}，{id}，{source}，{state}")
        };
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                &description,
            )
        });
        if ui.is_rect_visible(response.rect) {
            let rect = response.rect.shrink2(egui::vec2(10.0, 10.0));
            let foreground = if has_problem {
                ui.visuals().error_fg_color
            } else if selected {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            };
            let secondary = ui.visuals().weak_text_color();
            let painter = ui.painter_at(rect);
            let state_font = egui::TextStyle::Small.resolve(ui.style());
            let state_color = if has_problem {
                ui.visuals().error_fg_color
            } else if changed {
                Tokens::get(ui).primary
            } else if draft.enabled == Some(true) {
                Tokens::get(ui).success
            } else {
                secondary
            };
            let state_galley =
                ui.fonts_mut(|fonts| fonts.layout_no_wrap(state.clone(), state_font, state_color));
            let marker_width = ui.spacing().icon_width + 8.0;
            let state_right = rect.right() - marker_width;
            let name_width = (state_right - state_galley.size().x - 8.0 - rect.left()).max(0.0);
            let metadata = format!("{id} · {source}");
            let metadata_offset = if has_problem {
                let side = ui.spacing().icon_width_inner;
                egui_hunter::Icon::Warning.paint(
                    &painter,
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left(), rect.top() + 24.0),
                        egui::Vec2::splat(side),
                    ),
                    ui.visuals().error_fg_color,
                );
                side + ui.spacing().icon_spacing
            } else {
                0.0
            };
            for (text, x, y, style, color, width) in [
                (
                    name,
                    rect.left(),
                    rect.top(),
                    egui::TextStyle::Body,
                    foreground,
                    name_width,
                ),
                (
                    metadata.as_str(),
                    rect.left() + metadata_offset,
                    rect.top() + 24.0,
                    egui::TextStyle::Small,
                    secondary,
                    (rect.width() - marker_width - metadata_offset).max(0.0),
                ),
            ] {
                let mut job = egui::text::LayoutJob::simple_singleline(
                    text.to_owned(),
                    style.resolve(ui.style()),
                    color,
                );
                job.wrap.max_width = width;
                job.wrap.max_rows = 1;
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                painter.galley(egui::pos2(x, y), galley, color);
            }
            painter.galley(
                egui::pos2(state_right - state_galley.size().x, rect.top() + 1.0),
                state_galley,
                state_color,
            );
        }
        response.on_hover_text(if has_problem {
            format!("{name}\n{id}\n{source} · {state}\n{problem}")
        } else {
            format!("{name}\n{id}\n{source} · {state}")
        })
    }
}

fn candidate_details(ui: &mut egui::Ui, candidate: &Candidate) {
    match &candidate.source {
        Source::Builtin => {
            ui.label("随游戏提供");
        }
        Source::Directory(path) => {
            ui.add(egui::Label::new(format!("位置：{}", path.display())).wrap());
        }
    }
    if let Some(entry) = &candidate.manifest.entry {
        ui.add(egui::Label::new(format!("入口：{}", entry.display())).wrap());
    }
    dependency_details(ui, candidate, None);
}

fn dependency_details(
    ui: &mut egui::Ui,
    candidate: &Candidate,
    diagnostic: Option<&ModDiagnostic>,
) {
    let issues = diagnostic
        .map(|diagnostic| diagnostic.issues.as_slice())
        .unwrap_or_default();
    if candidate.manifest.dependencies.is_empty() {
        ui.label(RichText::new("无依赖").small().weak());
    } else {
        for (id, version) in &candidate.manifest.dependencies {
            let problem = issues
                .iter()
                .filter(|issue| issue.dependency.as_deref() == Some(id.as_str()))
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("；");
            if problem.is_empty() {
                ui.add(egui::Label::new(format!("{id}  {version}")).wrap());
            } else {
                ui.horizontal_top(|ui| {
                    let color = ui.visuals().error_fg_color;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::Vec2::splat(ui.spacing().icon_width_inner),
                        egui::Sense::hover(),
                    );
                    egui_hunter::Icon::Warning.paint(ui.painter(), rect, color);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{id}  {version} · {problem}")).color(color),
                        )
                        .wrap(),
                    );
                });
            }
        }
    }
}
