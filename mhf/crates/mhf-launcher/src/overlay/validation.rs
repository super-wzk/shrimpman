use egui::{Context, Event, Id, Key, Modifiers, Ui};
use egui_hunter::components::dialog::interaction::DialogInteraction;
use egui_hunter::{
    Button, ButtonKind, Checkbox, Dialog, DialogState, FocusEngagement, Icon, NoticeKind,
    Notifications, Panel, Popup, ResponsiveColumns, ScrollPanel, TextField, Validation, key_hint,
    notice,
};
use mhf_overlay::{InputCapture, InputPolicy};

const RECORD_COUNT: usize = 10_000;

pub(super) struct ValidationPage {
    modal: DialogState,
    selected: usize,
    text: String,
    editable: bool,
    dialog: DialogState,
    notices: Notifications,
}

impl Default for ValidationPage {
    fn default() -> Self {
        Self {
            modal: DialogState::default(),
            selected: 0,
            text: "新的记录".to_owned(),
            editable: true,
            dialog: DialogState::default(),
            notices: Notifications::new(Id::new("validation-notices")),
        }
    }
}

impl ValidationPage {
    pub(super) fn input_policy(&self) -> InputPolicy {
        let capture = if self.modal.is_open() {
            InputCapture::Block
        } else {
            InputCapture::PassThrough
        };
        InputPolicy {
            pointer: capture,
            keyboard: capture,
        }
    }

    pub(super) fn show(&mut self, ctx: &Context) {
        let toggle = ctx.input_mut(|input| {
            let pressed = input.events.iter().any(|event| {
                matches!(event, Event::Key {
                    key: Key::F8, pressed: true, repeat: false, modifiers, ..
                } if *modifiers == Modifiers::NONE)
            });
            input.consume_key(Modifiers::NONE, Key::F8);
            pressed
        });
        if toggle {
            if self.modal.is_open() {
                self.modal.close(ctx);
                self.dismiss_children(ctx);
            } else {
                self.modal.open(ctx);
            }
        }
        let size = (ctx.content_rect().size() - egui::vec2(48.0, 48.0))
            .min(egui::vec2(1000.0, 820.0))
            .max(egui::vec2(120.0, 120.0));
        let id = Id::new("validation-page");
        let native = egui::Modal::new(id)
            .area(
                egui::Modal::default_area(id)
                    .anchor(egui::Align2::LEFT_TOP, [24.0, 24.0])
                    .default_size(size),
            )
            .frame(egui::Frame::NONE);
        let mut modal = std::mem::take(&mut self.modal);
        let was_open = modal.is_open();
        let focus_on_open = modal.just_opened();
        DialogInteraction::default()
            .dismiss_on_backdrop(false)
            .show(ctx, &mut modal, native, |ui| {
                ui.set_width(size.x);
                ui.set_max_height(size.y);
                let mut regions = FocusEngagement::new(Id::new("validation-regions"));
                regions.begin(
                    ui,
                    focus_on_open.then(|| Id::new("validation-records-region")),
                );
                Panel::new("组件验证").show_with_header(
                    ui,
                    |ui| {
                        if ui
                            .add(Button::new("关闭").id(Id::new("validation-close")))
                            .clicked()
                        {
                            ui.close();
                        }
                    },
                    |ui| self.content(ui, &mut regions, focus_on_open),
                );
                // Render child modals before the page handles dismissal.
                self.confirmation(ctx);
            });
        self.modal = modal;
        if !self.modal.is_open() {
            if was_open {
                self.dismiss_children(ctx);
            }
            return;
        }
        self.notices.show(ctx);
    }

