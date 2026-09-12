//! Editing coordination: widgets produce validated patches, the worker rebuilds
//! once per batch, and only completed revisions replace the preview document.

use super::Workbench;
use crate::{
    edit::{self, NodeKey},
    field::{Binding, Field, FieldType},
    inspect::{Document, Kind, Node},
    preview::Command,
    session::Session,
    worker::Expanded,
};
use std::{
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    Field {
        node: NodeKey,
        index: usize,
        name: String,
    },
    Bytes {
        node: NodeKey,
        range: Range<usize>,
        owner_len: usize,
        kind: Kind,
    },
}

struct Input {
    target: Target,
    binding: Binding,
    original: Vec<u8>,
    text: String,
    revision: u64,
    pending: bool,
    error: String,
    conflicted: bool,
}

impl Input {
    fn new(target: Target, binding: Binding, document: &Document) -> Result<Self, String> {
        Ok(Self {
            original: binding.bytes(&document.buffers)?.to_vec(),
            text: binding.read(&document.buffers)?,
            target,
            binding,
            revision: 0,
            pending: false,
            error: String::new(),
            conflicted: false,
        })
    }

    fn change(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.pending = true;
        if !self.conflicted {
            self.error.clear();
        }
    }

    fn reset(&mut self, document: &Document) {
        let revision = self.revision.wrapping_add(1);
        match self
            .resolve(document)
            .and_then(|binding| Self::new(self.target.clone(), binding, document))
        {
            Ok(input) => {
                *self = input;
                self.revision = revision;
            }
            Err(error) => {
                self.error = error;
                self.pending = false;
                self.revision = revision;
                self.conflicted = true;
            }
        }
    }

    /// Resolve storage from its owner every time, including after a failed
    /// edit. Matching bytes at an old offset do not establish resource identity.
    fn resolve(&self, document: &Document) -> Result<Binding, String> {
        match &self.target {
            Target::Field { node, index, name } => edit::locate(document, node)
                .and_then(|node| document.nodes[node].fields.get(*index))
                .filter(|field| field.name == *name && field.writable)
                .map(|field| field.binding.clone())
                .ok_or_else(|| "数据结构已变化，原字段不再可编辑。".into()),
            Target::Bytes {
                node,
                range,
                owner_len,
                kind,
            } => {
                let owner = edit::locate(document, node)
                    .and_then(|node| document.nodes.get(node))
                    .filter(|node| node.kind == *kind && node.range.len() == *owner_len)
                    .ok_or("所属资源的结构或长度已变化，请重新定位字节后编辑。")?;
                Ok(Binding {
                    buffer: owner.buffer,
                    range: owner.range.start + range.start..owner.range.start + range.end,
                    ..self.binding.clone()
                })
            }
        }
    }
}

pub(super) struct Editing {
    pub sessions: BTreeMap<PathBuf, Session>,
    pub busy: bool,
    pub saving: bool,
    pub output: PathBuf,
    source_root: PathBuf,
    inputs: BTreeMap<PathBuf, Vec<Input>>,
    pending_preview: BTreeMap<PathBuf, Arc<Document>>,
    submitted: Vec<(usize, u64)>,
    last_input: Option<Instant>,
    last_dispatch: Instant,
    next_path: Option<PathBuf>,
    pack_after_edits: bool,
    raw: Option<usize>,
    raw_offset: usize,
    raw_length: usize,
    replacement: String,
    error: String,
}

impl Editing {
    pub fn new(root: &Path) -> Self {
        Self {
            sessions: BTreeMap::new(),
            busy: false,
            saving: false,
            output: root.parent().unwrap_or(root).join("dat-redirect"),
            source_root: root.into(),
            inputs: BTreeMap::new(),
            pending_preview: BTreeMap::new(),
            submitted: Vec::new(),
            last_input: None,
            last_dispatch: Instant::now(),
            next_path: None,
            pack_after_edits: false,
            raw: None,
            raw_offset: 0,
            raw_length: 16,
            replacement: String::new(),
            error: String::new(),
        }
    }

