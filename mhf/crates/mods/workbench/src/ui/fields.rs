//! Compact field rows with detached editors for values that need more space.

use crate::field::{Binding, FieldType, ScalarType};
use egui::{Align, Layout, Rect, Response, Sense, Ui, UiBuilder, vec2};
use std::borrow::Cow;

#[path = "binary_editor.rs"]
mod binary_editor;

/// The parent owns one fixed row; expanded editors live in independent Areas.
pub(super) fn input(ui: &mut Ui, binding: &Binding, original: &[u8], text: &mut String) -> bool {
    let height = ui
        .spacing()
        .interact_size
        .y
        .max(ui.text_style_height(&egui::TextStyle::Body));
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(1.0), height), Sense::hover());
    let mut row = bounded(ui, rect, "value");
    match binding.format {
        FieldType::Scalar(scalar) => scalar_input(&mut row, scalar, text),
        FieldType::Array(scalar) => array_input(
            &mut row,
            rect,
            scalar,
            binding.range.len() / scalar.size(),
            text,
        ),
        FieldType::Flags(scalar) => flags_input(&mut row, rect, binding, scalar, original, text),
        FieldType::Color { alpha } => color_input(&mut row, rect, binding, alpha, original, text),
        FieldType::Text { .. } => text_input(&mut row, rect, binding, text),
        FieldType::Bytes => {
            let (mut editor, button) = trailing_button(&mut row, rect, "编辑");
            summary(&mut editor, text.chars().take(80).collect());
            binary_editor::show(&mut row, &button, binding, original, text)
        }
        FieldType::ReadOnly => false,
    }
}

fn bounded(ui: &mut Ui, rect: Rect, salt: impl egui::AsIdSalt) -> Ui {
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(salt)
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    child
}

