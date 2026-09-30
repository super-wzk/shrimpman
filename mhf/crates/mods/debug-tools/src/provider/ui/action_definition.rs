use super::{Action, DebugSnapshot, NATIVE_WEAPON_NAMES};
use crate::provider::action_definition::{ActionEvent, motion_location};

fn event_label(event: &ActionEvent, weapon: u8) -> String {
    let label = event.label(weapon);
    if event.timing == 1 {
        label
    } else {
        format!("{} → {label}", event.when())
    }
}

pub(super) fn show(
    context: &egui::Context,
    snapshot: &DebugSnapshot,
    action: Action,
    open: &mut bool,
) {
    let bounds = context.content_rect();
    let viewport = bounds.shrink(8.0);
    egui::Window::new("招式定义")
        .id(egui::Id::new("debug-action-definition"))
        .open(open)
        .default_pos(bounds.min + egui::vec2(24.0, 24.0))
        .default_size(egui::vec2(620.0, 480.0).min(viewport.size()))
        .min_size(egui::vec2(260.0, 180.0).min(viewport.size()))
        .max_size(viewport.size())
        .resizable(true)
        .collapsible(false)
        .constrain_to(viewport)
        .show(context, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal_wrapped(|ui| {
                ui.strong(
                    NATIVE_WEAPON_NAMES
                        .get(usize::from(action.weapon))
                        .copied()
                        .unwrap_or("未知武器"),
                );
                ui.label(action.label());
            });
            let Some(definition) = snapshot
                .action_definition
                .as_deref()
                .filter(|definition| definition.action == action)
            else {
                ui.label("正在读取招式定义…");
                return;
            };
            let weapon = action.weapon;
            let data = match &definition.data {
                Ok(data) => data,
                Err(error) => {
                    ui.add(egui::Label::new(egui::RichText::new(error).weak()).wrap());
                    return;
                }
            };
            if data.steps.is_empty() && data.events.is_empty() {
                ui.add(
                    egui::Label::new("当前表中没有动作步骤或事件；原生代码中的定义尚未解析。")
                        .wrap(),
                );
                return;
            }
            ui.weak(format!(
                "{} 个步骤 · {} 个事件",
                data.steps.len(),
                data.events.len()
            ));
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("debug-action-definition-body")
                .auto_shrink([false, false])
                .max_height(ui.available_height().max(1.0))
                .content_margin(egui::Margin {
                    right: 8,
                    ..egui::Margin::ZERO
                })
                .show(ui, |ui| {
                    for (index, step) in data.steps.iter().enumerate() {
                        let [kind, value, arg_a, arg_b, frame, count] = step.0;
                        ui.group(|ui| {
                            ui.set_min_width(ui.available_width());
                            let label = match kind {
                                0 | 1 => "切换移动状态",
                                2 => "调用招式切换",
                                _ => "播放动画",
                            };
                            ui.strong(format!("步骤 {index} · {label}"));
                            let width = ui.available_width();
                            let columns = width >= 440.0;
                            let fields = |ui: &mut egui::Ui| {
                                match kind {
                                    0 | 1 => {
                                        field(ui, "状态参数", format!("{arg_a}, {arg_b}"), columns);
                                    }
                                    2 => {
                                        field(ui, "调用参数", value.to_string(), columns);
                                    }
                                    _ => {
                                        field(
                                            ui,
                                            "动画资源",
                                            motion_location(weapon, definition.motion_style, value),
                                            columns,
                                        )
                                        .on_hover_text(format!("原生动画编号 {value}"));
                                        field(
                                            ui,
                                            "动画参数",
                                            format!("{}, {}", arg_a as i16, arg_b as i16),
                                            columns,
                                        );
                                    }
                                }
                                let wait = match kind {
                                    3 => Some(format!("等待计数 {count}")),
                                    4 => Some(format!("等待帧条件 {frame}、计数 {count}")),
                                    5 => Some("等待原生状态切换".into()),
                                    _ => None,
                                };
                                if let Some(wait) = wait {
                                    field(ui, "等待条件", wait, columns);
                                }
                                for event in data
                                    .events
                                    .iter()
                                    .filter(|event| usize::from(event.step) == index)
                                {
                                    field(ui, "事件", event_label(event, weapon), columns);
                                }
                            };
                            if columns {
                                egui::Grid::new(("debug-action-step", index))
                                    .num_columns(2)
                                    .min_col_width(72.0)
                                    .max_col_width(width - 84.0)
                                    .spacing(egui::vec2(12.0, 4.0))
                                    .show(ui, fields);
                            } else {
                                fields(ui);
                            }
                        });
                    }
                    for event in data
                        .events
                        .iter()
                        .filter(|event| usize::from(event.step) >= data.steps.len())
                    {
                        ui.add(
                            egui::Label::new(format!(
                                "步骤 {} · {}",
                                event.step,
                                event_label(event, weapon)
                            ))
                            .wrap(),
                        );
                    }
                });
        });
}

fn field(ui: &mut egui::Ui, label: &str, value: String, columns: bool) -> egui::Response {
    ui.weak(label);
    let response = ui.add(egui::Label::new(value).wrap());
    if columns {
        ui.end_row();
    }
    response
}