    fn pending(&self, path: &Path) -> bool {
        self.inputs
            .get(path)
            .is_some_and(|inputs| inputs.iter().any(|input| input.pending))
    }
}

impl Workbench {
    pub(crate) fn set_redirect_paths(&mut self, source_root: PathBuf, output: PathBuf) {
        self.editing.source_root = source_root;
        self.editing.output = output;
    }

    pub(super) fn open_document(&mut self, path: PathBuf) {
        if self.editing.busy
            || self
                .path
                .as_ref()
                .is_some_and(|path| self.editing.pending(path))
        {
            self.editing.next_path = Some(path);
            self.editing.last_input = None;
        } else {
            self.switch_document(path);
        }
    }

    fn switch_document(&mut self, path: PathBuf) {
        if let Some(previous) = &self.path
            && self.editing.sessions.get(previous).is_some_and(|session| {
                !session.dirty() && !session.can_undo() && !session.can_redo()
            })
            && !self.editing.pending(previous)
        {
            self.editing.sessions.remove(previous);
            self.editing.inputs.remove(previous);
        }
        self.request = self.request.wrapping_add(1);
        self.path = Some(path.clone());
        self.document = None;
        self.expanding = None;
        self.error.clear();
        self.resource_counts.clear();
        self.editing.raw = None;
        self.editing.error.clear();
        self.hex_start = 0;
        self.hex_selection = None;
        self.hex_buffer = false;
        if let Some(session) = self.editing.sessions.get(&path) {
            let document = session.document.clone();
            self.loading = false;
            self.loaded_document(document);
        } else {
            self.loading = true;
            self.worker.load(self.request, path);
        }
    }

