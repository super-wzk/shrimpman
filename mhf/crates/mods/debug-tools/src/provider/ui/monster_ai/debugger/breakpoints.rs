use super::{DebuggerUi, format, send, session};
use crate::provider::{AiDebugOperation, AiDebugSnapshot, AiTarget, DebugControl, DebugSnapshot};
use egui_hunter::{Button, ButtonKind};

impl DebuggerUi {
    pub(in super::super) fn source_context(
        &mut self,
        ui: &mut egui::Ui,
        debug: &AiDebugSnapshot,
        control: &DebugControl,
        path: &str,
        line: usize,
    ) {
        ui.label(format!("{path}:{line}"));
        let indices = debug
            .breakpoints
            .iter()
            .enumerate()
            .filter_map(|(index, bp)| {
                let mhf_ai_debug::BreakpointKind::Location(pc) = bp.kind else {
                    return None;
                };
                if debug
                    .state
                    .pc
                    .is_some_and(|current| current.revision != pc.revision)
                {
                    return None;
                }
                debug
                    .debug_info
                    .lookup(pc.script as usize, pc.offset as usize)
                    .filter(|mapping| mapping.source.path == path && mapping.source.line == line)
                    .map(|_| index)
            })
            .collect::<Vec<_>>();
        if indices.is_empty() {
            if ui.button("添加行断点").clicked() {
                self.error = send(
                    control,
                    debug.target,
                    AiDebugOperation::SourceBreakpoint {
                        path: path.into(),
                        line,
                    },
                )
                .err();
                ui.close();
            }
        } else {
            for index in indices {
                ui.push_id(index, |ui| {
                    ui.weak(format!("断点 #{}", debug.breakpoints[index].id));
                    if let Some(updated) = condition_editor(ui, debug, index) {
                        self.error = send(
                            control,
                            debug.target,
                            AiDebugOperation::SetBreakpoints(updated),
                        )
                        .err();
                    }
                });
            }
        }
    }

    pub(in super::super) fn breakpoints(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
        target: Option<AiTarget>,
        active: bool,
    ) {
        let debug = session(snapshot, target);
        let Some(debug) = debug else {
            ui.weak("先附加到一个怪物实例，再设置断点。断点只作用于该实例。");
            return;
        };
        self.breakpoint_list(ui, debug, control, active);
        egui::CollapsingHeader::new("添加断点")
            .id_salt("ai-add-breakpoint")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (kind, label) in ["指令位置", "操作码"].iter().enumerate() {
                        ui.selectable_value(&mut self.breakpoint_kind, kind, *label);
                    }
                });
                ui.horizontal_wrapped(|ui| match self.breakpoint_kind {
                    0 => {
                        ui.label("脚本");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.breakpoint_script)
                                .desired_width(64.0)
                                .hint_text("0"),
                        );
                        ui.label("偏移");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.breakpoint_offset)
                                .desired_width(88.0)
                                .hint_text("0x00"),
                        );
                        if ui
                            .add_enabled(
                                debug.state.pc.is_some(),
                                Button::new("填入当前位置").kind(ButtonKind::Quiet),
                            )
                            .clicked()
                            && let Some(pc) = &debug.state.pc
                        {
                            self.breakpoint_script = pc.script.to_string();
                            self.breakpoint_offset = format!("0x{:X}", pc.offset);
                        }
                    }
                    1 => {
                        ui.label("操作码");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.breakpoint_opcode)
                                .desired_width(88.0)
                                .hint_text("0x05"),
                        );
                    }
                    _ => unreachable!(),
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.breakpoint_condition, "字段条件");
                    ui.add_enabled_ui(self.breakpoint_condition, |ui| {
                        field_picker(
                            ui,
                            &mut self.breakpoint_field,
                            &debug.state,
                            "condition-field",
                        );
                        ui.label("等于");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.breakpoint_value)
                                .desired_width(70.0)
                                .hint_text("0"),
                        );
                    });
                });
                if ui
                    .add_enabled(active && debug.attached, Button::new("添加断点"))
                    .clicked()
                {
                    match self.make_breakpoint(debug) {
                        Ok(breakpoint) => {
                            let mut breakpoints = debug.breakpoints.clone();
                            breakpoints.push(breakpoint);
                            self.error = send(
                                control,
                                debug.target,
                                AiDebugOperation::SetBreakpoints(breakpoints),
                            )
                            .err();
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                ui.separator();
            });
    }
}

