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
    position: egui::Pos2,
) -> Option<egui::Rect> {
    let viewport = context.content_rect().shrink(8.0);
    egui::Window::new(format!(
        "{} · {} · 定义",
        NATIVE_WEAPON_NAMES[usize::from(action.weapon)],
        action.label()
    ))
    .id(egui::Id::new("debug-action-definition"))
    .open(open)
    .default_pos(position)
    .default_width(430.0_f32.min(viewport.width()))
    .min_width(160.0_f32.min(viewport.width()))
    .max_width(viewport.width())
    .default_height(360.0_f32.min(viewport.height()))
    .max_height(viewport.height())
    .constrain_to(viewport)
    .vscroll(true)
    .show(context, |ui| {
        ui.set_min_width(ui.available_width());
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
                ui.weak(error);
                return;
            }
        };
        if data.steps.is_empty() && data.events.is_empty() {
            ui.weak("当前表中没有动作步骤或事件；原生代码中的定义尚未解析。");
            return;
        }
        for (index, step) in data.steps.iter().enumerate() {
            let [kind, value, arg_a, arg_b, frame, count] = step.0;
            ui.group(|ui| {
                ui.set_min_width(ui.available_width());
                let label = match kind {
                    0 | 1 => format!("切换移动状态 · 参数 {arg_a}, {arg_b}"),
                    2 => format!("调用招式切换 · 参数 {value}"),
                    _ => "播放动画".into(),
                };
                ui.label(format!("步骤 {index} · {label}"));
                if kind >= 3 {
                    ui.add(
                        egui::Label::new(motion_location(weapon, definition.motion_style, value))
                            .wrap(),
                    )
                    .on_hover_text(format!("原生动画编号 {value}"));
                    ui.weak(format!("参数 {}, {}", arg_a as i16, arg_b as i16));
                }
                match kind {
                    3 => {
                        ui.weak(format!("等待计数 {count}"));
                    }
                    4 => {
                        ui.weak(format!("等待帧条件 {frame}、计数 {count}"));
                    }
                    5 => {
                        ui.weak("等待原生状态切换");
                    }
                    _ => {}
                }
                for event in data
                    .events
                    .iter()
                    .filter(|event| usize::from(event.step) == index)
                {
                    ui.add(egui::Label::new(event_label(event, weapon)).wrap());
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
    })
    .map(|window| window.response.rect)
}