    fn content(&mut self, ui: &mut Ui, regions: &mut FocusEngagement, focus_on_open: bool) {
        ui.horizontal_wrapped(|ui| {
            key_hint(ui, "F8", "显示 / 隐藏");
            key_hint(ui, "Tab", "切换组件");
            key_hint(ui, "Enter", "选择 / 操作");
            key_hint(ui, "Esc", "返回 / 关闭");
        });
        ui.add_space(ui.spacing().item_spacing.y);
        egui::ScrollArea::vertical()
            .id_salt("validation-content")
            .max_height(ui.available_height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ResponsiveColumns::new(Id::new("validation-columns"))
                    .gap(ui.spacing().item_spacing.x * 2.0)
                    .min_column_width(360.0)
                    .show(ui, 4, |ui, section| match section {
                        0 => self.records(ui, regions, focus_on_open),
                        1 => self.text_input(ui, regions),
                        2 => self.overlays(ui, regions),
                        3 => self.feedback(ui, regions),
                        _ => unreachable!(),
                    });
                regions.navigate(ui);
            });
    }

    fn dismiss_children(&mut self, ctx: &Context) {
        egui::Popup::close_id(ctx, Id::new("validation-popup"));
        // The page's openers are hidden too; discard deferred focus restoration.
        self.dialog = DialogState::default();
        self.notices.clear(ctx);
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    fn records(&mut self, ui: &mut Ui, regions: &mut FocusEngagement, focus_on_open: bool) {
        let id = Id::new("validation-records-region");
        regions.show(ui, id, |ui, controls| {
            highlight_region(ui, id);
            let font = egui::FontSelection::Default
                .resolve_with_fallback(ui.style(), egui::TextStyle::Button.into());
            let row_height = (ui
                .fonts_mut(|fonts| fonts.row_height(&font))
                .max(ui.spacing().icon_width)
                + ui.spacing().button_padding.y * 2.0)
                .max(ui.spacing().interact_size.y);
            let mut panel = ScrollPanel::new(Id::new("validation-records"), "01  滚动与选择");
            panel.scroll = panel.scroll.max_height(258.0);
            let list = panel.show_list(
                ui,
                row_height,
                RECORD_COUNT,
                |_| true,
                |ui, row| {
                    ui.add_sized(
                        [ui.available_width(), row_height],
                        Button::new(&format!("记录 {:05}", row + 1))
                            .id(Id::new(("validation-record", row)))
                            .sense(egui::Sense::CLICK)
                            .icon(Icon::Quest)
                            .selected(self.selected == row),
                    )
                },
            );
            if let Some(row) = list.inner.activated {
                self.selected = row;
            }
            controls.push(list.inner.response.clone());
            if focus_on_open {
                list.inner.response.request_focus();
            }
        });
        ui.label(format!(
            "共 {RECORD_COUNT} 条 · 已选择 {:05}",
            self.selected + 1
        ));
        ui.weak("↑ / ↓ 移动，Home / End 跳到首尾，Enter 选择；Tab 切换组件，Esc 关闭页面。");
    }

    fn text_input(&mut self, ui: &mut Ui, regions: &mut FocusEngagement) {
        let id = Id::new("validation-text-region");
        regions.show(ui, id, |ui, controls| {
            highlight_region(ui, id);
            Panel::new("02  文本输入").show(ui, |ui| {
                controls.push(ui.add(Checkbox::new(&mut self.editable, "允许编辑")));
                let validation = if self.text.trim().is_empty() {
                    Validation::Error("请输入名称")
                } else {
                    Validation::None
                };
                controls.push(
                    ui.add_enabled(
                        self.editable,
                        TextField::new(Id::new("validation-text"), &mut self.text)
                            .label("记录名称")
                            .hint("输入名称")
                            .help("试试中英文输入、选中文字和方向键移动。")
                            .validation(validation),
                    ),
                );
                let text = self.text.trim();
                let confirm = ui.add_enabled(
                    !text.is_empty(),
                    Button::new("确认文本").id(Id::new("validation-submit")),
                );
                controls.push(confirm.clone());
                if confirm.clicked() {
                    self.notices
                        .push(ui.ctx(), NoticeKind::Success, format!("已确认：{text}"));
                }
            });
        });
    }

    fn overlays(&mut self, ui: &mut Ui, regions: &mut FocusEngagement) {
        let id = Id::new("validation-overlays-region");
        regions.show(ui, id, |ui, controls| {
            highlight_region(ui, id);
            Panel::new("03  窗口与弹层").show(ui, |ui| {
                let opener = ui.add(
                    Button::new("打开确认框")
                        .id(Id::new("validation-open-dialog"))
                        .kind(ButtonKind::Primary),
                );
                controls.push(opener.clone());
                if opener.clicked() {
                    self.dialog.open_from(&opener);
                }
                let anchor = ui.add(Button::new("打开菜单").id(Id::new("validation-open-popup")));
                controls.push(anchor.clone());
                let mut popup = Popup::new(&anchor)
                    .title("消息类型")
                    .initial_focus(Id::new("validation-popup-success"));
                popup.native = popup.native.id(Id::new("validation-popup"));
                popup.show(|ui| {
                    for (id, label, kind) in [
                        ("validation-popup-success", "成功提示", NoticeKind::Success),
                        ("validation-popup-warning", "提醒提示", NoticeKind::Warning),
                    ] {
                        if ui
                            .add(Button::new(label).id(Id::new(id)).full_width())
                            .clicked()
                        {
                            self.notices.push(ui.ctx(), kind, label);
                            ui.close();
                        }
                    }
                });
                ui.weak("关闭弹层后，焦点回到打开它的按钮。");
            });
        });
    }

    fn feedback(&mut self, ui: &mut Ui, regions: &mut FocusEngagement) {
        let id = Id::new("validation-feedback-region");
        regions.show(ui, id, |ui, controls| {
            highlight_region(ui, id);
            Panel::new("04  消息与反馈").show(ui, |ui| {
                notice(ui, NoticeKind::Success, "组件已就绪。");
                ui.horizontal_wrapped(|ui| {
                    let play = ui.add(Button::new("播放提示").id(Id::new("validation-notify")));
                    controls.push(play.clone());
                    if play.clicked() {
                        self.notices
                            .push(ui.ctx(), NoticeKind::Success, "第一条消息：操作完成。");
                        self.notices.push(
                            ui.ctx(),
                            NoticeKind::Warning,
                            "第二条消息：请检查输入内容。",
                        );
                    }
                    let clear = ui.add_enabled(!self.notices.is_empty(), Button::new("清空消息"));
                    controls.push(clear.clone());
                    if clear.clicked() {
                        self.notices.clear(ui.ctx());
                    }
                });
                ui.label(format!("待展示消息：{}", self.notices.len()));
            });
        });
    }

    fn confirmation(&mut self, ctx: &Context) {
        let confirm_id = Id::new("validation-confirm");
        let result = Dialog::new(Id::new("validation-dialog"), "确认选择")
            .initial_focus(confirm_id)
            .show(ctx, &mut self.dialog, |ui| {
                ui.label(format!("确认选择记录 {:05}？", self.selected + 1));
                ui.horizontal(|ui| {
                    let confirmed = ui
                        .add(Button::new("确认").id(confirm_id).kind(ButtonKind::Primary))
                        .clicked();
                    let cancelled = ui.add(Button::new("取消")).clicked();
                    if confirmed || cancelled {
                        ui.close();
                    }
                    confirmed
                })
                .inner
            });
        if result.is_some_and(|result| result.inner) {
            self.notices.push(
                ctx,
                NoticeKind::Success,
                format!("已确认记录 {:05}", self.selected + 1),
            );
        }
    }
}

fn highlight_region(ui: &mut Ui, id: Id) {
    if ui.memory(|memory| memory.has_focus(id)) {
        let active = ui.visuals().widgets.active;
        ui.visuals_mut().window_fill = active.bg_fill;
        ui.visuals_mut().window_stroke = active.bg_stroke;
    }
}

#[cfg(test)]
mod tests;