fn trailing_button(ui: &mut Ui, rect: Rect, label: &str) -> (Ui, Response) {
    // Let the actual button lay itself out against the right edge first.
    // Its response is authoritative for the remaining editor width.
    let mut button = ui.new_child(
        UiBuilder::new()
            .id_salt("expand")
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    button.set_clip_rect(ui.clip_rect().intersect(rect));
    let response = button.add(egui::Button::new(label).truncate());
    let button_rect = response.rect;
    let editor_rect = Rect::from_min_max(
        rect.min,
        egui::pos2((button_rect.left() - 4.0).max(rect.left()), rect.bottom()),
    );
    (bounded(ui, editor_rect, "inline"), response)
}

/// Width and height include the popup frame, independently of the value row.
fn popup(
    parent: &Ui,
    anchor: &Response,
    title: &str,
    contents: impl FnOnce(&mut Ui, f32) -> bool,
) -> bool {
    let screen = anchor.ctx.content_rect();
    let width = 320.0_f32.min((screen.width() - 24.0).max(24.0));
    let height = 240.0_f32.min((screen.height() - 24.0).max(24.0));
    let style = parent.style().clone();
    let frame = egui::Frame::popup(&style).inner_margin(8.0);
    let margin = frame.total_margin().sum();
    egui::Popup::from_toggle_button_response(anchor)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .layout(Layout::top_down(Align::Min))
        .frame(frame)
        .width(width)
        .show(|ui| {
            ui.set_style(style);
            ui.set_width((width - margin.x).max(1.0));
            let heading_height = ui.spacing().interact_size.y;
            ui.add_sized(
                vec2(ui.available_width(), heading_height),
                egui::Label::new(egui::RichText::new(title).strong()).truncate(),
            );
            contents(
                ui,
                (height - margin.y - heading_height - ui.spacing().item_spacing.y).max(1.0),
            )
        })
        .is_some_and(|output| output.inner)
}

fn array_input(
    ui: &mut Ui,
    rect: Rect,
    scalar: ScalarType,
    count: usize,
    text: &mut String,
) -> bool {
    if count != 0 && count <= 4 && (rect.width() - 4.0 * (count - 1) as f32) / count as f32 >= 48.0
    {
        let mut values: Vec<_> = text
            .split(',')
            .map(|value| Cow::Borrowed(value.trim()))
            .collect();
        if values.len() == count {
            let width = (rect.width() - 4.0 * (count - 1) as f32) / count as f32;
            let mut changed = false;
            for (index, value) in values.iter_mut().enumerate() {
                let cell = Rect::from_min_size(
                    rect.min + vec2((width + 4.0) * index as f32, 0.0),
                    vec2(width, rect.height()),
                );
                changed |= scalar_input(&mut bounded(ui, cell, index), scalar, value.to_mut());
            }
            if changed {
                *text = values.join(", ");
            }
            return changed;
        }
    }
    let (mut editor, button) = trailing_button(ui, rect, "展开");
    let mut changed = if count <= 4 {
        single_line(&mut editor, text).changed()
    } else {
        let first = text.split(',').take(3).collect::<Vec<_>>().join(", ");
        summary(&mut editor, format!("{count} 项 · {first}"));
        false
    };
    changed |= popup(ui, &button, "数组元素", |ui, max_height| {
        let mut values: Vec<_> = text
            .split(',')
            .map(|value| Cow::Borrowed(value.trim()))
            .collect();
        let mut changed = false;
        let height = ui.spacing().interact_size.y;
        // Only visible element controls are created for large geometry arrays.
        egui::ScrollArea::vertical()
            .max_height(max_height)
            .auto_shrink([false, true])
            .show_rows(ui, height, values.len(), |ui, rows| {
                for index in rows {
                    let (row, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
                    let label_rect =
                        Rect::from_min_size(row.min, vec2(38.0_f32.min(row.width()), height));
                    summary(
                        &mut bounded(ui, label_rect, (index, "index")),
                        index.to_string(),
                    );
                    let value_rect = Rect::from_min_max(
                        egui::pos2((label_rect.right() + 4.0).min(row.right()), row.top()),
                        row.max,
                    );
                    changed |= scalar_input(
                        &mut bounded(ui, value_rect, (index, "value")),
                        scalar,
                        values[index].to_mut(),
                    );
                }
            });
        if changed {
            *text = values.join(", ");
        }
        changed
    });
    changed
}

fn flags_input(
    ui: &mut Ui,
    rect: Rect,
    binding: &Binding,
    scalar: ScalarType,
    original: &[u8],
    text: &mut String,
) -> bool {
    let (mut editor, button) = trailing_button(ui, rect, "位");
    let mut changed = single_line(&mut editor, text).changed();
    changed |= popup(ui, &button, "位标志", |ui, max_height| {
        let Ok(bytes) = binding.encode(original, text) else {
            ui.weak("请输入有效的位标志值");
            return false;
        };
        let number = Binding {
            format: FieldType::Scalar(scalar),
            ..binding.clone()
        }
        .decode(&bytes);
        let Some(mut value) = number.ok().and_then(|value| value.parse::<u64>().ok()) else {
            return false;
        };
        let mut changed = false;
        let height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .max_height(max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for first in (0..scalar.size() * 8).step_by(8) {
                    let (row, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
                    let width = (row.width() - 4.0 * 7.0).max(0.0) / 8.0;
                    for column in 0..8 {
                        let bit = first + column;
                        let cell = Rect::from_min_size(
                            row.min + vec2((width + 4.0) * column as f32, 0.0),
                            vec2(width, height),
                        );
                        let enabled = value & (1_u64 << bit) != 0;
                        if bounded(ui, cell, bit)
                            .add_sized(
                                cell.size(),
                                egui::Button::selectable(enabled, bit.to_string()),
                            )
                            .on_hover_text(format!(
                                "bit {bit} · {}",
                                if enabled { "启用" } else { "关闭" }
                            ))
                            .clicked()
                        {
                            value ^= 1_u64 << bit;
                            changed = true;
                        }
                    }
                }
            });
        if changed {
            *text = format!("0x{value:X}");
        }
        changed
    });
    changed
}

fn color_input(
    ui: &mut Ui,
    rect: Rect,
    binding: &Binding,
    alpha: bool,
    original: &[u8],
    text: &mut String,
) -> bool {
    let size = if alpha { 4 } else { 3 };
    let bytes = binding
        .encode(original, text)
        .unwrap_or_else(|_| original.to_vec());
    if bytes.len() != size {
        return single_line(ui, text).changed();
    }
    let mut rgba = [255; 4];
    rgba[..size].copy_from_slice(&bytes);
    let width = rect.height().min(rect.width());
    let swatch = Rect::from_min_size(rect.min, vec2(width, rect.height()));
    let mut color = bounded(ui, swatch, "swatch");
    let mut changed = if alpha {
        color
            .color_edit_button_srgba_unmultiplied(&mut rgba)
            .changed()
    } else {
        let mut rgb = [rgba[0], rgba[1], rgba[2]];
        let changed = color.color_edit_button_srgb(&mut rgb).changed();
        rgba[..3].copy_from_slice(&rgb);
        changed
    };
    if changed {
        *text = binding.decode(&rgba[..size]).unwrap();
    }
    let editor_rect = Rect::from_min_max(
        egui::pos2((swatch.right() + 4.0).min(rect.right()), rect.top()),
        rect.max,
    );
    changed |= single_line(&mut bounded(ui, editor_rect, "channels"), text).changed();
    changed
}

fn text_input(ui: &mut Ui, rect: Rect, binding: &Binding, text: &mut String) -> bool {
    let compact = !text.contains(['\n', '\r']) && binding.range.len() <= 96;
    let (mut editor, button) = trailing_button(ui, rect, "编辑");
    let mut changed = if compact {
        single_line(&mut editor, text).changed()
    } else {
        summary(
            &mut editor,
            text.lines().next().unwrap_or("").chars().take(80).collect(),
        );
        false
    };
    changed |= popup(ui, &button, "文本内容", |ui, max_height| {
        egui::ScrollArea::vertical()
            .max_height(max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(6)
                        .desired_width(ui.available_width()),
                )
                .changed()
            })
            .inner
    });
    changed
}