fn field_picker(
    ui: &mut egui::Ui,
    selected: &mut String,
    state: &mhf_ai_debug::Snapshot,
    id: &str,
) {
    if selected.is_empty()
        && let Some(name) = state.fields.keys().next()
    {
        selected.clone_from(name);
    }
    egui::ComboBox::from_id_salt(id)
        .selected_text(if selected.is_empty() {
            "无已捕获字段"
        } else {
            selected.as_str()
        })
        .show_ui(ui, |ui| {
            for name in state.fields.keys() {
                ui.selectable_value(selected, name.clone(), name);
            }
        });
}

impl DebuggerUi {
    pub(super) fn make_breakpoint(
        &self,
        debug: &AiDebugSnapshot,
    ) -> Result<mhf_ai_debug::Breakpoint, String> {
        use mhf_ai_debug::{
            Breakpoint, BreakpointKind, Comparison, FieldPredicate, ProgramLocation,
        };
        let kind = match self.breakpoint_kind {
            0 => BreakpointKind::Location(ProgramLocation {
                revision: debug
                    .state
                    .pc
                    .ok_or("当前实例没有可绑定的指令位置")?
                    .revision,
                script: u32::try_from(unsigned(&self.breakpoint_script, "脚本编号")?)
                    .map_err(|_| "脚本编号超出范围")?,
                offset: u32::try_from(unsigned(&self.breakpoint_offset, "偏移")?)
                    .map_err(|_| "偏移超出范围")?,
            }),
            1 => BreakpointKind::Opcode(
                u8::try_from(unsigned(&self.breakpoint_opcode, "操作码")?)
                    .map_err(|_| "操作码必须在 0 至 255 之间")?,
            ),
            _ => return Err("未知断点类型".into()),
        };
        let condition = if self.breakpoint_condition {
            if !debug.state.fields.contains_key(&self.breakpoint_field) {
                return Err("请选择一个已捕获条件字段".into());
            }
            Some(FieldPredicate {
                field: self.breakpoint_field.clone(),
                comparison: Comparison::Equal,
                value: self
                    .breakpoint_value
                    .trim()
                    .parse()
                    .map_err(|_| "条件值必须是有符号整数")?,
            })
        } else {
            None
        };
        Ok(Breakpoint {
            id: debug
                .breakpoints
                .iter()
                .map(|breakpoint| breakpoint.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("断点编号已用尽")?,
            enabled: true,
            kind,
            condition,
        })
    }

    fn breakpoint_list(
        &mut self,
        ui: &mut egui::Ui,
        debug: &AiDebugSnapshot,
        control: &DebugControl,
        active: bool,
    ) {
        use mhf_ai_debug::BreakpointKind;
        if debug.breakpoints.is_empty() {
            ui.weak("尚未设置断点。");
            return;
        }
        let mut changed = None;
        egui::ScrollArea::vertical()
            .id_salt("ai-breakpoint-list")
            .max_height(240.0)
            .show(ui, |ui| {
                for (index, breakpoint) in debug.breakpoints.iter().enumerate() {
                    ui.push_id(breakpoint.id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let mut enabled = breakpoint.enabled;
                            if ui
                                .add_enabled(
                                    active && debug.attached,
                                    egui::Checkbox::new(
                                        &mut enabled,
                                        format!("#{}", breakpoint.id),
                                    ),
                                )
                                .changed()
                            {
                                let mut updated = debug.breakpoints.clone();
                                updated[index].enabled = enabled;
                                changed = Some(updated);
                            }
                            let text = match &breakpoint.kind {
                                BreakpointKind::Location(pc) => {
                                    let mut text = format::location(Some(pc));
                                    if debug
                                        .state
                                        .pc
                                        .is_some_and(|current| current.revision != pc.revision)
                                    {
                                        text.push_str(" · 旧版本，未绑定");
                                    } else if let Some(mapping) = debug
                                        .debug_info
                                        .lookup(pc.script as usize, pc.offset as usize)
                                    {
                                        text.push_str(&format!(
                                            " · {}:{}",
                                            mapping.source.path, mapping.source.line
                                        ));
                                    }
                                    text
                                }
                                BreakpointKind::Opcode(opcode) => format!("操作码 0x{opcode:02X}"),
                                BreakpointKind::FieldChanged(field) => format!("{field} 发生变化"),
                            };
                            ui.label(text).context_menu(|ui| {
                                ui.add_enabled_ui(active && debug.attached, |ui| {
                                    if let Some(updated) = condition_editor(ui, debug, index) {
                                        changed = Some(updated);
                                    }
                                });
                            });
                            if let Some(condition) = &breakpoint.condition {
                                ui.weak(format!(
                                    "条件 {} {:?} {}",
                                    condition.field, condition.comparison, condition.value
                                ));
                            }
                            if ui
                                .add_enabled(
                                    active && debug.attached,
                                    Button::new("删除").kind(ButtonKind::Quiet),
                                )
                                .clicked()
                            {
                                let mut updated = debug.breakpoints.clone();
                                updated.remove(index);
                                changed = Some(updated);
                            }
                        });
                    });
                }
            });
        if let Some(breakpoints) = changed
            && let Err(error) = send(
                control,
                debug.target,
                AiDebugOperation::SetBreakpoints(breakpoints),
            )
        {
            self.error = Some(error);
        }
    }
}

