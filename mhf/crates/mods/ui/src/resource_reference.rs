//! Typed resource references shared by Rust egui consumers.
//!
//! References retain their native keys until the caller resolves them. This module
//! renders references and emits activation intent; it does not load resources,
//! resolve filesystem paths, or communicate with another application.

use std::{borrow::Cow, fmt::Write, ops::Range, path::Path};

use egui::{Id, Response, Ui};
use mhf_resource::{
    ResourcePath,
    action_definition::{AttackReference, NativeMotionRef},
    effect_archive::{CurveKind, CurveLookup, CurveReference},
};

/// A canonical identity or a target described by native keys, never a display label.
#[derive(Clone, Debug)]
pub enum ResourceTarget<'a> {
    Path(Cow<'a, ResourcePath>),
    /// A real source that the caller cannot express as a game-relative identity.
    /// The source is retained as an OS path and is never guessed into a canonical path.
    Source(&'a Path),
    Motion(NativeMotionRef),
    Attack(AttackReference),
    /// A DLL AI descriptor reference, not a root inside an EMD file.
    NativeScript {
        table: u32,
        index: u32,
    },
    Curve {
        reference: CurveReference,
        lookup: &'a CurveLookup,
    },
    /// The producer supplies a stable collection code and its original index.
    /// Without a resolved path the collection's resource context is unknown.
    Index {
        collection: &'static str,
        index: u32,
    },
}

impl<'a> From<&'a ResourcePath> for ResourceTarget<'a> {
    fn from(path: &'a ResourcePath) -> Self {
        Self::Path(Cow::Borrowed(path))
    }
}

impl From<ResourcePath> for ResourceTarget<'_> {
    fn from(path: ResourcePath) -> Self {
        Self::Path(Cow::Owned(path))
    }
}

impl From<NativeMotionRef> for ResourceTarget<'_> {
    fn from(reference: NativeMotionRef) -> Self {
        Self::Motion(reference)
    }
}

impl From<AttackReference> for ResourceTarget<'_> {
    fn from(reference: AttackReference) -> Self {
        Self::Attack(reference)
    }
}

/// The primary response belongs to the value or editor, not the copy button.
pub struct ResourceReferenceResponse {
    pub response: Response,
    /// Produced only when activation is enabled and a canonical path is known.
    pub activated: Option<ResourcePath>,
}

/// A bounded reference row with canonical copying and native target context.
#[derive(Default)]
pub struct ResourceReference<'a> {
    target: Option<ResourceTarget<'a>>,
    resolved_path: Option<&'a ResourcePath>,
    editor: Option<(Id, &'a mut String)>,
    id: Option<Id>,
    label: Option<&'a str>,
    help: &'a str,
    source_range: Option<Range<usize>>,
    compact: bool,
    activate: bool,
}

impl<'a> ResourceReference<'a> {
    pub fn new(target: impl Into<ResourceTarget<'a>>) -> Self {
        Self {
            target: Some(target.into()),
            ..Self::default()
        }
    }

    /// Editable presentation of the same reference row. Parsing is lexical;
    /// the caller owns Enter handling, errors, loading, and asynchronous state.
    /// Copy uses the current valid draft's canonical path without changing it.
    pub fn editor(id: Id, draft: &'a mut String) -> Self {
        Self {
            editor: Some((id, draft)),
            id: Some(id),
            ..Self::default()
        }
    }

    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    /// Borrow a caller-established identity for an otherwise native reference.
    pub fn resolved_path(mut self, path: &'a ResourcePath) -> Self {
        self.resolved_path = Some(path);
        self
    }

    /// Retain a source-provided name while the full identity remains in hover.
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Additional caller context in the same tooltip as the resource identity.
    pub fn help(mut self, help: &'a str) -> Self {
        self.help = help;
        self
    }

