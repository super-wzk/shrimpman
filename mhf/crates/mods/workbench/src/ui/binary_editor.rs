//! A detached byte grid. Incomplete cells remain visible and block field writes.

use super::single_line_editor;
use crate::field::Binding;
use egui::{Align, FontId, Layout, Rect, Response, Sense, Ui, UiBuilder, pos2, vec2};
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    fmt::Write,
    hash::{Hash, Hasher},
};

#[derive(Clone, Default)]
struct State {
    open: bool,
    values: Vec<u8>,
    drafts: BTreeMap<usize, String>,
    published: Option<u64>,
    paste: String,
    error: String,
}

/// One set of columns and font metrics serves the header and every data row.
struct GridLayout {
    font: FontId,
    text_height: f32,
    row_height: f32,
    columns: usize,
    hex_start: f32,
    hex_width: f32,
    hex_stride: f32,
    ascii_start: f32,
    ascii_stride: f32,
    width: f32,
}

impl GridLayout {
    const INPUT_MARGIN: i8 = 2;

    fn new(ui: &mut Ui, count: usize) -> Self {
        let font = FontId::monospace(egui::TextStyle::Monospace.resolve(ui.style()).size);
        let (advance, height) =
            ui.fonts_mut(|fonts| (fonts.glyph_width(&font, '0'), fonts.row_height(&font)));
        let text_height = height + ui.spacing().extra_text_line_spacing;
        let gap = ui.spacing().item_spacing.x;
        let hex_start = advance * 8.0 + gap * 2.0;
        let hex_width = (advance * 2.0 + f32::from(Self::INPUT_MARGIN) * 2.0).ceil();
        let hex_stride = hex_width + gap;
        let columns =
            (if hex_start + 16.0 * hex_stride + gap + 16.0 * advance <= ui.available_width() {
                16
            } else {
                8
            })
            .min(count.max(1));
        let ascii_start = hex_start + columns as f32 * hex_stride + gap;
        Self {
            font,
            text_height,
            row_height: ui.spacing().interact_size.y.max(text_height),
            columns,
            hex_start,
            hex_width,
            hex_stride,
            ascii_start,
            ascii_stride: advance,
            width: ascii_start + columns.max(5) as f32 * advance,
        }
    }

    fn text_top(&self, row: Rect) -> f32 {
        row.center().y - self.text_height / 2.0
    }

    fn byte_rect(&self, row: Rect, column: usize) -> Rect {
        Rect::from_min_size(
            pos2(
                row.left() + self.hex_start + column as f32 * self.hex_stride,
                self.text_top(row),
            ),
            vec2(self.hex_width, self.text_height),
        )
    }

    fn paint(&self, ui: &Ui, row: Rect, x: f32, text: String, color: egui::Color32) {
        let mut job = egui::text::LayoutJob::simple_singleline(text, self.font.clone(), color);
        job.keep_trailing_whitespace = true;
        for section in &mut job.sections {
            section.format.line_height = Some(self.text_height);
        }
        let galley = ui.painter().layout_job(job);
        ui.painter()
            .with_clip_rect(row.intersect(ui.clip_rect()))
            .galley(pos2(row.left() + x, self.text_top(row)), galley, color);
    }
}

pub(super) fn show(
    ui: &mut Ui,
    button: &Response,
    binding: &Binding,
    original: &[u8],
    text: &mut String,
) -> bool {
    let id = ui.id().with("binary-editor");
    let mut state = ui
        .data_mut(|data| data.remove_temp::<State>(id))
        .unwrap_or_default();
    state.open |= button.clicked();
    if !state.open {
        ui.data_mut(|data| data.insert_temp(id, state));
        return false;
    }
    let observed = fingerprint(text);
    if state.published != Some(observed) {
        state.values = binding
            .encode(original, text)
            .unwrap_or_else(|_| original.to_vec());
        state.drafts.clear();
        state.published = Some(observed);
        state.error.clear();
    }
    let style = ui.style().clone();
    let frame = egui::Frame::popup(&style).inner_margin(8.0);
    let margin = frame.total_margin().sum();
    let screen = ui.ctx().content_rect().size();
    let size = vec2((screen.x * 0.9).min(700.0), (screen.y * 0.9).min(480.0));
    let mut changed = false;
    let mut close = false;
    let modal = egui::Modal::new(id).frame(frame).show(ui.ctx(), |ui| {
        ui.set_style(style);
        ui.set_width((size.x - margin.x).max(1.0));
        let row_height = ui.spacing().interact_size.y;
        ui.add(egui::Label::new(egui::RichText::new("二进制编辑").strong()).truncate());
        ui.add(
            egui::Label::new(format!(
                "b{} · 偏移 0x{:08X} · {} 字节",
                binding.buffer,
                binding.range.start,
                state.values.len()
            ))
            .truncate(),
        );
        let remaining = size.y - margin.y - 7.0 * row_height - 8.0 * ui.spacing().item_spacing.y;
        let layout = GridLayout::new(ui, state.values.len());
        egui::ScrollArea::both()
            .max_height(remaining.max(row_height))
            .auto_shrink([false, false])
            .show_rows(
                ui,
                layout.row_height,
                state.values.len().div_ceil(layout.columns) + 1,
                |ui, rows| {
                    changed |= grid(ui, binding.range.start, &mut state, &layout, rows);
                },
            );
        ui.add(
            egui::Label::new(if state.drafts.is_empty() {
                "完整字节实时更新；Tab 切换，Esc 关闭。".to_owned()
            } else {
                format!(
                    "{} 个未完成输入阻止预览与打包；Esc 取消当前格。",
                    state.drafts.len()
                )
            })
            .truncate(),
        );
        egui::ScrollArea::vertical()
            .max_height(row_height * 2.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut state.paste)
                        .hint_text("粘贴等长十六进制内容")
                        .desired_rows(2)
                        .desired_width(ui.available_width()),
                );
            });
        ui.horizontal(|ui| {
            if ui.button("粘贴覆盖").clicked() {
                match binding.encode(original, &state.paste) {
                    Ok(bytes) => {
                        state.values = bytes;
                        state.drafts.clear();
                        state.error.clear();
                        changed = true;
                    }
                    Err(error) => state.error = error,
                }
            }
            if ui
                .button("关闭")
                .on_hover_text("保留已生效修改；未完成输入不会写入，重新打开可继续修正。")
                .clicked()
            {
                close = true;
            }
        });
        if !state.error.is_empty() {
            ui.add(
                egui::Label::new(egui::RichText::new(&state.error).color(egui::Color32::LIGHT_RED))
                    .truncate(),
            );
        }
    });
    if changed {
        *text = serialize(&state.values, &state.drafts);
        state.published = Some(fingerprint(text));
    }
    state.open &= !close && !modal.should_close();
    ui.data_mut(|data| data.insert_temp(id, state));
    changed
}

