use super::{DebuggerUi, format};
use mhf_ai_debug::Recording;

impl DebuggerUi {
    pub(super) fn selected_detail(&self, ui: &mut egui::Ui, recording: &Recording) {
        if let Some(entry) = recording
            .entries
            .iter()
            .find(|entry| Some(entry.sequence) == self.selected_event)
        {
            trace_detail(ui, entry, recording);
        } else {
            ui.weak("选择一条执行记录查看输入与变化");
        }
    }
}

pub(super) fn header(ui: &mut egui::Ui, recording: &Recording) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!("{} 条指令", recording.entries.len()));
        if recording.dropped != 0 {
            ui.weak(format!("前 {} 条已移出缓冲区", recording.dropped));
        }
    });
    if recording.entries.is_empty() {
        ui.weak("暂无执行记录；继续运行后可在这里查看每条指令。");
    }
}

pub(super) fn list(
    ui: &mut egui::Ui,
    recording: &Recording,
    selected_event: &mut Option<u64>,
    follow_latest: &mut bool,
) {
    let row_height = ui
        .text_style_height(&egui::TextStyle::Monospace)
        .max(ui.spacing().interact_size.y);
    egui::ScrollArea::both()
        .id_salt("ai-trace-list")
        .max_height(260.0)
        .stick_to_bottom(*follow_latest)
        .show_rows(ui, row_height, recording.entries.len(), |ui, range| {
            for index in range {
                let entry = &recording.entries[index];
                let pc = entry.pc_before.map_or_else(
                    || "—".into(),
                    |pc| format!("{}+{:04X}", pc.script, pc.offset),
                );
                let text = format!(
                    "#{:<4} {:<10} {} · {}",
                    entry.sequence,
                    pc,
                    outcome(entry.outcome),
                    if entry.note.is_empty() {
                        format!("0x{:02X}", entry.opcode)
                    } else {
                        entry.note.clone()
                    }
                );
                if ui
                    .add(
                        egui::Button::selectable(
                            *selected_event == Some(entry.sequence),
                            egui::RichText::new(text).monospace(),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    )
                    .clicked()
                {
                    *selected_event = Some(entry.sequence);
                    *follow_latest = false;
                }
            }
        });
}

pub(super) fn recorded_source(
    ui: &mut egui::Ui,
    recording: &Recording,
    pc: Option<mhf_ai_debug::ProgramLocation>,
) {
    let Some(pc) = pc else {
        ui.weak("此位置没有源码游标");
        return;
    };
    let Some(script) = recording
        .scripts
        .iter()
        .find(|script| script.revision == pc.revision && script.script == pc.script)
    else {
        ui.weak("录制中没有此版本的脚本");
        return;
    };
    let Some(source) = &script.source else {
        ui.weak("此脚本没有录制源码，可在指令面板检查字节码。");
        return;
    };
    let span = script
        .source_spans
        .iter()
        .find(|span| span.offset_start <= pc.offset && pc.offset < span.offset_end);
    ui.weak(span.map_or_else(
        || script.name.clone(),
        |span| format!("{}:{} · 录制版本 {}", span.path, span.line, pc.revision),
    ));
    let reveal_id = ui.id().with("recorded-source-position");
    let key = (recording.initial.instance, pc);
    let reveal = ui.data_mut(|data| {
        let changed = data
            .get_temp::<(mhf_ai_debug::InstanceId, mhf_ai_debug::ProgramLocation)>(reveal_id)
            != Some(key);
        data.insert_temp(reveal_id, key);
        changed
    });
    egui::ScrollArea::both()
        .id_salt("ai-recorded-workspace-source")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, line) in source.lines().enumerate() {
                let selected = span.is_some_and(|span| span.line as usize == index + 1);
                let text = egui::RichText::new(format!("{:>4}  {}", index + 1, line)).monospace();
                let response = ui.add(
                    egui::Label::new(if selected {
                        text.background_color(ui.visuals().selection.bg_fill)
                    } else {
                        text
                    })
                    .selectable(true),
                );
                if selected && reveal {
                    response.scroll_to_me(Some(egui::Align::Center));
                }
            }
        });
}

fn outcome(outcome: mhf_ai_debug::Outcome) -> &'static str {
    use mhf_ai_debug::Outcome;
    match outcome {
        Outcome::Continue => "继续",
        Outcome::Yield => "让出",
        Outcome::Reset => "复位",
        Outcome::Halt => "停止",
    }
}

fn trace_detail(ui: &mut egui::Ui, entry: &mhf_ai_debug::TraceEntry, recording: &Recording) {
    ui.strong(format!(
        "#{} · 操作码 0x{:02X} · {}",
        entry.sequence,
        entry.opcode,
        outcome(entry.outcome)
    ));
    ui.monospace(format::location(entry.pc_before.as_ref()));
    ui.weak(format!(
        "执行后：{}",
        format::location(entry.pc_after.as_ref())
    ));
    if !entry.operands.is_empty() {
        ui.monospace(format!("参数：{}", hex_bytes(&entry.operands)));
    }
    if !entry.note.is_empty() {
        ui.label(&entry.note);
    }
    ui.strong("本条指令字段变化");
    changes(ui, "instruction-changes", &entry.changes);
    if !entry.input.fields.is_empty()
        || entry.input.pc_before != entry.input.pc_after
        || entry.input.frame_before != entry.input.frame_after
    {
        ui.separator();
        ui.strong("指令间的外部输入");
        ui.weak(format!(
            "帧 {} → {}",
            entry.input.frame_before, entry.input.frame_after
        ));
        if entry.input.pc_before != entry.input.pc_after {
            ui.small(format!(
                "{} → {}",
                format::location(entry.input.pc_before.as_ref()),
                format::location(entry.input.pc_after.as_ref())
            ));
        }
        changes(ui, "external-changes", &entry.input.fields);
    }
    if let Some(pc) = entry.pc_before
        && let Some(script) = recording
            .scripts
            .iter()
            .find(|script| script.revision == pc.revision && script.script == pc.script)
    {
        egui::CollapsingHeader::new(format!("指令列表 · {}", script.name))
            .id_salt("ai-script-instructions")
            .show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("ai-script-bytecode")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        let mut offset = 0;
                        while offset < script.bytes.len() {
                            let bytes = &script.bytes[offset..];
                            let Ok(length) = mhf_monster::ai::bytecode::instruction_len(bytes)
                            else {
                                ui.weak(format!("+0x{offset:04X} · 无法解码剩余字节"));
                                break;
                            };
                            if length == 0 || length > bytes.len() {
                                break;
                            }
                            let text = format!("+0x{offset:04X}  {}", hex_bytes(&bytes[..length]));
                            ui.add(
                                egui::Button::selectable(
                                    offset == pc.offset as usize,
                                    egui::RichText::new(text).monospace(),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                            offset += length;
                        }
                    });
            });
    }
}

fn changes(ui: &mut egui::Ui, id: &str, changes: &[mhf_ai_debug::FieldChange]) {
    if changes.is_empty() {
        ui.weak("无已捕获字段变化");
        return;
    }
    egui::Grid::new(id)
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for change in changes {
                ui.monospace(&change.field);
                ui.monospace(
                    change
                        .before
                        .map_or_else(|| "—".into(), |value| value.to_string()),
                );
                ui.monospace(format!(
                    "→ {}",
                    change
                        .after
                        .map_or_else(|| "—".into(), |value| value.to_string())
                ));
                ui.end_row();
            }
        });
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