    /// Bytes in the owning data layer, separate from any native index range.
    /// An explicit range adds a weak detail line as well as hover information.
    /// Compact rows keep the range in hover so fixed-height lists can reuse them.
    pub fn source_range(mut self, range: Range<usize>) -> Self {
        self.source_range = Some(range);
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Allow this row to emit a canonical path on activation. Default is false.
    pub fn activate(mut self, activate: bool) -> Self {
        self.activate = activate;
        self
    }

    pub fn show(self, ui: &mut Ui) -> ResourceReferenceResponse {
        // Density scopes must not change the caller's default control identity.
        let id = self.id.unwrap_or_else(|| ui.next_auto_id());
        if self.compact {
            egui_hunter::Density::Compact
                .scope(ui, |ui| self.show_inner(ui, id))
                .inner
        } else {
            self.show_inner(ui, id)
        }
    }

    fn show_inner(mut self, ui: &mut Ui, id: Id) -> ResourceReferenceResponse {
        let path = if let Some((_, draft)) = &self.editor {
            draft.parse::<ResourcePath>().ok().map(Cow::Owned)
        } else {
            self.resolved_path
                .map(Cow::Borrowed)
                .or_else(|| match &self.target {
                    Some(ResourceTarget::Path(path)) => Some(Cow::Borrowed(path.as_ref())),
                    Some(ResourceTarget::Motion(reference)) => {
                        reference.resource_path().map(Cow::Owned)
                    }
                    _ => None,
                })
        };
        let canonical = path.as_ref().map(ToString::to_string);
        let value = match (self.label, canonical.as_deref(), &self.target) {
            (Some(label), _, _) | (_, Some(label), _) => Cow::Borrowed(label),
            (_, _, Some(ResourceTarget::Source(file))) => file.file_name().map_or_else(
                || Cow::Owned(file.display().to_string()),
                |name| name.to_string_lossy(),
            ),
            (_, _, Some(target)) => {
                let mut query = query_text(target).unwrap_or_default();
                query.push_str("（未解析）");
                Cow::Owned(query)
            }
            _ => Cow::Borrowed(""),
        };
        let span = self.source_range.as_ref().map(range_text);
        let interactive = self.activate && path.is_some() && self.editor.is_none();
        let mut copied = false;
        let row_height = ui.spacing().interact_size.y;
        let available = ui.available_rect_before_wrap();
        let width = available
            .width()
            .min((ui.clip_rect().right() - available.left()).max(0.0))
            .min((ui.max_rect().right() - available.left()).max(0.0));
        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(width, row_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    // Popup sizing passes must include the ordinary row height.
                    ui.set_min_height(row_height);
                    let response = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(id)
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |ui| {
                                if canonical.is_some() {
                                    copied = ui
                                        .add(
                                            egui_hunter::IconButton::new(
                                                egui_hunter::Icon::Copy,
                                                "复制资源路径",
                                            )
                                            .id(id.with("copy"))
                                            .kind(egui_hunter::ButtonKind::Quiet),
                                        )
                                        .clicked();
                                }
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width().max(0.0), row_height),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        if let Some((id, draft)) = self.editor.as_mut() {
                                            ui.add(
                                                egui_hunter::TextField::new(*id, draft)
                                                    .hint("资源路径"),
                                            )
                                        } else if interactive {
                                            ui.add(
                                                egui::Button::new(value.as_ref())
                                                    .frame(false)
                                                    .truncate(),
                                            )
                                        } else {
                                            ui.add(
                                                egui::Label::new(value.as_ref())
                                                    .truncate()
                                                    .selectable(true),
                                            )
                                        }
                                    },
                                )
                                .inner
                            },
                        )
                        .inner;
                    if let Some(span) = &span
                        && !self.compact
                    {
                        ui.weak(span);
                    }
                    response
                },
            )
            .inner
            .on_hover_ui(|ui| {
                if let Some(canonical) = &canonical {
                    ui.label(canonical);
                } else if self.editor.is_some() {
                    ui.label("游戏相对文件路径，可接 #原始索引/字段；无效输入没有规范资源身份。");
                }
                if let Some(query) = self.target.as_ref().and_then(query_text) {
                    ui.label(query);
                }
                if let Some(span) = &span {
                    ui.weak(span);
                }
                if !self.help.is_empty() {
                    ui.label(self.help);
                }
            });
        if copied {
            if let Some((_, draft)) = &self.editor {
                if let Ok(path) = draft.parse::<ResourcePath>() {
                    ui.ctx().copy_text(path.to_string());
                }
            } else if let Some(canonical) = canonical {
                ui.ctx().copy_text(canonical);
            }
        }
        let activated = if interactive && response.clicked() {
            path.map(Cow::into_owned)
        } else {
            None
        };
        ResourceReferenceResponse {
            response,
            activated,
        }
    }
}

fn range_text(range: &Range<usize>) -> String {
    format!(
        "数据层偏移 {:#x}..{:#x} · {} 字节",
        range.start,
        range.end,
        range.end.saturating_sub(range.start)
    )
}

fn query_text(target: &ResourceTarget<'_>) -> Option<String> {
    Some(match target {
        ResourceTarget::Path(_) => return None,
        ResourceTarget::Source(file) => format!("来源 {}", file.display()),
        ResourceTarget::Motion(reference) => format!(
            "动画 {} · 资源库 {} · 武器 {} · 风格 {} · 目录[{}] · 槽位[{}]",
            reference.id,
            reference.bank(),
            reference.weapon,
            reference
                .style
                .map_or_else(|| "未确定".into(), |style| style.to_string()),
            reference.record(),
            reference.slot(),
        ),
        ResourceTarget::Attack(reference) => {
            let subtype = reference
                .subtype
                .map_or_else(String::new, |subtype| format!(" · 子类别键[{subtype}]"));
            format!(
                "mhfsdt.bin · 类别键[{}]{subtype} · 攻击参数 · 记录[{}]",
                reference.category, reference.record,
            )
        }
        ResourceTarget::NativeScript { table, index } => {
            format!("DLL AI 原生根引用 · root[{table}][{index}]")
        }
        ResourceTarget::Curve { reference, lookup } => {
            let kind = match reference.kind {
                CurveKind::Vector => "向量",
                CurveKind::Color => "颜色",
                CurveKind::Integer => "整数",
            };
            let mut text = format!("{kind}曲线 ID {} · 匹配 [", reference.id);
            for (position, index) in lookup.matching_indices.iter().take(8).enumerate() {
                if position != 0 {
                    text.push_str(", ");
                }
                write!(text, "{index}").unwrap();
            }
            if lookup.matching_indices.len() > 8 {
                write!(text, ", …（共 {} 项）", lookup.matching_indices.len()).unwrap();
            }
            text.push_str("] · ");
            if let Some(range) = lookup.native_range() {
                write!(
                    text,
                    "原生键索引跨度 {}..{}（不含上界）",
                    range.start, range.end
                )
                .unwrap();
                if !lookup.is_contiguous() {
                    text.push_str("；匹配位置与原生跨度不同");
                }
            } else {
                text.push_str("无匹配记录");
            }
            text
        }
        ResourceTarget::Index { collection, index } => {
            format!("资源索引 · {collection}[{index}]（上下文未确定）")
        }
    })
}

#[cfg(test)]
mod tests;