fn condition_editor(
    ui: &mut egui::Ui,
    debug: &AiDebugSnapshot,
    index: usize,
) -> Option<Vec<mhf_ai_debug::Breakpoint>> {
    use mhf_ai_debug::{Comparison, FieldPredicate};
    let breakpoint = &debug.breakpoints[index];
    let mut conditional = breakpoint.condition.is_some();
    let mut changed = ui.checkbox(&mut conditional, "字段条件").changed();
    let mut condition = breakpoint
        .condition
        .clone()
        .unwrap_or_else(|| FieldPredicate {
            field: debug
                .state
                .fields
                .keys()
                .next()
                .cloned()
                .unwrap_or_default(),
            comparison: Comparison::Equal,
            value: 0,
        });
    if conditional {
        let before = condition.clone();
        field_picker(
            ui,
            &mut condition.field,
            &debug.state,
            "edit-condition-field",
        );
        egui::ComboBox::from_id_salt("edit-comparison")
            .selected_text(format!("{:?}", condition.comparison))
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (Comparison::Equal, "等于"),
                    (Comparison::NotEqual, "不等于"),
                    (Comparison::Less, "小于"),
                    (Comparison::LessOrEqual, "小于等于"),
                    (Comparison::Greater, "大于"),
                    (Comparison::GreaterOrEqual, "大于等于"),
                    (Comparison::BitsSet, "位掩码"),
                ] {
                    ui.selectable_value(&mut condition.comparison, value, label);
                }
            });
        ui.add(egui::DragValue::new(&mut condition.value));
        changed |= before != condition;
    }
    if !changed || conditional && !debug.state.fields.contains_key(&condition.field) {
        return None;
    }
    let mut updated = debug.breakpoints.clone();
    updated[index].condition = conditional.then_some(condition);
    Some(updated)
}

fn unsigned(text: &str, name: &str) -> Result<u64, String> {
    let text = text.trim();
    let parsed = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        text.parse()
    };
    parsed.map_err(|_| format!("{name}必须是非负整数，可以使用 0x 十六进制。"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_addresses_accept_decimal_or_hex_and_reject_invalid_values() {
        assert_eq!(unsigned(" 0X18 ", "偏移").unwrap(), 24);
        assert_eq!(unsigned("24", "偏移").unwrap(), 24);
        assert!(unsigned("-1", "偏移").is_err());
        assert!(unsigned("1.5", "偏移").is_err());
        assert!(unsigned("18446744073709551616", "偏移").is_err());
    }
}