fn summary(ui: &mut Ui, value: String) {
    ui.add_sized(
        ui.available_size(),
        egui::Label::new(value).truncate().halign(Align::Min),
    );
}

fn scalar_input(ui: &mut egui::Ui, scalar: ScalarType, text: &mut String) -> bool {
    macro_rules! drag {
        ($ty:ty, $range:expr) => {{
            if let Ok(mut value) = text.parse::<$ty>() {
                let response = ui.add_sized(
                    ui.available_size(),
                    egui::DragValue::new(&mut value)
                        .range($range)
                        .speed(1.0)
                        .update_while_editing(true),
                );
                if response.changed() {
                    *text = value.to_string();
                }
                response.changed()
            } else {
                single_line(ui, text).changed()
            }
        }};
    }
    match scalar {
        ScalarType::U8 => drag!(u8, u8::MIN..=u8::MAX),
        ScalarType::U16 => drag!(u16, u16::MIN..=u16::MAX),
        ScalarType::U32 => drag!(u32, u32::MIN..=u32::MAX),
        ScalarType::U64 => single_line(ui, text).changed(),
        ScalarType::I8 => drag!(i8, i8::MIN..=i8::MAX),
        ScalarType::I16 => drag!(i16, i16::MIN..=i16::MAX),
        ScalarType::I32 => drag!(i32, i32::MIN..=i32::MAX),
        ScalarType::I64 => single_line(ui, text).changed(),
        ScalarType::F32 | ScalarType::F64 => {
            if let Ok(mut value) = text.parse::<f64>()
                && value.is_finite()
            {
                let response = ui.add_sized(
                    ui.available_size(),
                    egui::DragValue::new(&mut value)
                        .speed(0.01)
                        .update_while_editing(true),
                );
                if response.changed() {
                    *text = value.to_string();
                }
                response.changed()
            } else {
                single_line(ui, text).changed()
            }
        }
    }
}

fn single_line(ui: &mut Ui, text: &mut String) -> Response {
    ui.add_sized(
        ui.available_size(),
        single_line_editor(text).desired_width(ui.available_width()),
    )
}

/// Match DragValue's centered row contents, including focus and invalid drafts.
fn single_line_editor(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .vertical_align(Align::Center)
        .min_size(egui::Vec2::ZERO)
}