fn grid(
    ui: &mut Ui,
    base: usize,
    state: &mut State,
    layout: &GridLayout,
    rows: std::ops::Range<usize>,
) -> bool {
    let mut changed = false;
    // Clip and scroll the grid itself, never enlarge the modal or owner row.
    for row in rows {
        let (rect, _) =
            ui.allocate_exact_size(vec2(layout.width, layout.row_height), Sense::hover());
        if row == 0 {
            for (x, label) in [
                (0.0, "Offset"),
                (
                    layout.hex_start + f32::from(GridLayout::INPUT_MARGIN),
                    "HEX",
                ),
                (layout.ascii_start, "ASCII"),
            ] {
                layout.paint(ui, rect, x, label.into(), ui.visuals().weak_text_color());
            }
            continue;
        }
        let first = (row - 1) * layout.columns;
        let end = (first + layout.columns).min(state.values.len());
        layout.paint(
            ui,
            rect,
            0.0,
            format!("{:08X}", base + first),
            ui.visuals().text_color(),
        );
        for index in first..end {
            let mut input = state
                .drafts
                .get(&index)
                .cloned()
                .unwrap_or_else(|| format!("{:02X}", state.values[index]));
            let cell = layout.byte_rect(rect, index - first);
            let mut cell_ui = ui.new_child(
                UiBuilder::new()
                    .id_salt(index)
                    .max_rect(cell)
                    .layout(Layout::left_to_right(Align::Min)),
            );
            cell_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            cell_ui.spacing_mut().item_spacing.x = 0.0;
            let response = cell_ui.add(
                single_line_editor(&mut input)
                    .font(layout.font.clone())
                    .frame(
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(GridLayout::INPUT_MARGIN, 0)),
                    )
                    .desired_width(layout.hex_width),
            );
            if (response.clicked() || response.gained_focus())
                && let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), response.id)
            {
                edit.cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(input.chars().count()),
                    )));
                edit.store(ui.ctx(), response.id);
            }
            if response.changed() {
                if input.len() == 2 && input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    state.values[index] = u8::from_str_radix(&input, 16).unwrap();
                    state.drafts.remove(&index);
                } else {
                    state.drafts.insert(index, input);
                }
                changed = true;
            }
            if (response.has_focus()
                || ui.memory(|memory| memory.had_focus_last_frame(response.id)))
                && state.drafts.contains_key(&index)
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                state.drafts.remove(&index);
                changed = true;
            }
            if state.drafts.contains_key(&index) {
                cell_ui.painter().rect_stroke(
                    cell,
                    2.0,
                    egui::Stroke::new(1.0, egui::Color32::LIGHT_RED),
                    egui::StrokeKind::Inside,
                );
            }
        }
        for (column, &byte) in state.values[first..end].iter().enumerate() {
            let ascii = if (0x20..=0x7e).contains(&byte) {
                char::from(byte)
            } else {
                '.'
            };
            layout.paint(
                ui,
                rect,
                layout.ascii_start + column as f32 * layout.ascii_stride,
                ascii.to_string(),
                ui.visuals().text_color(),
            );
        }
    }
    changed
}

fn serialize(values: &[u8], drafts: &BTreeMap<usize, String>) -> String {
    let mut text = String::with_capacity(values.len().saturating_mul(3));
    for (index, byte) in values.iter().enumerate() {
        if index != 0 {
            text.push(' ');
        }
        if let Some(draft) = drafts.get(&index) {
            text.push('?');
            text.push_str(draft);
        } else {
            let _ = write!(text, "{byte:02X}");
        }
    }
    text
}

fn fingerprint(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