    /// Widgets stay responsive while rebuilding. A newer input revision is
    /// retained and submitted against the completed document in the next batch.
    pub(super) fn flush_edits(&mut self, context: &egui::Context) {
        if self.editing.busy && !self.editing.pack_after_edits {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(document) = self.document.clone() else {
            return;
        };
        if let Some(last) = self.editing.last_input {
            let elapsed = if context.input(|input| input.pointer.any_down()) {
                self.editing.last_dispatch.elapsed()
            } else {
                last.elapsed()
            };
            let delay = Duration::from_millis(150).saturating_sub(elapsed);
            if !delay.is_zero()
                && !self.editing.pack_after_edits
                && self.editing.next_path.is_none()
            {
                context.request_repaint_after(delay);
                return;
            }
        }
        let mut patches = Vec::new();
        let mut submitted = Vec::new();
        if let Some(inputs) = self.editing.inputs.get_mut(&path) {
            for (index, input) in inputs
                .iter_mut()
                .enumerate()
                .filter(|(_, input)| input.pending && input.error.is_empty())
            {
                match input.resolve(&document).and_then(|binding| {
                    input.binding = binding;
                    input.binding.write(&document.buffers, &input.text)
                }) {
                    Ok(Some(patch)) => {
                        patches.push(patch);
                        submitted.push((index, input.revision));
                    }
                    Ok(None) if !self.editing.busy => input.pending = false,
                    Ok(None) => {}
                    Err(error) => input.error = error,
                }
            }
        }
        if self.editing.pack_after_edits {
            self.editing.pack_after_edits = false;
            if self.editing.inputs.get(&path).is_some_and(|inputs| {
                inputs
                    .iter()
                    .any(|input| input.pending && !input.error.is_empty())
            }) {
                self.editing.error = "有字段尚未通过校验，请修正或撤回该输入后打包。".into();
            } else {
                self.editing.saving = true;
                // The worker owns the exact requested snapshot, including
                // unprocessed inputs, so an accepted save also survives exit.
                self.worker.pack(
                    path.clone(),
                    self.editing.source_root.clone(),
                    self.editing.output.clone(),
                    document.clone(),
                    patches.clone(),
                );
            }
        }
        if self.editing.busy {
            return;
        }
        if !patches.is_empty() {
            self.request = self.request.wrapping_add(1);
            self.expanding = None;
            self.editing.busy = true;
            self.editing.last_dispatch = Instant::now();
            self.editing.submitted = submitted;
            self.editing.error.clear();
            self.worker.edit(self.request, document, patches);
            return;
        }
        if let Some(path) = self.editing.next_path.take() {
            self.switch_document(path);
        }
    }

    pub(super) fn finish_edit(&mut self, edited: Expanded) {
        if edited.request != self.request {
            return;
        }
        self.editing.busy = false;
        let submitted = std::mem::take(&mut self.editing.submitted);
        match edited.document {
            Ok(document) => {
                if let Some(path) = &self.path {
                    if let Some(session) = self.editing.sessions.get_mut(path) {
                        session.apply(document.clone());
                    }
                    if let Some(inputs) = self.editing.inputs.get_mut(path) {
                        for (index, input) in inputs.iter_mut().enumerate() {
                            let was_submitted =
                                submitted.iter().find(|(submitted, _)| *submitted == index);
                            if was_submitted
                                .is_some_and(|(_, revision)| *revision == input.revision)
                            {
                                input.pending = false;
                            }
                            match input.resolve(&document) {
                                Ok(binding) => input.binding = binding,
                                Err(error) => {
                                    input.error = error;
                                    continue;
                                }
                            }
                            if input.pending {
                                match input.binding.bytes(&document.buffers) {
                                    Ok(bytes)
                                        if was_submitted.is_some() || bytes == input.original =>
                                    {
                                        input.original = bytes.to_vec()
                                    }
                                    _ => {
                                        input.conflicted = true;
                                        input.error =
                                            "该字节范围同时被其他编辑修改，请撤回此输入后重新编辑。"
                                                .into()
                                    }
                                }
                            } else {
                                input.reset(&document);
                            }
                        }
                    }
                }
                self.refresh_preview(document);
            }
            Err(error) => {
                self.editing.error = error.clone();
                if let Some(inputs) = self
                    .path
                    .as_ref()
                    .and_then(|path| self.editing.inputs.get_mut(path))
                {
                    for (index, revision) in submitted {
                        if let Some(input) = inputs.get_mut(index)
                            && input.revision == revision
                        {
                            input.error = error.clone();
                        }
                    }
                }
            }
        }
    }

    fn refresh_preview(&mut self, document: Arc<Document>) {
        if let Some(path) = self.path.clone() {
            self.editing.pending_preview.insert(path, document.clone());
        }
        self.refresh_document(document);
        self.flush_previews();
    }

    pub(super) fn flush_previews(&mut self) {
        while let Some((path, document)) = self.editing.pending_preview.first_key_value() {
            let command = Command::RefreshDocument {
                path: path.clone(),
                document: document.clone(),
            };
            if let Err(error) = self.control.send(command) {
                self.error = error;
                break;
            }
            self.editing.pending_preview.pop_first();
        }
    }

    fn history(&mut self, redo: bool) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !redo && self.editing.pending(&path) {
            if let Some(inputs) = self.editing.inputs.get_mut(&path)
                && let Some(document) = &self.document
            {
                for input in inputs {
                    input.reset(document);
                }
            }
            self.editing.error.clear();
            return;
        }
        let Some(session) = self.editing.sessions.get_mut(&path) else {
            return;
        };
        if redo {
            session.redo();
        } else {
            session.undo();
        }
        let document = session.document.clone();
        self.request = self.request.wrapping_add(1);
        self.expanding = None;
        self.editing.inputs.remove(&path);
        self.editing.raw = None;
        self.editing.error.clear();
        self.refresh_preview(document);
    }

