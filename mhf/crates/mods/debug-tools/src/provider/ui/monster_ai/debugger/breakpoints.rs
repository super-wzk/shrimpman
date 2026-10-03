use super::{DebuggerUi, format, send, session};
use crate::provider::{AiDebugOperation, AiDebugSnapshot, AiTarget, DebugControl, DebugSnapshot};
use egui_hunter::{Button, ButtonKind, Checkbox, Icon, IconButton, SelectField};

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
                debug
                    .breakpoint_mapping(bp)
                    .filter(|mapping| mapping.source.path == path && mapping.source.line == line)
                    .map(|_| index)
            })
            .collect::<Vec<_>>();
        if indices.is_empty() {
            if ui
                .add(
                    Button::new("添加行断点")
                        .kind(ButtonKind::Quiet)
                        .full_width(),
                )
                .clicked()
            {
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
                let names = ["指令位置", "操作码"];
                let mut kinds = SelectField::new(
                    egui::Id::new("ai-breakpoint-kind"),
                    names[self.breakpoint_kind],
                );
                kinds.native = kinds.native.width(180.0_f32.min(ui.available_width()));
                kinds.show_ui(ui, |ui| {
                    for (kind, name) in names.into_iter().enumerate() {
                        if ui
                            .add(
                                Button::new(name)
                                    .kind(ButtonKind::Quiet)
                                    .selected(self.breakpoint_kind == kind)
                                    .full_width(),
                            )
                            .clicked()
                        {
                            self.breakpoint_kind = kind;
                            ui.close();
                        }
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
                    ui.add(Checkbox::new(&mut self.breakpoint_condition, "字段条件"));
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
    let mut fields = SelectField::new(
        ui.id().with(("ai-breakpoint-field", id)),
        if selected.is_empty() {
            "无已捕获字段"
        } else {
            selected.as_str()
        },
    );
    fields.native = fields.native.width(200.0_f32.min(ui.available_width()));
    fields.show_ui(ui, |ui| {
        for name in state.fields.keys() {
            if ui
                .add(
                    Button::new(name)
                        .kind(ButtonKind::Quiet)
                        .selected(name == selected)
                        .full_width(),
                )
                .clicked()
            {
                selected.clone_from(name);
                ui.close();
            }
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
        if debug.breakpoints.is_empty() {
            ui.weak("尚未设置断点。");
            return;
        }
        let mut changed = None;
        for (index, breakpoint) in debug.breakpoints.iter().enumerate() {
            let (name, detail, stale) = breakpoint_text(debug, breakpoint);
            ui.scope_builder(
                egui::UiBuilder::new().id(egui::Id::new(("ai-breakpoint-row", breakpoint.id))),
                |ui| {
                    ui.horizontal(|ui| {
                        let mut enabled = breakpoint.enabled;
                        let toggle = ui
                            .add_enabled(active && debug.attached, Checkbox::new(&mut enabled, ""))
                            .on_hover_text(format!(
                                "{}断点 #{}",
                                if enabled { "停用" } else { "启用" },
                                breakpoint.id
                            ));
                        toggle.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                toggle.enabled(),
                                enabled,
                                format!("断点 {name}"),
                            )
                        });
                        if toggle.changed() {
                            let mut updated = debug.breakpoints.clone();
                            updated[index].enabled = enabled;
                            changed = Some(updated);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    active && debug.attached,
                                    IconButton::new(Icon::Trash, "删除断点")
                                        .id(egui::Id::new(("ai-breakpoint-delete", breakpoint.id)))
                                        .kind(ButtonKind::DangerQuiet),
                                )
                                .clicked()
                            {
                                let mut updated = debug.breakpoints.clone();
                                updated.remove(index);
                                changed = Some(updated);
                            }
                            if stale {
                                ui.label(egui::RichText::new("旧版").small().weak())
                                    .on_hover_text(&detail);
                            }
                            if breakpoint.condition.is_some() {
                                ui.label(
                                    egui::RichText::new("条件")
                                        .small()
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .on_hover_text(&detail);
                            }
                            ui.add_sized(
                                [ui.available_width().max(0.0), ui.spacing().interact_size.y],
                                egui::Label::new(name)
                                    .truncate()
                                    .halign(egui::Align::Min)
                                    .sense(egui::Sense::click())
                                    .show_tooltip_when_elided(false)
                                    .selectable(false),
                            )
                            .on_hover_text(&detail)
                            .context_menu(|ui| {
                                ui.add_enabled_ui(active && debug.attached, |ui| {
                                    if let Some(updated) = condition_editor(ui, debug, index) {
                                        changed = Some(updated);
                                    }
                                });
                            });
                        });
                    });
                },
            );
        }
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

fn breakpoint_text(
    debug: &AiDebugSnapshot,
    breakpoint: &mhf_ai_debug::Breakpoint,
) -> (String, String, bool) {
    use mhf_ai_debug::{BreakpointKind, Comparison};
    let mut detail = format!("断点 #{}", breakpoint.id);
    let (name, stale) = match &breakpoint.kind {
        BreakpointKind::Location(pc) => {
            let (mapping, stale) = match debug.state.pc {
                Some(current) if current.revision == pc.revision => (
                    debug
                        .debug_info
                        .lookup(pc.script as usize, pc.offset as usize),
                    false,
                ),
                Some(_) => (None, true),
                None => (None, false),
            };
            detail.push_str(&format!("\n{}", format::location(Some(pc))));
            if stale {
                detail.push_str("\n旧版本，未绑定到当前程序");
            } else if let Some(mapping) = mapping {
                detail.push_str(&format!(
                    "\n{}:{}",
                    mapping.source.path, mapping.source.line
                ));
            }
            let name = mapping.map_or_else(
                || format!("脚本 {} +0x{:04X}", pc.script, pc.offset),
                |mapping| {
                    format!(
                        "{}:{}",
                        mapping.source.path.rsplit(['/', '\\']).next().unwrap(),
                        mapping.source.line
                    )
                },
            );
            (name, stale)
        }
        BreakpointKind::Opcode(opcode) => {
            let name = format!("操作码 0x{opcode:02X}");
            detail.push('\n');
            detail.push_str(&name);
            (name, false)
        }
        BreakpointKind::FieldChanged(field) => {
            detail.push_str(&format!("\n{field} 发生变化时暂停"));
            (format!("字段 {field}"), false)
        }
    };
    if let Some(condition) = &breakpoint.condition {
        let operator = match condition.comparison {
            Comparison::Equal => "==",
            Comparison::NotEqual => "!=",
            Comparison::Less => "<",
            Comparison::LessOrEqual => "<=",
            Comparison::Greater => ">",
            Comparison::GreaterOrEqual => ">=",
            Comparison::BitsSet => "&",
        };
        detail.push_str(&format!(
            "\n条件：{} {operator} {}",
            condition.field, condition.value
        ));
        if condition.comparison == Comparison::BitsSet {
            detail.push_str(&format!(" == {}", condition.value));
        }
    }
    (name, detail, stale)
}

fn condition_editor(
    ui: &mut egui::Ui,
    debug: &AiDebugSnapshot,
    index: usize,
) -> Option<Vec<mhf_ai_debug::Breakpoint>> {
    use mhf_ai_debug::{Comparison, FieldPredicate};
    let breakpoint = &debug.breakpoints[index];
    let mut conditional = breakpoint.condition.is_some();
    let mut changed = ui
        .add(Checkbox::new(&mut conditional, "字段条件"))
        .changed();
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
        let operators = [
            (Comparison::Equal, "等于"),
            (Comparison::NotEqual, "不等于"),
            (Comparison::Less, "小于"),
            (Comparison::LessOrEqual, "小于等于"),
            (Comparison::Greater, "大于"),
            (Comparison::GreaterOrEqual, "大于等于"),
            (Comparison::BitsSet, "位掩码"),
        ];
        SelectField::new(
            ui.id().with("edit-comparison"),
            operators
                .iter()
                .find(|(value, _)| *value == condition.comparison)
                .unwrap()
                .1,
        )
        .show_ui(ui, |ui| {
            for (value, label) in operators {
                if ui
                    .add(
                        Button::new(label)
                            .kind(ButtonKind::Quiet)
                            .selected(condition.comparison == value)
                            .full_width(),
                    )
                    .clicked()
                {
                    condition.comparison = value;
                    ui.close();
                }
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
    use mhf_ai_debug::{
        Breakpoint, BreakpointKind, Comparison, FieldPredicate, ProgramLocation, Recording,
        Snapshot,
    };

    fn fixture() -> AiDebugSnapshot {
        let pc = ProgramLocation {
            revision: 2,
            script: 4,
            offset: 0,
        };
        let state = Snapshot {
            pc: Some(pc),
            fields: [("ai_state".into(), 4)].into_iter().collect(),
            ..Default::default()
        };
        AiDebugSnapshot {
            target: AiTarget {
                epoch: 1,
                pool: 0x1000,
                slot: 1,
                serial: 7,
                model: 0x2000,
                species: 6,
            },
            attached: true,
            paused: true,
            state: state.clone(),
            reason: String::new(),
            recording: Recording::empty(state),
            breakpoints: vec![Breakpoint {
                id: 12,
                enabled: true,
                kind: BreakpointKind::Location(pc),
                condition: Some(FieldPredicate {
                    field: "ai_state".into(),
                    comparison: Comparison::Equal,
                    value: 4,
                }),
            }],
            debug_info: mhf_monster::ai::dsl::DebugInfo {
                files: vec![],
                mappings: vec![mhf_monster::ai::dsl::SourceMapping {
                    script: 4,
                    start: 0,
                    end: 1,
                    generated: false,
                    source: mhf_monster::ai::dsl::SourceLocation {
                        path: "maps/97/163/main.mhai".into(),
                        line: 14,
                        ..Default::default()
                    },
                }],
            }
            .into(),
        }
    }

    #[test]
    fn names_are_short_and_hover_keeps_identity_condition_and_old_revision_status() {
        let mut debug = fixture();
        let breakpoint = &debug.breakpoints[0];
        let (name, detail, stale) = breakpoint_text(&debug, breakpoint);
        assert_eq!(name, "main.mhai:14");
        assert!(!stale);
        for value in [
            "断点 #12",
            "v2",
            "脚本 4 + 0x0000",
            "maps/97/163/main.mhai:14",
            "条件：ai_state == 4",
        ] {
            assert!(detail.contains(value), "{detail}");
        }
        let mut old = breakpoint.clone();
        old.kind = BreakpointKind::Location(ProgramLocation {
            revision: 1,
            script: 4,
            offset: 0,
        });
        let (name, detail, stale) = breakpoint_text(&debug, &old);
        assert_eq!(name, "脚本 4 +0x0000");
        assert!(stale);
        assert!(detail.contains("旧版本，未绑定"));
        assert!(!detail.contains("main.mhai"));
        old.kind = BreakpointKind::Opcode(5);
        assert_eq!(breakpoint_text(&debug, &old).0, "操作码 0x05");
        old.kind = BreakpointKind::FieldChanged("ai_state".into());
        assert_eq!(breakpoint_text(&debug, &old).0, "字段 ai_state");
        debug.state.pc = None;
        let (name, detail, stale) = breakpoint_text(&debug, &debug.breakpoints[0]);
        assert_eq!(name, "脚本 4 +0x0000");
        assert!(!stale);
        assert!(!detail.contains("main.mhai"));
    }

    #[test]
    fn narrow_rows_keep_delete_visible_and_toggle_delete_preserve_other_breakpoint_data() {
        for width in [200.0, 600.0] {
            let context = egui::Context::default();
            egui_hunter::Theme::default()
                .density(egui_hunter::Density::Compact)
                .apply(&context);
            let mut debugger = DebuggerUi::new();
            let mut debug = fixture();
            std::sync::Arc::make_mut(&mut debug.debug_info).mappings[0]
                .source
                .path = format!("maps/97/163/{}.mhai", "very_long_file_name_".repeat(4));
            let name = breakpoint_text(&debug, &debug.breakpoints[0]).0;
            let mut old = debug.breakpoints[0].clone();
            old.id = 13;
            old.kind = BreakpointKind::Location(ProgramLocation {
                revision: 1,
                script: 4,
                offset: 0,
            });
            debug.breakpoints.push(old);
            let control = DebugControl::new();
            let mut time = 0.0;
            let mut draw = |debug: &AiDebugSnapshot, events| {
                time += 0.1;
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 300.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ui, |ui| {
                                debugger.breakpoint_list(ui, debug, &control, true);
                            });
                    },
                );
                let texts = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.job.text.clone(),
                            text.galley.rect.translate(text.pos.to_vec2()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let events = output.platform_output.events.clone();
                output.drop_without_applying_deltas();
                (texts, events)
            };
            draw(&debug, vec![]);
            let (texts, _) = draw(&debug, vec![]);
            let row_id = egui::Id::new(("ai-breakpoint-row", 12_u64));
            let delete_id = egui::Id::new(("ai-breakpoint-delete", 12_u64));
            let row = context.read_response(row_id).unwrap().rect;
            let delete = context.read_response(delete_id).unwrap().rect;
            assert!(delete.right() <= width && row.contains_rect(delete));
            assert!(row.height() <= 25.0, "{row:?}");
            let old_row = context
                .read_response(egui::Id::new(("ai-breakpoint-row", 13_u64)))
                .unwrap()
                .rect;
            let old_delete = context
                .read_response(egui::Id::new(("ai-breakpoint-delete", 13_u64)))
                .unwrap()
                .rect;
            assert!(old_delete.right() <= width && old_row.contains_rect(old_delete));
            assert!(old_row.height() <= 25.0, "{old_row:?}");
            assert!(texts.iter().any(|(text, _)| text == &name));
            for badge in ["条件", "旧版"] {
                let (_, rect) = texts
                    .iter()
                    .find(|(text, rect)| text == badge && old_row.contains_rect(*rect))
                    .unwrap();
                assert!(rect.right() < old_delete.left(), "{badge}: {rect:?}");
            }
            let click = |pos, button, pressed| egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Default::default(),
            };
            let point = egui::pos2(row.left() + 40.0, row.center().y);
            draw(
                &debug,
                vec![
                    egui::Event::PointerMoved(point),
                    click(point, egui::PointerButton::Secondary, true),
                ],
            );
            draw(
                &debug,
                vec![click(point, egui::PointerButton::Secondary, false)],
            );
            let (texts, _) = draw(&debug, vec![]);
            for label in ["字段条件", "ai_state", "等于"] {
                assert!(texts.iter().any(|(text, _)| text == label), "{label}");
            }
            assert!(control.commands().is_empty());
            draw(
                &debug,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
            let point = egui::pos2(row.left() + 8.0, row.center().y);
            draw(
                &debug,
                vec![
                    egui::Event::PointerMoved(point),
                    click(point, egui::PointerButton::Primary, true),
                ],
            );
            let (_, events) = draw(
                &debug,
                vec![click(point, egui::PointerButton::Primary, false)],
            );
            assert!(events.iter().any(|event| {
                let info = event.widget_info();
                info.typ == egui::WidgetType::Checkbox
                    && info.label.as_deref() == Some(format!("断点 {name}").as_str())
                    && info.selected == Some(false)
                    && info.enabled
            }));
            let commands = control.commands();
            let [
                crate::provider::DebugCommand::AiDebug {
                    target,
                    operation: AiDebugOperation::SetBreakpoints(updated),
                },
            ] = commands.as_slice()
            else {
                panic!("missing toggle")
            };
            assert_eq!(*target, debug.target);
            assert!(!updated[0].enabled);
            assert_eq!(updated[0].condition, debug.breakpoints[0].condition);
            assert_eq!(updated[0].kind, debug.breakpoints[0].kind);
            assert_eq!(updated[1], debug.breakpoints[1]);
            debug.breakpoints = updated.clone();
            draw(&debug, vec![]);
            let point = context.read_response(delete_id).unwrap().rect.center();
            draw(
                &debug,
                vec![
                    egui::Event::PointerMoved(point),
                    click(point, egui::PointerButton::Primary, true),
                ],
            );
            draw(
                &debug,
                vec![click(point, egui::PointerButton::Primary, false)],
            );
            assert!(
                matches!(control.commands().as_slice(), [crate::provider::DebugCommand::AiDebug { target, operation: AiDebugOperation::SetBreakpoints(updated) }] if *target == debug.target && updated == &debug.breakpoints[1..])
            );
        }
    }

    #[test]
    fn breakpoint_addresses_accept_decimal_or_hex_and_reject_invalid_values() {
        assert_eq!(unsigned(" 0X18 ", "偏移").unwrap(), 24);
        assert_eq!(unsigned("24", "偏移").unwrap(), 24);
        assert!(unsigned("-1", "偏移").is_err());
        assert!(unsigned("1.5", "偏移").is_err());
        assert!(unsigned("18446744073709551616", "偏移").is_err());
    }
}
