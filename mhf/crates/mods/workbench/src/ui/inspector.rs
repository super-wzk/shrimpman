//! Bounded inspector rows. Widget contents never size the docked panel.

use super::Workbench;
use crate::inspect::{Document, Node};
use egui::{Color32, RichText};

impl Workbench {
    pub(super) fn inspector(&mut self, ui: &mut egui::Ui, document: &Document, node: &Node) {
        self.edit_toolbar(ui);
        ui.add(egui::Label::new(RichText::new(&node.name).strong()).truncate());
        ui.horizontal_wrapped(|ui| {
            ui.weak(node.kind.label());
            if ui.button("导出当前字节").clicked() {
                self.error = self
                    .worker
                    .export(document, self.node)
                    .err()
                    .unwrap_or_default();
            }
        });
        let location = format!(
            "b{} · 0x{:08X} · {} B",
            node.buffer,
            node.range.start,
            node.range.len()
        );
        ui.add(egui::Label::new(RichText::new(&location).monospace()).truncate());
        if let Some(error) = &node.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        self.inspector_fields(ui, document, node);
        let hex_id = ui.make_persistent_id("resource-hex");
        let hex = egui::CollapsingHeader::new("十六进制")
            .id_salt("resource-hex")
            .show(ui, |ui| {
                if ui
                    .checkbox(&mut self.hex_buffer, "查看整个数据层（含目录字段）")
                    .changed()
                {
                    self.hex_start = 0;
                }
                let range = if self.hex_buffer {
                    0..document.buffers[node.buffer].len()
                } else {
                    node.range.clone()
                };
                if let Some(bytes) = document
                    .buffers
                    .get(node.buffer)
                    .and_then(|bytes| bytes.get(range.clone()))
                {
                    self.hex_start = self.hex_start.min(bytes.len().saturating_sub(1) / 16 * 16);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(self.hex_start > 0, egui::Button::new("上一页"))
                            .clicked()
                        {
                            self.hex_start = self.hex_start.saturating_sub(256);
                        }
                        if ui
                            .add_enabled(
                                self.hex_start + 256 < bytes.len(),
                                egui::Button::new("下一页"),
                            )
                            .clicked()
                        {
                            self.hex_start += 256;
                        }
                        ui.small(format!("+0x{:X}", self.hex_start));
                    });
                    egui::ScrollArea::both()
                        .id_salt("workbench-hex")
                        .max_width(ui.available_width())
                        .auto_shrink([false, false])
                        .max_height(140.0)
                        .show(ui, |ui| {
                            for (row, bytes) in bytes
                                [self.hex_start..bytes.len().min(self.hex_start + 256)]
                                .chunks(16)
                                .enumerate()
                            {
                                let hex = bytes
                                    .iter()
                                    .map(|byte| format!("{byte:02X}"))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                let ascii: String = bytes
                                    .iter()
                                    .map(|&byte| {
                                        if byte.is_ascii_graphic() || byte == b' ' {
                                            byte as char
                                        } else {
                                            '.'
                                        }
                                    })
                                    .collect();
                                let offset = range.start + self.hex_start + 16 * row;
                                let mut text =
                                    RichText::new(format!("{offset:08X}  {hex:47}  {ascii}"))
                                        .monospace();
                                if self.hex_selection.as_ref().is_some_and(|selected| {
                                    selected.start < offset + bytes.len() && selected.end > offset
                                }) {
                                    text = text.background_color(ui.visuals().selection.bg_fill);
                                }
                                if ui
                                    .add(egui::Label::new(text).sense(egui::Sense::click()))
                                    .clicked()
                                {
                                    self.select_bytes(
                                        document,
                                        node.buffer,
                                        offset..offset + bytes.len(),
                                    );
                                }
                            }
                        });
                }
                self.byte_editor(ui, document, node);
            });
        if ui
            .ctx()
            .data_mut(|data| data.remove_temp::<bool>(hex_id.with("reveal")))
            .unwrap_or(false)
        {
            hex.header_response.scroll_to_me(Some(egui::Align::Min));
        }
    }

    pub(super) fn inspector_fields(&mut self, ui: &mut egui::Ui, document: &Document, node: &Node) {
        // A click outside an editor dismisses it. It must not also activate
        // the field underneath and issue a scroll to the raw-byte inspector.
        let editor_open = egui::Popup::is_any_open(ui.ctx()) || ui.ctx().text_edit_focused();
        let dismiss_id = ui.id().with("dismiss-field-editor");
        let (pressed, released) = ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_released(),
            )
        });
        // Focus can disappear on press, before the label sees a click on
        // release. Remember the purpose of the entire pointer gesture.
        let dismissing_editor = ui.data_mut(|data| {
            if pressed {
                data.insert_temp(dismiss_id, editor_open);
            }
            let dismissing = data.get_temp::<bool>(dismiss_id).unwrap_or(false);
            if released {
                data.remove::<bool>(dismiss_id);
            }
            editor_open || dismissing
        });
        let key = crate::edit::node_key(document, self.node);
        let width = ui.available_width().max(0.0);
        let height = ui
            .spacing()
            .interact_size
            .y
            .max(ui.text_style_height(&egui::TextStyle::Body))
            + 6.0;
        let name_width = (width * 0.42).min(160.0);
        let old_spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, field) in node.fields.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            if index % 2 == 1 {
                ui.painter()
                    .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
            }
            let row = rect.shrink2(egui::vec2(4.0, 3.0));
            let mut name_rect = egui::Rect::from_min_max(
                row.min,
                egui::pos2(
                    (row.left() + name_width - 8.0).max(row.left()),
                    row.bottom(),
                ),
            );
            let value_rect =
                egui::Rect::from_min_max(egui::pos2(row.left() + name_width, row.top()), row.max);
            let id = ui.id().with(("inspector-field", &self.path, &key, index));
            // Status uses the name column so an invalid value never displaces
            // its editor or hides the button needed to correct it.
            let mut status_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id(id.with("status"))
                    .max_rect(name_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            status_ui.set_clip_rect(name_rect.intersect(ui.clip_rect()));
            let mut value_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id(id.with("value"))
                    .max_rect(value_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            value_ui.set_clip_rect(value_rect.intersect(ui.clip_rect()));
            self.field_row(
                &mut value_ui,
                &mut status_ui,
                document,
                key.as_ref(),
                index,
                field,
            );
            name_rect.max.x = status_ui
                .available_rect_before_wrap()
                .right()
                .max(name_rect.left());
            let mut name_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id(id.with("name"))
                    .max_rect(name_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            name_ui.set_clip_rect(name_rect.intersect(ui.clip_rect()));
            if name_ui
                .add_sized(
                    name_rect.size(),
                    egui::Label::new(&field.name)
                        .truncate()
                        .show_tooltip_when_elided(false)
                        .sense(egui::Sense::click()),
                )
                .on_hover_text(format!(
                    "{}\nb{} · 0x{:08X} · {} 字节\n单击定位字节",
                    field.name,
                    field.binding.buffer,
                    field.binding.range.start,
                    field.binding.range.len()
                ))
                .clicked()
                && !dismissing_editor
            {
                self.hex_buffer = true;
                self.hex_start = field.binding.range.start / 16 * 16;
                self.hex_selection = Some(field.binding.range.clone());
                let id = ui.make_persistent_id("resource-hex");
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    true,
                );
                state.set_open(true);
                state.store(ui.ctx());
                ui.ctx()
                    .data_mut(|data| data.insert_temp(id.with("reveal"), true));
            }
        }
        ui.spacing_mut().item_spacing.y = old_spacing;
    }
}
