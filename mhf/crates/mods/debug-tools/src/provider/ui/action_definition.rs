use super::{Action, DebugSnapshot, NATIVE_WEAPON_NAMES};
use crate::provider::action_definition::ActionDefinition;
use mhf_resource::action_definition::{ACTION_SIZE, Definition, NativeMotionRef};
use mhf_ui::resource_reference::ResourceReference;

fn event(
    ui: &mut egui::Ui,
    definition: &ActionDefinition,
    data: &Definition,
    index: usize,
    columns: bool,
) {
    let event = &data.events[index];
    let action = definition.action;
    let reference = event.attack_reference(action.weapon);
    let label = if reference.is_some() {
        "生成攻击".into()
    } else {
        format!("操作 {} · 参数 {}", event.operation, event.argument)
    };
    let label = match event.timing {
        1 => label,
        2 => format!("步骤结束后 → {label}"),
        _ if event.phase > 0 => format!("动作阶段 {} → {label}", event.phase),
        _ => format!("帧条件 {} · 计数 {} → {label}", event.frame, event.count),
    };
    let response = if usize::from(event.step) < data.steps.len() {
        field(ui, "事件", label, columns)
    } else {
        ui.add(egui::Label::new(format!("步骤 {} · {label}", event.step)).wrap())
    };
    response.on_hover_ui(|ui| {
        if let Some((path, span)) = data
            .event_path("mhfdat.bin", index)
            .zip(data.event_span(index))
        {
            ui.weak("来源");
            ResourceReference::new(path).source_range(span).show(ui);
        }
        ui.label(format!("所属步骤 {} · 时机 {}", event.step, event.timing));
        ui.label(format!(
            "阶段 {} · 帧条件 {} · 计数 {}",
            event.phase, event.frame, event.count
        ));
        ui.label(format!(
            "原始操作 {} · 参数 {}",
            event.operation, event.argument
        ));
    });
    let Some(reference) = reference else {
        return;
    };
    let target = definition
        .attacks
        .as_deref()
        .map(|directory| match directory {
            Ok(directory) => directory
                .resolve(reference)
                .map_err(|error| format!("mhfsdt.bin：{error}"))?
                .ok_or_else(|| {
                    let subtype = reference
                        .subtype
                        .map_or_else(String::new, |key| format!("、子类别键 {key}"));
                    format!(
                        "mhfsdt.bin 未包含类别键 {}{subtype} 的攻击参数表",
                        reference.category
                    )
                }),
            Err(error) => Err(error.clone()),
        });
    let widget = ResourceReference::new(reference).id(egui::Id::new((
        "debug-action-attack-resource",
        action.group,
        action.weapon,
        action.id,
        index,
    )));
    let widget = match target.as_ref() {
        Some(Ok(path)) => widget.resolved_path(path),
        Some(Err(error)) => widget.help(error),
        None => widget,
    };
    ui.weak("攻击资源");
    widget.show(ui);
    if columns {
        ui.end_row();
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
            ))
            .on_hover_ui(|ui| {
                if let Ok(path) = data.resource_path("mhfdat.bin") {
                    ui.weak("来源");
                    ResourceReference::new(path)
                        .source_range(data.offset..data.offset + ACTION_SIZE)
                        .show(ui);
                }
            });
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
                            ui.strong(format!("步骤 {index} · {label}"))
                                .on_hover_ui(|ui| {
                                    if let Some((path, span)) = data
                                        .step_path("mhfdat.bin", index)
                                        .zip(data.step_span(index))
                                    {
                                        ui.weak("来源");
                                        ResourceReference::new(path).source_range(span).show(ui);
                                    }
                                    ui.monospace(format!("原始 WORD：{:04X?}", step.0));
                                });
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
                                        let motion = NativeMotionRef {
                                            id: value,
                                            weapon: action.weapon,
                                            style: definition.motion_style,
                                        };
                                        ui.weak("动画资源");
                                        ResourceReference::new(motion)
                                            .id(egui::Id::new((
                                                "debug-action-motion-resource",
                                                action.group,
                                                action.weapon,
                                                action.id,
                                                index,
                                            )))
                                            .show(ui);
                                        if columns {
                                            ui.end_row();
                                        }
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
                                for (event_index, entry) in data.events.iter().enumerate() {
                                    if usize::from(entry.step) == index {
                                        event(ui, definition, data, event_index, columns);
                                    }
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
                    for (event_index, entry) in data.events.iter().enumerate() {
                        if usize::from(entry.step) >= data.steps.len() {
                            event(ui, definition, data, event_index, false);
                        }
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