    pub(super) fn edit_toolbar(&mut self, ui: &mut egui::Ui) {
        let Some(path) = &self.path else {
            return;
        };
        let Some(session) = self.editing.sessions.get(path) else {
            return;
        };
        let pending = self.editing.pending(path);
        let (dirty, undo, redo) = (
            session.dirty() || pending,
            session.can_undo() || pending,
            session.can_redo(),
        );
        let ready = !self.editing.busy && !self.loading;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready && undo, egui::Button::new("撤销"))
                .clicked()
            {
                self.history(false);
            }
            if ui
                .add_enabled(ready && redo && !pending, egui::Button::new("重做"))
                .clicked()
            {
                self.history(true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.loading
                            && (!self.editing.busy || !self.editing.submitted.is_empty())
                            && !self.editing.saving
                            && !self.editing.pack_after_edits,
                        egui::Button::new("打包"),
                    )
                    .on_hover_text(format!("打包完整文件到 {}", self.editing.output.display()))
                    .clicked()
                {
                    self.editing.pack_after_edits = true;
                }
            });
        });
        let status = if self.editing.saving || self.editing.pack_after_edits {
            "正在打包…"
        } else if self.editing.busy {
            "已修改 · 正在更新预览…"
        } else if dirty {
            "已修改 · 尚未打包"
        } else {
            "当前文件 · 无待保存修改"
        };
        ui.add(egui::Label::new(egui::RichText::new(status).small()).truncate());
        if !self.editing.error.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&self.editing.error).color(egui::Color32::LIGHT_RED),
                )
                .wrap(),
            );
        }
        ui.separator();
    }

    pub(super) fn field_row(
        &mut self,
        ui: &mut egui::Ui,
        status: &mut egui::Ui,
        document: &Document,
        node: Option<&NodeKey>,
        index: usize,
        field: &Field,
    ) {
        if !field.writable || field.binding.range.is_empty() {
            ui.add(egui::Label::new(&field.value).truncate());
            return;
        }
        let Some(path) = &self.path else {
            ui.add(egui::Label::new(&field.value).truncate());
            return;
        };
        let Some(node) = node else {
            return;
        };
        let target = Target::Field {
            node: node.clone(),
            index,
            name: field.name.clone(),
        };
        let inputs = self.editing.inputs.entry(path.clone()).or_default();
        let input_index = if let Some(index) = inputs
            .iter()
            .position(|input| input.target == target || input.binding == field.binding)
        {
            index
        } else {
            match Input::new(target.clone(), field.binding.clone(), document) {
                Ok(input) => {
                    inputs.push(input);
                    inputs.len() - 1
                }
                Err(error) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                    return;
                }
            }
        };
        let input = &mut inputs[input_index];
        if !self.editing.busy && !input.pending {
            input.target = target;
        }
        ui.push_id(("field-input", node, index), |ui| {
            let editor_rect = ui.available_rect_before_wrap();
            let mut editor = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("editor")
                    .max_rect(editor_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            editor.set_clip_rect(editor_rect.intersect(ui.clip_rect()));
            let changed = editor
                .add_enabled_ui(!input.conflicted, |ui| {
                    super::fields::input(ui, &input.binding, &input.original, &mut input.text)
                })
                .inner;
            if changed {
                input.change();
                self.editing.last_input = Some(Instant::now());
                ui.ctx().request_repaint_after(Duration::from_millis(150));
            }
            if !input.error.is_empty()
                && status
                    .add(
                        egui::Button::new(egui::RichText::new("!").color(egui::Color32::LIGHT_RED))
                            .small(),
                    )
                    .on_hover_text(format!("{}\n单击撤回此输入", input.error))
                    .clicked()
            {
                input.reset(document);
            }
        });
    }

    pub(super) fn select_bytes(&mut self, document: &Document, buffer: usize, range: Range<usize>) {
        let Some(path) = &self.path else {
            return;
        };
        let owner_index = if document.nodes.get(self.node).is_some_and(|node| {
            node.buffer == buffer && node.range.start <= range.start && node.range.end >= range.end
        }) {
            Some(self.node)
        } else {
            document.nodes.iter().position(|node| {
                node.buffer == buffer && node.range == (0..document.buffers[buffer].len())
            })
        };
        let Some(owner_index) = owner_index else {
            return;
        };
        let Some(owner_key) = edit::node_key(document, owner_index) else {
            return;
        };
        let owner = &document.nodes[owner_index];
        let target = Target::Bytes {
            node: owner_key,
            range: range.start - owner.range.start..range.end - owner.range.start,
            owner_len: owner.range.len(),
            kind: owner.kind,
        };
        let binding = Binding {
            buffer,
            range,
            format: FieldType::Bytes,
            endian: mhf_resource::binary::Endian::Little,
        };
        match Input::new(target, binding, document) {
            Ok(input) => {
                self.editing.raw_offset = input.binding.range.start;
                self.editing.raw_length = input.binding.range.len();
                let inputs = self.editing.inputs.entry(path.clone()).or_default();
                let index = if let Some(index) = inputs
                    .iter()
                    .position(|old| old.target == input.target && old.binding == input.binding)
                {
                    index
                } else {
                    inputs.push(input);
                    inputs.len() - 1
                };
                self.editing.raw = Some(index);
            }
            Err(error) => self.editing.error = error,
        }
    }

    pub(super) fn byte_editor(&mut self, ui: &mut egui::Ui, document: &Document, node: &Node) {
        ui.separator();
        ui.weak("点击字节行直接编辑，输入后自动预览；按原长度覆盖。");
        ui.horizontal_wrapped(|ui| {
            ui.label("偏移");
            ui.add(egui::DragValue::new(&mut self.editing.raw_offset).hexadecimal(8, false, true));
            ui.label("长度");
            ui.add(
                egui::DragValue::new(&mut self.editing.raw_length)
                    .range(1..=document.buffers[node.buffer].len()),
            );
            if ui.small_button("定位").clicked() {
                let start = self
                    .editing
                    .raw_offset
                    .min(document.buffers[node.buffer].len());
                let end = start
                    .saturating_add(self.editing.raw_length)
                    .min(document.buffers[node.buffer].len());
                self.select_bytes(document, node.buffer, start..end);
            }
            if ui.small_button("所选字段").clicked()
                && let Some(range) = self.hex_selection.clone()
            {
                self.select_bytes(document, node.buffer, range);
            }
        });
        if let Some(input) = self
            .path
            .as_ref()
            .and_then(|path| self.editing.inputs.get_mut(path))
            .and_then(|inputs| self.editing.raw.and_then(|index| inputs.get_mut(index)))
            .filter(|input| input.binding.buffer == node.buffer)
        {
            ui.monospace(format!(
                "0x{:08X} · {} 字节",
                input.binding.range.start,
                input.binding.range.len()
            ));
            let changed = ui
                .push_id(("raw-editor", &self.path, &input.target), |ui| {
                    ui.add_enabled_ui(!input.conflicted, |ui| {
                        super::fields::input(ui, &input.binding, &input.original, &mut input.text)
                    })
                    .inner
                })
                .inner;
            if changed {
                input.change();
                self.editing.last_input = Some(Instant::now());
            }
            if !input.error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &input.error);
                if ui.small_button("撤回字节输入").clicked() {
                    input.reset(document);
                }
            }
        }
    }

    pub(super) fn replacement_editor(&mut self, ui: &mut egui::Ui, document: &Arc<Document>) {
        ui.collapsing("从文件替换完整资源", |ui| {
            ui.small("PNG、DDS、音频、模型和动画等可用编辑后的文件替换，所属容器会一起重建。");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.editing.replacement)
                    .hint_text("编辑后的资源文件路径")
                    .desired_width(ui.available_width()),
            );
            let ready = !self.editing.busy
                && !self.editing.replacement.is_empty()
                && self
                    .path
                    .as_ref()
                    .is_none_or(|path| !self.editing.pending(path));
            let enter =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if ui
                .add_enabled(ready, egui::Button::new("替换并预览"))
                .clicked()
                || ready && enter
            {
                self.request = self.request.wrapping_add(1);
                self.expanding = None;
                self.editing.busy = true;
                self.editing.error.clear();
                let node = if self.view.show_encoding_layers {
                    self.node
                } else {
                    document.payload(self.node).unwrap_or(self.node)
                };
                self.worker.replace(
                    self.request,
                    document.clone(),
                    node,
                    PathBuf::from(&self.editing.replacement),
                );
            }
        });
    }
}

#[cfg(test)]
#[path = "editing_tests.rs"]
mod tests;
