use crate::preview::{effects, lighting::LightingPreset};
use crate::{
    catalog::Catalog,
    inspect::{Document, Kind},
    preview::{
        Command, Control, DEFAULT_BACKGROUND_COLOR, LoadedModel, PlaybackTrack, ResourceRef,
        Snapshot, Viewport,
    },
    settings::ViewSettings,
    worker::Worker,
};
use egui::{Color32, RichText};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt::Write as _,
    path::PathBuf,
    sync::Arc,
};

mod editing;
#[cfg(test)]
mod field_layout_tests;
mod fields;
mod inspector;
#[cfg(test)]
mod popup_scroll_tests;
#[cfg(test)]
mod resource_scope_tests;
use editing::Editing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum InspectorTab {
    Loaded,
    Resource,
}

struct TimelineTrack {
    target: PlaybackTrack,
    range: [f32; 2],
    frame: Option<f32>,
}

pub(crate) struct Workbench {
    control: Arc<Control>,
    worker: Arc<Worker>,
    root: PathBuf,
    open: bool,
    view: ViewSettings,
    configuration: Option<mhf_config::Config<'static>>,
    view_dirty: bool,
    view_save_error: String,
    tab: InspectorTab,
    viewport_rect: egui::Rect,
    log: VecDeque<(bool, String)>,
    last_messages: [String; 3],
    catalog: Arc<Catalog>,
    filter: String,
    filtered: Vec<usize>,
    directory: Directory,
    scanning: bool,
    request: u64,
    loading: bool,
    expanding: Option<usize>,
    path: Option<PathBuf>,
    document: Option<Arc<Document>>,
    resource_counts: Vec<usize>,
    node: usize,
    selection: Option<ResourceRef>,
    hex_start: usize,
    hex_buffer: bool,
    hex_selection: Option<std::ops::Range<usize>>,
    bone: Option<(u64, usize)>,
    error: String,
    status: String,
    editing: Editing,
}

impl Workbench {
    pub fn new(
        control: Arc<Control>,
        worker: Arc<Worker>,
        root: PathBuf,
        view: ViewSettings,
        configuration: Option<mhf_config::Config<'static>>,
    ) -> Self {
        Self {
            control,
            worker,
            root: root.clone(),
            open: true,
            view,
            configuration,
            view_dirty: false,
            view_save_error: String::new(),
            tab: InspectorTab::Loaded,
            viewport_rect: egui::Rect::NOTHING,
            log: VecDeque::new(),
            last_messages: Default::default(),
            catalog: Arc::new(Catalog::default()),
            filter: String::new(),
            filtered: Vec::new(),
            directory: Directory::default(),
            scanning: true,
            request: 0,
            loading: false,
            expanding: None,
            path: None,
            document: None,
            node: 0,
            selection: None,
            hex_start: 0,
            resource_counts: Vec::new(),
            hex_buffer: false,
            hex_selection: None,
            bone: None,
            error: String::new(),
            status: String::new(),
            editing: Editing::new(&root),
        }
    }

    fn save_view(&mut self) {
        if !self.view_dirty {
            return;
        }
        self.view_dirty = false;
        if let Some(configuration) = self.configuration {
            if self.error == self.view_save_error {
                self.error.clear();
            }
            self.view_save_error = self.view.save(configuration).err().unwrap_or_default();
            if !self.view_save_error.is_empty() {
                self.error.clone_from(&self.view_save_error);
            }
        }
    }

    fn send(&mut self, command: Command) {
        self.error = self.control.send(command).err().unwrap_or_default();
    }

    fn poll(&mut self) {
        let updates = self.worker.updates();
        if let Some(catalog) = updates.catalog {
            self.scanning = false;
            match catalog {
                Ok(catalog) => {
                    self.catalog = catalog;
                    self.filter_files();
                }
                Err(error) => self.error = error,
            }
        }
        if let Some(loaded) = updates.loaded
            && loaded.request == self.request
        {
            self.loading = false;
            self.expanding = None;
            self.path = Some(loaded.path);
            self.hex_start = 0;
            self.hex_buffer = false;
            self.hex_selection = None;
            match loaded.document {
                Ok(document) => {
                    if let Some(path) = &self.path {
                        self.editing
                            .sessions
                            .insert(path.clone(), crate::session::Session::new(document.clone()));
                    }
                    self.loaded_document(document);
                }
                Err(error) => {
                    self.document = None;
                    self.selection = None;
                    self.resource_counts.clear();
                    self.error = error;
                }
            }
        }
        if let Some(expanded) = updates.expanded
            && expanded.request == self.request
        {
            self.expanding = None;
            match expanded.document {
                Ok(document) => self.refresh_document(document),
                Err(error) => self.error = error,
            }
        }
        if let Some(result) = updates.exported {
            match result {
                Ok(path) => self.status = format!("已导出 {}", path.display()),
                Err(error) => self.error = error,
            }
        }
        if let Some(edited) = updates.edited {
            self.finish_edit(edited);
        }
        if let Some(packed) = updates.packed {
            self.finish_pack(packed);
        }
    }

    fn loaded_document(&mut self, document: Arc<Document>) {
        self.node = visible_node(&document, document.root, self.view.show_encoding_layers);
        self.selection = Some(ResourceRef::new(document.clone(), self.node));
        self.refresh_document(document);
    }

    fn refresh_document(&mut self, document: Arc<Document>) {
        let selection = self
            .selection
            .as_ref()
            .filter(|source| source.node == self.node)
            .cloned()
            .or_else(|| self.selected_source())
            .and_then(|source| {
                if Arc::ptr_eq(&source.document, &document) {
                    Some(source)
                } else {
                    source.remap_path(document.clone()).ok()
                }
            })
            .unwrap_or_else(|| {
                let node = self
                    .document
                    .as_ref()
                    .and_then(|old| crate::edit::node_key(old, self.node))
                    .and_then(|key| crate::edit::locate(&document, &key))
                    .unwrap_or(document.root);
                ResourceRef::new(document.clone(), node)
            });
        self.select_source(selection);
        self.resource_counts = crate::preview::loadable_resource_counts(&document);
        self.status.clear();
        if !document.nodes.iter().any(|node| node.kind == Kind::Fmod)
            && document
                .nodes
                .iter()
                .any(|node| node.kind == Kind::StageLighting)
        {
            self.status =
                "此包包含高清场景光照与环境贴图；几何模型请打开 stage 中对应编号的主场景资源。"
                    .into();
        }
        if let Some(path) = &self.path
            && let Some(session) = self.editing.sessions.get_mut(path)
        {
            session.document = document.clone();
        }
        self.document = Some(document);
    }

    fn filter_files(&mut self) {
        self.filtered = self
            .catalog
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matches(&self.filter))
            .map(|(index, _)| index)
            .collect();
        self.directory = Directory::build(&self.catalog, &self.filtered);
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll();
        self.flush_previews();
        let context = ui.ctx().clone();
        self.flush_edits(&context);
        let previous_view = self.view;
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F8)) {
            self.open = !self.open;
        }
        if context.input_mut(|input| {
            let pressed = input.events.iter().any(|event| {
                matches!(event, egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if *modifiers == egui::Modifiers::ALT)
            });
            input.consume_key(egui::Modifiers::ALT, egui::Key::Enter);
            pressed
        }) {
            self.send(Command::ToggleFullscreen);
        }
        if !self.open {
            self.save_view();
            self.control.set_viewport(Viewport::default());
            return;
        }
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            self.view.preview_only = !self.view.preview_only;
        }
        let snapshot = self.control.snapshot();
        if self
            .bone
            .is_some_and(|(id, _)| !snapshot.resources.iter().any(|resource| resource.id == id))
        {
            self.bone = None;
        }
        self.record_messages(&snapshot);
        ui.scope(|ui| {
            if self.view.compact {
                egui_hunter::Density::Compact.scope(ui, |ui| self.layout(ui, &snapshot));
            } else {
                self.layout(ui, &snapshot);
            }
        });
        if self.view != previous_view {
            self.view_dirty = true;
            self.control
                .set_preview_options(self.view.preview_options());
        }
        // Apply color while dragging, and persist once the pointer is released.
        if !context.input(|input| input.pointer.any_down()) {
            self.save_view();
        }
        self.flush_edits(&context);
    }

    fn layout(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let screen = ui.ctx().content_rect();
        let frame = egui::Frame::NONE
            .fill(ui.visuals().window_fill)
            .inner_margin(ui.spacing().window_margin);
        egui::Panel::top("workbench-toolbar")
            .frame(frame)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("MHF 资源工作台");
                    ui.separator();
                    // Native menu Areas need an explicit copy of this local style.
                    let menu_style = ui.style().clone();
                    ui.menu_button("视图", |ui| {
                        ui.set_style(menu_style.clone());
                        egui::containers::menu::menu_style(ui.style_mut());
                        ui.checkbox(&mut self.view.compact, "紧凑模式");
                        egui::containers::menu::SubMenuButton::new("模型预览背景")
                            .config(
                                egui::containers::menu::MenuConfig::new()
                                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                            )
                            .ui(ui, |ui| {
                                ui.spacing_mut().slider_width = 240.0;
                                let [r, g, b] = self.view.background_color;
                                let mut color = Color32::from_rgb(r, g, b);
                                if egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut color,
                                    egui::color_picker::Alpha::Opaque,
                                ) {
                                    self.view.background_color = [color.r(), color.g(), color.b()];
                                }
                                if ui.button("恢复默认").clicked() {
                                    self.view.background_color = DEFAULT_BACKGROUND_COLOR;
                                }
                            });
                        ui.menu_button("预览光照", |ui| {
                            for preset in LightingPreset::ALL {
                                ui.selectable_value(
                                    &mut self.view.lighting_preset,
                                    preset,
                                    preset.label(),
                                )
                                .on_hover_text(preset.description());
                            }
                        });
                        ui.separator();
                        ui.checkbox(&mut self.view.show_resources, "资源目录");
                        if ui
                            .checkbox(&mut self.view.show_encoding_layers, "显示编码层")
                            .changed()
                            && !self.view.show_encoding_layers
                            && let Some(document) = &self.document
                        {
                            self.node = visible_node(document, self.node, false);
                            self.selection = self
                                .selection
                                .as_ref()
                                .and_then(|source| source.related(self.node));
                            self.hex_start = 0;
                            self.hex_buffer = false;
                            self.hex_selection = None;
                        }
                        ui.checkbox(&mut self.view.show_inspector, "检查器");
                        ui.checkbox(&mut self.view.show_log, "输出日志");
                        ui.checkbox(&mut self.view.preview_only, "专注预览 · F11");
                        ui.checkbox(&mut self.view.show_grid, "地面网格");
                        ui.checkbox(&mut self.view.show_axes, "坐标显示");
                        if ui.button("切换全屏 · Alt+Enter").clicked() {
                            self.send(Command::ToggleFullscreen);
                            ui.close();
                        }
                        if !self.view_save_error.is_empty() {
                            ui.separator();
                            ui.colored_label(ui.visuals().error_fg_color, &self.view_save_error);
                            if ui.button("重试保存视图设置").clicked() {
                                self.view_dirty = true;
                            }
                        }
                    });
                    if ui
                        .button(if self.view.preview_only {
                            "恢复布局 · F11"
                        } else {
                            "专注预览 · F11"
                        })
                        .clicked()
                    {
                        self.view.preview_only = !self.view.preview_only;
                    }
                    ui.menu_button("工作台", |ui| {
                        ui.set_style(menu_style);
                        egui::containers::menu::menu_style(ui.style_mut());
                        if ui.button("隐藏界面 · F8").clicked() {
                            self.open = false;
                            ui.close();
                        }
                        if ui.button("结束工作台").clicked() {
                            self.send(Command::Exit);
                            ui.close();
                        }
                    });
                    let message = if self.error.is_empty() {
                        snapshot.message.as_ref()
                    } else {
                        &self.error
                    };
                    ui.add(egui::Label::new(message).truncate())
                        .on_hover_text(message);
                });
            });
        // Both side panels leave room for a usable center even in a smaller window.
        let side_limit = ((screen.width() - 300.0) / 2.0).clamp(160.0, 520.0);
        if self.view.show_resources && !self.view.preview_only {
            egui::Panel::left("workbench-resources")
                .default_size(300.0)
                .size_range(200.0_f32.min(side_limit)..=side_limit)
                .frame(frame)
                .show(ui, |ui| {
                    self.resources(ui);
                });
        }
        if self.view.show_inspector && !self.view.preview_only {
            egui::Panel::right("workbench-inspector")
                .default_size(320.0)
                .size_range(220.0_f32.min(side_limit)..=side_limit)
                .frame(frame)
                .show(ui, |ui| self.inspector_panel(ui, snapshot));
        }
        if self.view.show_log && !self.view.preview_only {
            egui::Panel::bottom("workbench-log")
                .resizable(true)
                .default_size(120.0)
                .size_range(65.0..=(screen.height() * 0.3).max(65.0))
                .frame(frame)
                .show(ui, |ui| self.output_log(ui));
        }
        egui::Panel::bottom("workbench-timeline")
            .frame(frame)
            .show(ui, |ui| self.timeline(ui, snapshot));
        egui::Panel::top("workbench-viewport-toolbar")
            .frame(frame)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("资源预览");
                    if ui
                        .add_enabled(
                            has_visible_resources(snapshot),
                            egui_hunter::Button::new("聚焦全部 · F"),
                        )
                        .clicked()
                    {
                        self.send(Command::FocusAll);
                    }
                    ui.checkbox(&mut self.view.show_bones, "骨架");
                    ui.checkbox(&mut self.view.show_grid, "网格");
                    ui.checkbox(&mut self.view.show_axes, "坐标");
                    ui.weak("左键环绕 · 中键 / Shift+左键平移 · 滚轮缩放");
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport(ui, snapshot, screen));
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (InspectorTab::Loaded, "已加载"),
                (InspectorTab::Resource, "详情"),
            ] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        let scroll_style = ui.spacing().scroll;
        let mut margin = scroll_style.content_margin;
        if scroll_style.floating {
            // Floating bars use no layout space, including while appearing or
            // expanding on hover. Reserve their full width before laying out rows.
            let gutter = (scroll_style.bar_width + scroll_style.bar_inner_margin).ceil() as i8;
            margin.right = margin.right.max(gutter);
            margin.bottom = margin.bottom.max(gutter);
        }
        egui::ScrollArea::both()
            .id_salt(("workbench-inspector-content", self.tab))
            .max_width(ui.available_width())
            .content_margin(margin)
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                InspectorTab::Loaded => self.loaded_resources(ui, snapshot),
                InspectorTab::Resource => {
                    if let Some(document) = self.document.clone() {
                        self.resource_actions(ui, &document);
                        if let Some(node) = document.nodes.get(self.node) {
                            self.inspector(ui, &document, node);
                            // A placeholder item replaces no existing bytes, so
                            // a whole-resource file has nothing to overwrite.
                            if node.kind != Kind::MissingBlock {
                                self.replacement_editor(ui);
                            }
                        }
                    } else {
                        ui.weak("单击资源查看字段与原始字节，点击右侧按钮加载。");
                    }
                }
            });
    }

    fn viewport(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, screen: egui::Rect) {
        let rect = ui.available_rect_before_wrap();
        self.viewport_rect = rect;
        self.control.set_viewport(Viewport {
            x: (rect.left() - screen.left()) / screen.width(),
            y: (rect.top() - screen.top()) / screen.height(),
            width: rect.width() / screen.width(),
            height: rect.height() / screen.height(),
        });
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        if response.clicked() {
            response.request_focus();
        }
        if !has_visible_resources(snapshot) {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "选择左侧资源，点击右侧的加载按钮",
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
        self.draw_bones(ui, snapshot);
        self.draw_effects(ui, snapshot);
        self.draw_coordinates(ui, snapshot);
        let mut distance = snapshot.distance;
        let mut pitch = snapshot.pitch;
        let mut yaw = snapshot.yaw;
        let primary_drag = response.dragged_by(egui::PointerButton::Primary);
        let pan = response.dragged_by(egui::PointerButton::Middle)
            || (primary_drag && ui.input(|input| input.modifiers.shift));
        if primary_drag && !pan {
            let delta = response.drag_delta();
            yaw = (yaw - delta.x * 0.4 + 180.0).rem_euclid(360.0) - 180.0;
            pitch = (pitch + delta.y * 0.4).clamp(-80.0, 80.0);
        }
        if pan && let Some(camera) = snapshot.camera {
            response.request_focus();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let delta = response.drag_delta();
            if delta != egui::Vec2::ZERO
                && let Some(offset) =
                    camera.pan_offset([delta.x / rect.width(), delta.y / rect.height()])
            {
                self.send(Command::Pan(offset));
            }
        }
        if response.hovered() {
            let scroll = ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y));
            distance = (distance * (-scroll * 0.002).exp()).clamp(1.0, 100_000.0);
        }
        if (distance, pitch, yaw) != (snapshot.distance, snapshot.pitch, snapshot.yaw) {
            self.send(Command::Camera {
                distance,
                pitch,
                yaw,
            });
        }
        if (response.hovered() || response.has_focus()) && !ui.ctx().text_edit_focused() {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F))
                && has_visible_resources(snapshot)
            {
                self.send(Command::FocusAll);
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Space))
                && (!snapshot.motions.is_empty() || !snapshot.loaded_effects.is_empty())
            {
                self.send(Command::Playing(!snapshot.playing));
            }
        }
    }

    fn timeline(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.add_enabled_ui(
            !snapshot.motions.is_empty() || !snapshot.loaded_effects.is_empty(),
            |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(if snapshot.playing { "暂停" } else { "播放" })
                        .clicked()
                    {
                        self.send(Command::Playing(!snapshot.playing));
                    }
                    let mut speed = snapshot.playback_speed;
                    egui::ComboBox::from_id_salt("preview-speed")
                        .width(58.0)
                        .selected_text(format!("{speed}×"))
                        .show_ui(ui, |ui| {
                            for value in [0.25, 0.5, 1.0, 2.0, 4.0] {
                                ui.selectable_value(&mut speed, value, format!("{value}×"));
                            }
                        })
                        .response
                        .on_hover_text("同时调整动画与特效的预览速度");
                    if speed != snapshot.playback_speed {
                        self.send(Command::PlaybackSpeed(speed));
                    }
                });
            },
        );
    }

    fn record_messages(&mut self, snapshot: &Snapshot) {
        for (index, text) in [
            snapshot.message.as_ref(),
            self.error.as_str(),
            self.status.as_str(),
        ]
        .into_iter()
        .enumerate()
        {
            if text != self.last_messages[index] {
                text.clone_into(&mut self.last_messages[index]);
                if !text.is_empty() {
                    if self.log.len() == 200 {
                        self.log.pop_front();
                    }
                    self.log.push_back((index == 1, text.to_owned()));
                }
            }
        }
    }

    fn output_log(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("输出日志");
            if ui.small_button("清空").clicked() {
                self.log.clear();
            }
            if ui.small_button("复制").clicked() {
                ui.ctx().copy_text(
                    self.log
                        .iter()
                        .map(|(_, text)| text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
        });
        egui::ScrollArea::both()
            .id_salt("workbench-log-entries")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (error, text) in &self.log {
                    let color = if *error {
                        ui.visuals().error_fg_color
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.label(RichText::new(text).monospace().color(color));
                }
            });
    }

    fn resources(&mut self, ui: &mut egui::Ui) -> egui::Rect {
        let root = self.root.display().to_string();
        ui.add(egui::Label::new(&root).truncate())
            .on_hover_text(root);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.scanning, egui::Button::new("刷新"))
                    .clicked()
                {
                    self.worker.scan();
                    self.scanning = true;
                }
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .id(egui::Id::new("workbench-filter"))
                            .hint_text("文件名 / 路径，多词筛选")
                            .desired_width(ui.available_width()),
                    )
                    .changed()
                {
                    self.filter_files();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.strong("资源目录");
            ui.small(format!(
                "{} / {} 个文件",
                self.filtered.len(),
                self.catalog.entries.len()
            ));
            if self.scanning || self.loading || self.expanding.is_some() {
                ui.spinner();
            }
        });
        let before = self.node;
        let mut load = None;
        let mut load_node = None;
        let mut details = None;
        let browser = egui::ScrollArea::both()
            .id_salt("workbench-files")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                directory(
                    ui,
                    &self.directory,
                    &self.catalog,
                    self.path.as_ref(),
                    self.document.as_ref(),
                    &self.resource_counts,
                    self.view.show_encoding_layers,
                    &mut self.node,
                    &mut self.selection,
                    &mut load,
                    &mut load_node,
                    &mut details,
                    !self.filter.is_empty(),
                );
                if self.catalog.entries.is_empty() && !self.scanning {
                    ui.weak("此目录没有可读取的文件。");
                }
            });
        if before != self.node {
            self.hex_start = 0;
            self.hex_buffer = false;
            self.hex_selection = None;
        }
        if let Some(index) = details {
            self.expand_node(index);
        }
        if let Some(source) = load_node {
            self.load_source(source);
        }
        if let Some(path) = load {
            self.open_document(path);
        }
        browser.inner_rect
    }

    fn expand_node(&mut self, index: usize) {
        if self.expanding.is_none()
            && !self.editing.busy
            && let Some(document) = &self.document
            && document.nodes[index].deferred
        {
            self.expanding = Some(index);
            self.worker.expand(self.request, document.clone(), index);
        }
    }

    fn resource_actions(&mut self, ui: &mut egui::Ui, document: &Arc<Document>) {
        if document.nodes[self.node].deferred && ui.button("展开明细").clicked() {
            self.expand_node(self.node);
        }
        let resource_count = self.resource_counts.get(self.node).copied().unwrap_or(0);
        if resource_count != 0 {
            ui.horizontal(|ui| {
                ui.strong(format!("资源 {resource_count}"));
                if ui
                    .small_button("加载")
                    .on_hover_text("加载所选范围内的资源")
                    .clicked()
                {
                    self.load_node(self.node);
                }
            });
        }
    }

    fn load_node(&mut self, node: usize) {
        if let Some(document) = &self.document {
            let source = self
                .selected_source()
                .filter(|source| source.node == node)
                .unwrap_or_else(|| ResourceRef::new(document.clone(), node));
            self.load_source(source);
        }
    }

    fn selected_source(&self) -> Option<ResourceRef> {
        let document = self.document.as_ref()?;
        document.nodes.get(self.node)?;
        Some(
            self.selection
                .as_ref()
                .filter(|source| {
                    source.node == self.node && Arc::ptr_eq(&source.document, document)
                })
                .cloned()
                .unwrap_or_else(|| ResourceRef::new(document.clone(), self.node)),
        )
    }

    fn select_source(&mut self, source: ResourceRef) {
        self.node = source.node;
        self.selection = Some(source);
    }

    fn load_source(&mut self, source: ResourceRef) {
        self.select_source(source.clone());
        self.tab = InspectorTab::Loaded;
        self.send(Command::LoadResource(source));
    }

    fn effect_controls(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        for effect in snapshot.loaded_effects.iter() {
            ui.push_id(("loaded-effect", effect.id), |ui| {
                ui.horizontal(|ui| {
                    let mut enabled = effect.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        self.send(Command::EffectEnabled {
                            binding: effect.id,
                            enabled,
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("卸载").clicked() {
                            self.send(Command::RemoveEffect(effect.id));
                        }
                        ui.add_sized(
                            [ui.available_width(), ui.spacing().interact_size.y],
                            egui::Label::new(effect.source.short_name()).truncate(),
                        )
                        .on_hover_text(effect.source.name());
                    });
                });
                let previous = if effect.automatic {
                    None
                } else {
                    Some(effect.manual_target)
                };
                let mut target = previous;
                let name = (if effect.automatic {
                    effect.model
                } else {
                    effect.manual_target
                })
                .and_then(|id| snapshot.models.iter().find(|model| model.id == id))
                .map_or("未绑定", |model| model.name.as_ref());
                let short_name = name.rsplit(['/', '\\']).next().unwrap_or(name);
                let target_width = ui.available_width();
                egui::ComboBox::from_id_salt(("effect-target", effect.id))
                    .width(target_width)
                    .truncate()
                    .selected_text(if effect.automatic {
                        format!("自动 · {short_name}")
                    } else {
                        short_name.to_owned()
                    })
                    .show_ui(ui, |ui| {
                        ui.set_max_width(target_width);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        ui.selectable_value(
                            &mut target,
                            None,
                            effect.model_id.map_or_else(
                                || "自动匹配".into(),
                                |id| format!("自动匹配 · 模型 ID {id}"),
                            ),
                        );
                        ui.selectable_value(&mut target, Some(None), "未绑定");
                        for model in snapshot.models.iter().filter(|model| model.error.is_none()) {
                            ui.selectable_value(
                                &mut target,
                                Some(Some(model.id)),
                                model
                                    .name
                                    .rsplit(['/', '\\'])
                                    .next()
                                    .unwrap_or(model.name.as_ref()),
                            )
                            .on_hover_text(model.name.as_ref());
                        }
                    })
                    .response
                    .on_hover_text(format!("{name}\n{}", effect.message));
                if target != previous {
                    self.send(match target {
                        None => Command::AutoBindEffect(effect.id),
                        Some(model) => Command::BindEffect {
                            binding: effect.id,
                            model,
                        },
                    });
                }
                for entry in &effect.binding.definitions {
                    self.effect_definition_controls(
                        ui,
                        &effect.binding,
                        entry,
                        effect.enabled && effect.model.is_some(),
                    );
                }
                ui.add_space(4.0);
            });
        }
    }

    fn effect_definition_controls(
        &mut self,
        ui: &mut egui::Ui,
        binding: &effects::BindingSnapshot,
        entry: &effects::DefinitionSnapshot,
        bound: bool,
    ) {
        ui.push_id(("effect-definition", binding.id, entry.slot), |ui| {
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.id().with("details"),
                false,
            )
            .show_header(ui, |ui| {
                ui.strong(format!("定义 {}", entry.id));
                if ui
                    .add_enabled(
                        bound,
                        egui::Button::new(if entry.frame.is_some() {
                            "重播"
                        } else {
                            "触发"
                        })
                        .small(),
                    )
                    .clicked()
                {
                    self.send(Command::TriggerEffectDefinition {
                        binding: binding.id,
                        slot: entry.slot,
                    });
                }
                if ui
                    .add_enabled(
                        bound && entry.frame.is_some(),
                        egui::Button::new("停止").small(),
                    )
                    .clicked()
                {
                    self.send(Command::StopEffectDefinition {
                        binding: binding.id,
                        slot: entry.slot,
                    });
                }
                if ui.small_button("移除").clicked() {
                    self.send(Command::RemoveEffectDefinition {
                        binding: binding.id,
                        slot: entry.slot,
                    });
                }
            })
            .body(|ui| {
                let target = entry.draw.map_or_else(
                    || "附着点".into(),
                    |(group, item)| format!("绘制组 {group} / 材质槽 {item}"),
                );
                ui.label(format!(
                    "节点 {} · {target} · 延迟 {} 步 · 原条件 {}",
                    entry.node, entry.delay, entry.condition
                ));
                ui.label(&entry.message);
            });
            self.effect_entry_track(ui, binding, entry);
        });
    }

    fn motion_list(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        for motion in snapshot.motions.iter() {
            ui.push_id(("loaded-motion", motion.id), |ui| {
                ui.horizontal(|ui| {
                    let mut enabled = motion.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        self.send(Command::MotionEnabled {
                            id: motion.id,
                            enabled,
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("卸载").clicked() {
                            self.send(Command::RemoveMotion(motion.id));
                        }
                        let target = motion
                            .skeleton
                            .and_then(|id| snapshot.resources.iter().find(|entry| entry.id == id))
                            .map(|entry| entry.source.short_name())
                            .unwrap_or_else(|| "未绑定".into());
                        ui.add_sized(
                            [ui.available_width(), ui.spacing().interact_size.y],
                            egui::Label::new(motion.source.short_name()).truncate(),
                        )
                        .on_hover_text(format!(
                            "{}\n{}",
                            motion.source.name(),
                            target
                        ));
                    });
                });
                self.track_row(
                    ui,
                    &motion.source.name(),
                    TimelineTrack {
                        target: PlaybackTrack::Motion(motion.id),
                        range: [0.0, motion.frames],
                        frame: motion.frame,
                    },
                    Color32::from_rgb(75, 115, 205),
                );
            });
        }
    }

    fn effect_entry_track(
        &mut self,
        ui: &mut egui::Ui,
        binding: &effects::BindingSnapshot,
        entry: &effects::DefinitionSnapshot,
    ) {
        self.track_row(
            ui,
            &binding.name,
            TimelineTrack {
                target: PlaybackTrack::Effect {
                    binding: binding.id,
                    slot: entry.slot,
                },
                range: [f32::from(entry.delay), entry.frames],
                frame: entry.frame,
            },
            if entry.active {
                Color32::from_rgb(40, 150, 165)
            } else {
                Color32::from_gray(70)
            },
        )
        .on_hover_text(format!(
            "延迟 {} 步{} · {}",
            entry.delay,
            if entry.looping {
                " · 最长通道一轮"
            } else {
                ""
            },
            entry.message
        ));
    }

    fn track_row(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        track: TimelineTrack,
        color: Color32,
    ) -> egui::Response {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (caption, delta, help) in
                    [("+1", 1, "本轨道下一步"), ("-1", -1, "本轨道上一步")]
                {
                    if ui
                        .add_enabled(track.frame.is_some(), egui::Button::new(caption).small())
                        .on_hover_text(help)
                        .clicked()
                    {
                        self.send(Command::Step {
                            track: track.target,
                            delta,
                        });
                    }
                }
                let progress = track.frame.map_or_else(
                    || format!("— / {:.0} 步", track.range[1]),
                    |frame| format!("{frame:.0} / {:.0} 步", track.range[1]),
                );
                ui.add_sized([86.0, 18.0], egui::Label::new(&progress).truncate())
                    .on_hover_text(progress);
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().max(1.0), 16.0),
                    if track.frame.is_some() {
                        egui::Sense::click_and_drag()
                    } else {
                        egui::Sense::hover()
                    },
                );
                let painter = ui.painter();
                painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                let x = |step: f32| {
                    rect.left() + rect.width() * (step / track.range[1].max(1.0)).clamp(0.0, 1.0)
                };
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x(track.range[0]), rect.top()),
                        egui::pos2(x(track.range[1]), rect.bottom()),
                    ),
                    2.0,
                    color,
                );
                if let Some(frame) = track.frame {
                    let playhead = x(frame);
                    painter.line_segment(
                        [
                            egui::pos2(playhead, rect.top()),
                            egui::pos2(playhead, rect.bottom()),
                        ],
                        egui::Stroke::new(2.0, Color32::WHITE),
                    );
                }
                if (response.clicked() || response.dragged())
                    && let Some(pointer) = response.interact_pointer_pos()
                {
                    self.send(Command::Seek {
                        track: track.target,
                        frame: ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0)
                            * track.range[1],
                    });
                }
                response
            })
            .inner
        })
        .inner
        .on_hover_text(label)
    }

    fn draw_effects(&self, ui: &egui::Ui, snapshot: &Snapshot) {
        let Some(camera) = snapshot.camera.filter(|_| snapshot.ready) else {
            return;
        };
        let viewport = rendered_viewport(ui, snapshot);
        let painter = ui
            .painter()
            .with_clip_rect(viewport.intersect(ui.clip_rect()));
        for (binding_index, effect) in snapshot.loaded_effects.iter().enumerate() {
            if !effect.enabled
                || !snapshot.models.iter().any(|model| {
                    Some(model.id) == effect.model && model.visible && model.error.is_none()
                })
            {
                continue;
            }
            for entry in &effect.binding.definitions {
                let Some(position) = entry.position.filter(|_| entry.active) else {
                    continue;
                };
                let Some(point) = camera.project(position) else {
                    continue;
                };
                let point = viewport.min
                    + egui::vec2(point[0] * viewport.width(), point[1] * viewport.height());
                let color = Color32::from_rgb(entry.color[0], entry.color[1], entry.color[2]);
                painter.circle_stroke(point, 6.0, egui::Stroke::new(2.0, color));
                painter.text(
                    point + egui::vec2(9.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("{}:{} 定义 {}", binding_index + 1, entry.slot, entry.id),
                    egui::FontId::monospace(12.0),
                    color,
                );
            }
        }
    }

    fn loaded_resources(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.id().with("模型"),
            false,
        )
        .show_header(ui, |ui| {
            ui.strong("模型");
            let visible = snapshot
                .models
                .iter()
                .any(|model| model.visible && model.error.is_none());
            if ui
                .add_enabled(visible, egui::Button::new("聚焦全部").small())
                .clicked()
            {
                self.send(Command::FocusAll);
            }
            if ui
                .add_enabled(
                    !snapshot.models.is_empty(),
                    egui::Button::new("清空").small(),
                )
                .clicked()
            {
                self.send(Command::ClearAssets);
            }
        })
        .body(|ui| {
            if snapshot.models.is_empty() {
                ui.weak("未加载模型");
            } else {
                for model in snapshot.models.iter() {
                    ui.push_id(("preview-model", model.id), |ui| {
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            ui.id().with("meshes"),
                            false,
                        )
                        .show_header(ui, |ui| {
                            let mut visible = model.visible;
                            if ui
                                .add_enabled(
                                    model.error.is_none(),
                                    egui::Checkbox::without_text(&mut visible),
                                )
                                .on_hover_text("显示模型")
                                .changed()
                            {
                                self.send(Command::ModelVisible {
                                    id: model.id,
                                    visible,
                                });
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("卸载").clicked() {
                                        self.send(Command::RemoveAsset(model.id));
                                    }
                                    let name = model
                                        .name
                                        .rsplit(['/', '\\'])
                                        .next()
                                        .unwrap_or(model.name.as_ref());
                                    ui.add_sized(
                                        [ui.available_width(), ui.spacing().interact_size.y],
                                        egui::Label::new(name).truncate(),
                                    )
                                    .on_hover_text(model.name.as_ref());
                                },
                            );
                        })
                        .body(|ui| {
                            if model.error.is_none() {
                                self.mesh_controls(ui, model);
                            }
                        });
                        if let Some(error) = &model.error {
                            ui.colored_label(Color32::LIGHT_RED, format!("无法预览：{error}"));
                        }
                    });
                }
            }
        });
        for (title, kind) in [("骨架", Kind::Fskl), ("贴图", Kind::Txb)] {
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.id().with(title),
                false,
            )
            .show_header(ui, |ui| {
                ui.strong(title);
                if ui
                    .add_enabled(
                        snapshot
                            .resources
                            .iter()
                            .any(|entry| entry.in_category(kind)),
                        egui::Button::new("清空").small(),
                    )
                    .clicked()
                {
                    self.send(Command::ClearResources(kind));
                }
            })
            .body(|ui| self.resource_list(ui, snapshot, kind));
        }
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.id().with("动画"),
            false,
        )
        .show_header(ui, |ui| {
            ui.strong("动画");
            if ui
                .add_enabled(
                    !snapshot.motions.is_empty(),
                    egui::Button::new("清空").small(),
                )
                .clicked()
            {
                self.send(Command::ClearMotions);
            }
        })
        .body(|ui| self.motion_list(ui, snapshot));
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.id().with("特效"),
            false,
        )
        .show_header(ui, |ui| {
            ui.strong("特效");
            if ui
                .add_enabled(
                    !snapshot.loaded_effects.is_empty(),
                    egui::Button::new("清空").small(),
                )
                .clicked()
            {
                self.send(Command::ClearLoadedEffects);
            }
        })
        .body(|ui| self.effect_controls(ui, snapshot));
        ui.collapsing("相机参数", |ui| self.camera_controls(ui, snapshot));
    }

    fn resource_list(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, kind: Kind) {
        for entry in snapshot
            .resources
            .iter()
            .filter(|entry| entry.in_category(kind))
        {
            ui.push_id(("loaded-resource", entry.id), |ui| {
                if kind == Kind::Fskl {
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        ui.id().with("nodes"),
                        false,
                    )
                    .show_header(ui, |ui| self.resource_row(ui, snapshot, entry))
                    .body(|ui| {
                        if let Some(skeleton) = snapshot
                            .skeletons
                            .iter()
                            .find(|skeleton| skeleton.id == entry.id)
                        {
                            self.bones(ui, skeleton, entry.enabled);
                        }
                    });
                } else {
                    ui.horizontal(|ui| self.resource_row(ui, snapshot, entry));
                }
            });
        }
    }

    fn resource_row(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        entry: &crate::preview::LoadedResource,
    ) {
        let mut enabled = entry.enabled;
        if ui.checkbox(&mut enabled, "").changed() {
            self.send(Command::ResourceEnabled {
                id: entry.id,
                enabled,
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("卸载").clicked() {
                self.send(Command::RemoveResource(entry.id));
            }
            let bound = snapshot
                .models
                .iter()
                .filter(|model| model.resources.contains(&entry.source))
                .count();
            ui.add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                egui::Label::new(entry.source.short_name()).truncate(),
            )
            .on_hover_text(format!("{}\n绑定 {bound} 个模型", entry.source.name()));
        });
    }

    fn mesh_controls(&mut self, ui: &mut egui::Ui, model: &LoadedModel) {
        let visible = model.meshes.iter().filter(|mesh| mesh.visible).count();
        ui.horizontal(|ui| {
            ui.strong("子网格");
            ui.weak(format!("{visible} / {} 可见", model.meshes.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !model.meshes.is_empty(),
                        egui::Button::new("恢复全部").small(),
                    )
                    .clicked()
                {
                    self.send(Command::ShowAllMeshes(model.id));
                }
            });
        });
        let row_height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt(("workbench-meshes", model.id))
            .max_height(8.0 * (row_height + ui.spacing().item_spacing.y))
            .auto_shrink([false, true])
            .show_rows(ui, row_height, model.meshes.len(), |ui, rows| {
                for row in rows {
                    let mesh = &model.meshes[row];
                    ui.push_id((model.id, mesh.index), |ui| {
                        ui.horizontal(|ui| {
                            let mut visible = mesh.visible;
                            if ui
                                .checkbox(&mut visible, format!("网格 {}", mesh.index))
                                .changed()
                            {
                                self.send(Command::MeshVisible {
                                    model: model.id,
                                    mesh: mesh.index,
                                    visible,
                                });
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("单独查看").clicked() {
                                        self.send(Command::IsolateMesh {
                                            model: model.id,
                                            mesh: mesh.index,
                                        });
                                    }
                                    ui.weak(format!("{} 顶点", mesh.vertices));
                                },
                            );
                        });
                    });
                }
            });
    }

    fn camera_controls(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let (mut distance, mut pitch, mut yaw) = (snapshot.distance, snapshot.pitch, snapshot.yaw);
        let mut changed = ui
            .add(
                egui::Slider::new(&mut distance, 1.0..=100_000.0)
                    .logarithmic(true)
                    .text("距离"),
            )
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut pitch, -80.0..=80.0)
                    .text("俯仰")
                    .suffix("°"),
            )
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut yaw, -180.0..=180.0)
                    .text("环绕")
                    .suffix("°"),
            )
            .changed();
        if changed {
            self.send(Command::Camera {
                distance,
                pitch,
                yaw,
            });
        }
    }

    fn bones(
        &mut self,
        ui: &mut egui::Ui,
        skeleton: &crate::preview::LoadedSkeleton,
        enabled: bool,
    ) {
        if let Some(error) = &skeleton.error {
            ui.colored_label(Color32::LIGHT_RED, error.as_ref());
            return;
        }
        ui.horizontal(|ui| {
            ui.small(format!("{} 个节点", skeleton.bones.len()));
            if ui
                .add_enabled(
                    enabled && !skeleton.bones.is_empty(),
                    egui::Button::new("聚焦全部").small(),
                )
                .clicked()
            {
                self.send(Command::FocusBone {
                    skeleton: skeleton.id,
                    node: None,
                });
            }
            if skeleton.bone_bindings.iter().any(Option::is_some)
                && ui
                    .add_enabled(enabled, egui::Button::new("清除姿态跟随").small())
                    .clicked()
            {
                self.send(Command::ClearBoneBindings(skeleton.id));
            }
        });
        egui::ScrollArea::vertical()
            .id_salt(("workbench-bones", skeleton.id))
            .max_height(280.0)
            .show(ui, |ui| {
                for bone in skeleton.bones.iter() {
                    let key = (skeleton.id, bone.index);
                    let response = ui.selectable_label(
                        self.bone == Some(key),
                        format!(
                            "节点 {} · 父节点 {}",
                            bone.index,
                            bone.parent
                                .map_or_else(|| "—".into(), |parent| parent.to_string())
                        ),
                    );
                    if response.clicked() {
                        self.bone = (self.bone != Some(key)).then_some(key);
                    }
                    if enabled && response.double_clicked() {
                        self.bone = Some(key);
                        self.send(Command::FocusBone {
                            skeleton: skeleton.id,
                            node: Some(bone.index),
                        });
                    }
                    if self.bone == Some(key) {
                        ui.indent(("bone-properties", skeleton.id, bone.index), |ui| {
                            egui::Frame::new()
                                .fill(ui.visuals().faint_bg_color)
                                .inner_margin(egui::Margin::symmetric(8, 6))
                                .show(ui, |ui| {
                                    ui.columns(3, |columns| {
                                        for (axis, column) in columns.iter_mut().enumerate() {
                                            column.weak(["X", "Y", "Z"][axis]);
                                            let value = format!("{:.2}", bone.position[axis]);
                                            column
                                                .add(
                                                    egui::Label::new(
                                                        RichText::new(&value).monospace(),
                                                    )
                                                    .truncate(),
                                                )
                                                .on_hover_text(value);
                                        }
                                    });
                                    ui.add_space(4.0);
                                    ui.add_enabled_ui(enabled, |ui| {
                                        self.bone_binding_controls(ui, skeleton, bone.index)
                                    });
                                });
                        });
                    }
                }
            });
    }

    fn bone_binding_controls(
        &mut self,
        ui: &mut egui::Ui,
        skeleton: &crate::preview::LoadedSkeleton,
        node: usize,
    ) {
        let previous = skeleton.bone_bindings.get(node).copied().flatten();
        let mut source = previous;
        ui.label("姿态跟随");
        egui::ComboBox::from_id_salt(("workbench-bone-binding", skeleton.id, node))
            .width(ui.available_width())
            .selected_text(
                source.map_or_else(|| "原始骨架姿态".into(), |index| format!("节点 {index}")),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut source, None, "原始骨架姿态");
                for bone in skeleton.bones.iter().filter(|bone| bone.index != node) {
                    ui.selectable_value(
                        &mut source,
                        Some(bone.index),
                        format!("节点 {}", bone.index),
                    );
                }
            })
            .response
            .on_hover_text(
                "使用来源节点的世界姿态，保留当前节点自身的逆绑定矩阵；作用于此骨架及其关联模型。",
            );

        if source != previous {
            self.send(Command::BoneBinding {
                skeleton: skeleton.id,
                node,
                source,
            });
        }
    }

    fn draw_coordinates(&self, ui: &egui::Ui, snapshot: &Snapshot) {
        let Some(camera) = snapshot.camera.filter(|_| snapshot.ready) else {
            return;
        };
        if !self.view.show_grid && !self.view.show_axes {
            return;
        }
        let rect = self.viewport_rect.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(rect);
        let mut text = String::new();
        if self.view.show_axes {
            let bone = self.bone.and_then(|(id, index)| {
                snapshot
                    .skeletons
                    .iter()
                    .find(|skeleton| skeleton.id == id && visible_skeleton(snapshot, skeleton))
                    .and_then(|skeleton| skeleton.bones.iter().find(|bone| bone.index == index))
            });
            let position = bone.map_or(camera.target, |bone| bone.position);
            if let Some(bone) = bone {
                let _ = write!(text, "骨骼 {}", bone.index);
            } else {
                text.push_str("焦点");
            }
            let _ = write!(
                text,
                " · 世界坐标\nX {:+.2}  Y {:+.2}  Z {:+.2}",
                position[0], position[1], position[2]
            );
            if rect.width() >= 120.0 && rect.height() >= 180.0 {
                let center = egui::pos2(rect.right() - 58.0, rect.top() + 58.0);
                painter.circle_filled(center, 48.0, Color32::from_black_alpha(150));
                let (view, _) = camera.matrices(1.0, 200_000.0);
                let mut axes = [0, 1, 2];
                axes.sort_by(|&a, &b| view[a * 4 + 2].total_cmp(&view[b * 4 + 2]));
                for axis in axes {
                    let [r, g, b] = crate::guides::AXIS_COLORS[axis];
                    let color = Color32::from_rgb(r, g, b);
                    let direction = egui::vec2(view[axis * 4], -view[axis * 4 + 1]);
                    let end = center + direction * 32.0;
                    painter.line_segment([center, end], egui::Stroke::new(2.0, color));
                    painter.circle_filled(end, 3.0, color);
                    painter.text(
                        center + direction * 42.0,
                        egui::Align2::CENTER_CENTER,
                        ["X", "Y", "Z"][axis],
                        egui::FontId::monospace(13.0),
                        color,
                    );
                }
            }
        }
        if self.view.show_grid {
            if !text.is_empty() {
                text.push('\n');
            }
            let _ = write!(
                text,
                "网格间距 {} · 地面 Y=0",
                crate::guides::grid_spacing(camera)
            );
        }
        let galley = painter.layout(
            text,
            egui::FontId::monospace(12.0),
            Color32::WHITE,
            (rect.width() - 32.0).max(1.0),
        );
        let position = egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0 - galley.size().y);
        painter.rect_filled(
            egui::Rect::from_min_size(position, galley.size()).expand(5.0),
            4.0,
            Color32::from_black_alpha(175),
        );
        painter.galley(position, galley, Color32::WHITE);
    }

    fn draw_bones(&self, ui: &egui::Ui, snapshot: &Snapshot) {
        let Some(camera) = snapshot.camera.filter(|_| snapshot.ready) else {
            return;
        };
        if !self.view.show_bones && self.bone.is_none() {
            return;
        }
        let viewport = rendered_viewport(ui, snapshot);
        let painter = ui
            .painter()
            .with_clip_rect(ui.clip_rect().intersect(viewport));
        let point = |position| {
            camera.project(position).map(|[x, y]| {
                egui::pos2(
                    viewport.left() + x * viewport.width(),
                    viewport.top() + y * viewport.height(),
                )
            })
        };
        for skeleton in snapshot
            .skeletons
            .iter()
            .filter(|skeleton| visible_skeleton(snapshot, skeleton))
        {
            for bone in skeleton.bones.iter() {
                let selected = self.bone == Some((skeleton.id, bone.index));
                if !self.view.show_bones && !selected {
                    continue;
                }
                let Some(position) = point(bone.position) else {
                    continue;
                };
                let color = if selected {
                    Color32::GOLD
                } else {
                    Color32::from_rgba_unmultiplied(125, 210, 255, 180)
                };
                if let Some(parent) = bone
                    .parent
                    .and_then(|index| {
                        skeleton
                            .bones
                            .binary_search_by_key(&index, |bone| bone.index)
                            .ok()
                    })
                    .map(|index| &skeleton.bones[index])
                    && let Some(parent) = point(parent.position)
                {
                    painter.line_segment(
                        [parent, position],
                        egui::Stroke::new(if selected { 2.5 } else { 1.0 }, color),
                    );
                }
                painter.circle_filled(position, if selected { 5.0 } else { 2.0 }, color);
                if selected {
                    painter.text(
                        position + egui::vec2(8.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("节点 {}", bone.index),
                        egui::FontId::proportional(14.0),
                        color,
                    );
                }
            }
        }
    }
}

impl Drop for Workbench {
    fn drop(&mut self) {
        self.save_view();
    }
}

fn rendered_viewport(ui: &egui::Ui, snapshot: &Snapshot) -> egui::Rect {
    let screen = ui.ctx().content_rect();
    let region = snapshot.viewport;
    egui::Rect::from_min_size(
        screen.min + egui::vec2(region.x * screen.width(), region.y * screen.height()),
        egui::vec2(
            region.width * screen.width(),
            region.height * screen.height(),
        ),
    )
}

fn visible_skeleton(snapshot: &Snapshot, skeleton: &crate::preview::LoadedSkeleton) -> bool {
    skeleton.error.is_none()
        && !skeleton.bones.is_empty()
        && snapshot
            .resources
            .iter()
            .any(|entry| entry.id == skeleton.id && entry.enabled)
}

fn has_visible_resources(snapshot: &Snapshot) -> bool {
    snapshot
        .models
        .iter()
        .any(|model| model.visible && model.error.is_none())
        || snapshot
            .skeletons
            .iter()
            .any(|skeleton| visible_skeleton(snapshot, skeleton))
}

#[derive(Default)]
struct Directory {
    folders: BTreeMap<String, Directory>,
    files: Vec<usize>,
}

impl Directory {
    fn build(catalog: &Catalog, filtered: &[usize]) -> Self {
        let mut root = Self::default();
        for &index in filtered {
            let path = &catalog.entries[index].relative_path;
            let mut directory = &mut root;
            if let Some(parent) = path.parent() {
                for part in parent.components() {
                    directory = directory
                        .folders
                        .entry(part.as_os_str().to_string_lossy().into_owned())
                        .or_default();
                }
            }
            directory.files.push(index);
        }
        root
    }
}

#[allow(clippy::too_many_arguments)]
fn directory(
    ui: &mut egui::Ui,
    folder: &Directory,
    catalog: &Catalog,
    path: Option<&PathBuf>,
    document: Option<&Arc<Document>>,
    resource_counts: &[usize],
    show_encoding_layers: bool,
    node: &mut usize,
    selection: &mut Option<ResourceRef>,
    load: &mut Option<PathBuf>,
    load_resource: &mut Option<ResourceRef>,
    details: &mut Option<usize>,
    expand: bool,
) {
    for (name, child) in &folder.folders {
        ui.push_id(name, |ui| {
            egui::CollapsingHeader::new(name)
                .default_open(expand)
                .show(ui, |ui| {
                    directory(
                        ui,
                        child,
                        catalog,
                        path,
                        document,
                        resource_counts,
                        show_encoding_layers,
                        node,
                        selection,
                        load,
                        load_resource,
                        details,
                        expand,
                    );
                });
        });
    }
    for &index in &folder.files {
        let entry = &catalog.entries[index];
        let name = entry
            .relative_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let selected = path == Some(&entry.path);
        if selected && let Some(document) = document {
            ui.push_id(&entry.path, |ui| {
                let _ = tree(
                    ui,
                    &ResourceRef::new(document.clone(), document.root),
                    resource_counts,
                    show_encoding_layers,
                    node,
                    selection,
                    load_resource,
                    details,
                );
            });
        } else {
            let response = ui
                .horizontal(|ui| {
                    ui.add_space(ui.spacing().indent);
                    tree_row(ui, name.as_ref(), selected, 0, None).0
                })
                .inner;
            if response.clicked() && !selected {
                *load = Some(entry.path.clone());
            }
            response.on_hover_text(format!("{} · {} 字节", entry.encoding(), entry.size));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn tree(
    ui: &mut egui::Ui,
    source: &ResourceRef,
    resource_counts: &[usize],
    show_encoding_layers: bool,
    selected: &mut usize,
    selection: &mut Option<ResourceRef>,
    load_resource: &mut Option<ResourceRef>,
    details: &mut Option<usize>,
) -> Option<(egui::Response, Option<egui::Response>)> {
    let document = &source.document;
    let original = document.nodes.get(source.node)?;
    let root = source.node == document.root;
    let index = visible_node(document, source.node, show_encoding_layers);
    let source = if index == source.node {
        source.clone()
    } else {
        source.related(index)?
    };
    let node = document.nodes.get(index)?;
    let name = if root {
        original
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&original.name)
    } else {
        &original.name
    };
    let label = format!("{name} · {}", node.kind);
    let resource_count = resource_counts.get(index).copied().unwrap_or(0);
    let kind = crate::preview::resource_kind(document, index);
    let selected_row = *selected == index
        && selection
            .as_ref()
            .filter(|source| source.node == *selected)
            .is_none_or(|selected| selected.same_origin(&source));
    let response = if node.children.is_empty() && !node.deferred {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            tree_row(ui, &label, selected_row, resource_count, Some(kind))
        })
        .inner
    } else {
        let mut clicked = false;
        let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id(("resource-node", index)),
            root,
        )
        .show_header(ui, |ui| {
            let response = tree_row(ui, &label, selected_row, resource_count, Some(kind));
            clicked = response.0.clicked();
            response
        });
        if clicked {
            header.toggle();
        }
        let (_, header, _) = header.body(|ui| {
            if node.deferred {
                *details = Some(index);
                ui.spinner();
            }
            // Closed branches and leaves occupy one row. Only expanded branches
            // need recursive layout; clip fixed rows before creating widgets.
            let mut start = 0;
            while start < node.children.len() {
                let context = ui.ctx().clone();
                let parent_id = ui.id();
                let is_leaf = |child: usize| {
                    let row_id = parent_id.with(("resource-row", child));
                    let child = visible_node(document, child, show_encoding_layers);
                    let node = &document.nodes[child];
                    (node.children.is_empty() && !node.deferred)
                        || egui::collapsing_header::CollapsingState::load(
                            &context,
                            row_id.with(("resource-node", child)),
                        )
                        .is_none_or(|state| state.openness(&context) == 0.0)
                };
                let end = if is_leaf(node.children[start]) {
                    start
                        + node.children[start..]
                            .iter()
                            .take_while(|&&child| is_leaf(child))
                            .count()
                } else {
                    start + 1
                };
                let mut render = |ui: &mut egui::Ui, rows: std::ops::Range<usize>| {
                    for offset in rows {
                        let child = source.child_at(start + offset).unwrap();
                        // Neither clipping nor splitting runs around expanded
                        // siblings may change a row's widget or collapse IDs.
                        ui.scope_builder(
                            egui::UiBuilder::new().id(parent_id.with(("resource-row", child.node))),
                            |ui| {
                                tree(
                                    ui,
                                    &child,
                                    resource_counts,
                                    show_encoding_layers,
                                    selected,
                                    selection,
                                    load_resource,
                                    details,
                                );
                            },
                        );
                    }
                };
                if is_leaf(node.children[start]) {
                    visible_leaf_rows(ui, end - start, &mut render);
                } else {
                    render(ui, 0..1);
                }
                start = end;
            }
        });
        header.inner
    };
    if response.0.clicked() {
        *selected = index;
        *selection = Some(source.clone());
    }
    if response.1.as_ref().is_some_and(egui::Response::clicked) {
        *selected = index;
        *selection = Some(source.clone());
        *load_resource = Some(source);
    }
    Some(response)
}

/// Reserve the whole run, but only build widgets intersecting the viewport.
fn visible_leaf_rows(
    ui: &mut egui::Ui,
    count: usize,
    render: impl FnOnce(&mut egui::Ui, std::ops::Range<usize>),
) {
    let spacing = ui.spacing().item_spacing.y;
    let stride = ui.spacing().interact_size.y + spacing;
    let top = ui.next_widget_position().y;
    let first = (((ui.clip_rect().top() - top) / stride).floor().max(0.0) as usize).min(count);
    let end = ((((ui.clip_rect().bottom() - top) / stride).ceil().max(0.0) as usize)
        .saturating_add(1))
    .min(count)
    .max(first);
    let rect = egui::Rect::from_min_size(
        ui.next_widget_position(),
        egui::vec2(
            ui.available_width(),
            (stride * count as f32 - spacing).max(0.0),
        ),
    );
    let visible = egui::Rect::from_min_max(
        egui::pos2(rect.left(), top + first as f32 * stride),
        rect.max,
    );
    ui.allocate_space(rect.size());
    // The renderer assigns row IDs independently of this transient run UI.
    let mut rows = ui.new_child(egui::UiBuilder::new().max_rect(visible));
    render(&mut rows, first..end);
}

fn visible_node(document: &Document, original: usize, show_encoding_layers: bool) -> usize {
    if show_encoding_layers {
        return original;
    }
    let mut index = original;
    // Inspection trees are acyclic; also keep malformed chains visible if
    // an invalid link or cycle ever reaches the UI.
    for _ in &document.nodes {
        let Some(node) = document.nodes.get(index) else {
            return original;
        };
        if node.error.is_some()
            || !matches!(node.kind, Kind::Ecd | Kind::Exf | Kind::Jkr)
            || node.children.len() != 1
            || document.nodes.get(node.children[0]).is_none()
        {
            return index;
        }
        index = node.children[0];
    }
    original
}

fn resource_action_width(ui: &egui::Ui, count: usize) -> f32 {
    let text_width = |text: egui::WidgetText| {
        text.into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
        .ceil()
    };
    let button_width =
        text_width(RichText::new("加载").small().into()) + ui.spacing().button_padding.x * 2.0;
    if count == 0 {
        button_width
    } else {
        button_width
            + ui.spacing().item_spacing.x
            + text_width(RichText::new(format!("资源 {count}")).strong().into())
    }
}

fn tree_row(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    resource_count: usize,
    kind: Option<Kind>,
) -> (egui::Response, Option<egui::Response>) {
    let help = match kind {
        Some(kind) if effects::is_binding(kind) || effects::is_definition(kind) => {
            Some("加载特效并解析自身的模型绑定".to_owned())
        }
        Some(Kind::Motion) => Some("加载动画并匹配关联骨架".to_owned()),
        Some(Kind::Fmod | Kind::Fskl | Kind::Txb | Kind::Png | Kind::Dds) => {
            Some("立即加载此资源到当前工作台".to_owned())
        }
        _ if resource_count != 0 => Some(format!("加载所选目录内的 {resource_count} 个资源")),
        _ => None,
    };
    let height = ui.spacing().interact_size.y;
    let width =
        (ui.clip_rect().right().min(ui.max_rect().right()) - ui.next_widget_position().x).max(0.0);
    let action_width = help
        .as_ref()
        .map_or(0.0, |_| resource_action_width(ui, resource_count))
        .min(width);
    let name_width = if help.is_some() {
        (width - action_width - ui.spacing().item_spacing.x).max(0.0)
    } else {
        width
    };
    let label = ui
        .add_sized(
            [name_width, height],
            egui::Button::selectable(selected, ())
                .left_text(label)
                .truncate(),
        )
        .on_hover_text(label);
    let button = help.map(|help| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(action_width, height), egui::Sense::hover());
        let mut actions = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let response = actions.small_button("加载").on_hover_text(help);
        if resource_count != 0 {
            actions.add(
                egui::Label::new(
                    RichText::new(format!("资源 {resource_count}"))
                        .strong()
                        .color(ui.visuals().selection.stroke.color),
                )
                .extend(),
            );
        }
        response
    });
    (label, button)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Field, Node};
    use crate::preview::AssetBundle;

    #[test]
    fn large_leaf_runs_only_render_the_viewport_and_keep_full_height() {
        let context = egui::Context::default();
        for row in [0, 5_000, 9_990] {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(300.0, 400.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let stride = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                        egui::ScrollArea::vertical()
                            .vertical_scroll_offset(row as f32 * stride)
                            .show(ui, |ui| {
                                let top = ui.next_widget_position().y;
                                let stride =
                                    ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                                visible_leaf_rows(ui, 10_000, |ui, range| {
                                    assert!(range.len() < 30, "rendered {} rows", range.len());
                                    assert!(!range.is_empty());
                                    for row in range {
                                        ui.horizontal(|ui| {
                                            tree_row(ui, &row.to_string(), false, 0, None);
                                        });
                                    }
                                });
                                assert!(
                                    (ui.next_widget_position().y - top - stride * 10_000.0).abs()
                                        < 1.0
                                );
                            });
                    },
                )
                .drop_without_applying_deltas();
        }
    }

    fn loaded_effect_fixture(
        source: &ResourceRef,
        bindings: Vec<effects::BindingSnapshot>,
    ) -> Arc<Vec<crate::preview::LoadedEffect>> {
        Arc::new(
            bindings
                .into_iter()
                .map(|binding| crate::preview::LoadedEffect {
                    id: binding.id,
                    source: source.clone(),
                    enabled: true,
                    model: Some(41),
                    automatic: false,
                    manual_target: Some(41),
                    model_id: None,
                    message: "已手动绑定".into(),
                    binding,
                })
                .collect(),
        )
    }

    #[test]
    fn long_effect_target_names_do_not_expand_the_container_or_popup() {
        let (_, source) = effects::tests::fixture();
        let effects = effects::Effects {
            bindings: vec![effects::Binding::read(source.clone()).unwrap()],
            ..Default::default()
        };
        let snapshot = Snapshot {
            loaded_effects: loaded_effect_fixture(&source, effects.sample(0.0).bindings),
            models: Arc::new(vec![LoadedModel {
                id: 41,
                resources: AssetBundle::find_with_nodes(multiple_models()).0.remove(0),
                name: format!(
                    "Z:/game/dat/extend/archive/{} · 模型 1",
                    "long-model-name-".repeat(30)
                )
                .into(),
                visible: true,
                error: None,
                meshes: Arc::default(),
            }]),
            ..Default::default()
        };
        for width in [260.0, 380.0] {
            let mut workbench = preview_fixture();
            let context = egui::Context::default();
            let time = std::cell::Cell::new(0.0);
            let draw = |workbench: &mut Workbench, events| {
                time.set(time.get() + 0.016);
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 600.0),
                        )),
                        time: Some(time.get()),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(width);
                        let right = ui.max_rect().right();
                        workbench.effect_controls(ui, &snapshot);
                        assert!(
                            ui.min_rect().right() <= right + 1.0,
                            "binding content expanded the panel"
                        );
                    },
                )
            };
            let mut target = None;
            for _ in 0..4 {
                let output = draw(&mut workbench, vec![]);
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.text().contains("long-model-name")
                    {
                        assert!(text.galley.elided);
                        assert!(text.galley.size().x <= width);
                        target = Some(text.pos + text.galley.rect.center().to_vec2());
                    }
                }
                output.drop_without_applying_deltas();
            }
            let target = target.unwrap();
            for pressed in [true, false] {
                draw(&mut workbench, pointer(target, pressed)).drop_without_applying_deltas();
            }
            let output = draw(&mut workbench, vec![]);
            let names = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().contains("long-model-name") => {
                        Some(text)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(names.len(), 2, "the popup must be open");
            assert!(names.iter().all(|text| text.galley.size().x <= width));
            output.drop_without_applying_deltas();
            assert!(workbench.control.commands().is_empty());
        }
    }

    #[test]
    fn effect_target_choice_overrides_auto_and_keeps_disabled_manual_targets_until_unbound() {
        let (_, source) = effects::tests::fixture();
        let effects = effects::Effects {
            bindings: vec![effects::Binding::read(source.clone()).unwrap()],
            ..Default::default()
        };
        let binding = effects.bindings[0].id;
        let mut snapshot = Snapshot {
            loaded_effects: loaded_effect_fixture(&source, effects.sample(0.0).bindings),
            models: Arc::new(
                AssetBundle::find_with_nodes(multiple_models())
                    .0
                    .into_iter()
                    .zip([41, 42])
                    .map(|(resources, id)| LoadedModel {
                        id,
                        resources,
                        name: format!("fixture model {id}").into(),
                        visible: true,
                        error: None,
                        meshes: Arc::default(),
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let effect = &mut Arc::make_mut(&mut snapshot.loaded_effects)[0];
        effect.automatic = true;
        effect.manual_target = None;
        effect.model_id = Some(44);
        let first_name = snapshot.models[0].name.to_string();
        let second_name = snapshot.models[1].name.to_string();
        let mut workbench = preview_fixture();
        workbench.loaded_document(multiple_models());
        let context = egui::Context::default();
        context.all_styles_mut(|style| style.animation_time = 0.0);
        let draw = |workbench: &mut Workbench, snapshot: &Snapshot, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 450.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| workbench.effect_controls(ui, snapshot),
            )
        };
        let caption = |output: &egui::FullOutput, caption: &str| {
            output
                .shapes
                .iter()
                .rev()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == caption => {
                        Some(text.pos + text.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
        };
        let choose = |workbench: &mut Workbench,
                      snapshot: &Snapshot,
                      selected: &str,
                      option: &str| {
            let output = draw(workbench, snapshot, vec![]);
            let button = caption(&output, selected).expect("selected target must remain visible");
            output.drop_without_applying_deltas();
            for pressed in [true, false] {
                draw(workbench, snapshot, pointer(button, pressed)).drop_without_applying_deltas();
            }
            let output = draw(workbench, snapshot, vec![]);
            let option = caption(&output, option).expect("target option must be selectable");
            output.drop_without_applying_deltas();
            for pressed in [true, false] {
                draw(workbench, snapshot, pointer(option, pressed)).drop_without_applying_deltas();
            }
        };
        choose(
            &mut workbench,
            &snapshot,
            &format!("自动 · {first_name}"),
            &second_name,
        );
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::BindEffect { binding: id, model: Some(42) }] if *id == binding
        ));
        let effect = &mut Arc::make_mut(&mut snapshot.loaded_effects)[0];
        effect.automatic = false;
        effect.manual_target = Some(42);
        effect.enabled = false;
        effect.model = None;
        // Disabled effects have no resolved model, but must retain the manual
        // choice so explicitly selecting "unbound" can clear that request.
        choose(&mut workbench, &snapshot, &second_name, "未绑定");
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::BindEffect { binding: id, model: None }] if *id == binding
        ));
        Arc::make_mut(&mut snapshot.loaded_effects)[0].manual_target = None;
        choose(&mut workbench, &snapshot, "未绑定", "自动匹配 · 模型 ID 44");
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::AutoBindEffect(id)] if *id == binding
        ));
    }

    #[test]
    fn manual_effect_buttons_target_the_definition_without_loading_a_motion() {
        let (_, source) = effects::tests::fixture();
        let mut effects = effects::Effects {
            bindings: vec![effects::Binding::read(source.clone()).unwrap()],
            ..Default::default()
        };
        let binding = effects.bindings[0].id;
        let mut workbench = preview_fixture();
        let context = egui::Context::default();
        let mut snapshot = Snapshot {
            loaded_effects: loaded_effect_fixture(&source, effects.sample(0.0).bindings),
            ..Default::default()
        };
        let draw = |workbench: &mut Workbench, snapshot: &Snapshot, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 450.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| workbench.effect_controls(ui, snapshot),
            )
        };
        for (caption, stop) in [("触发", false), ("停止", true)] {
            let output = draw(&mut workbench, &snapshot, vec![]);
            let point = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == caption => {
                        Some(text.pos + text.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
                .unwrap();
            output.drop_without_applying_deltas();
            draw(&mut workbench, &snapshot, pointer(point, true)).drop_without_applying_deltas();
            draw(&mut workbench, &snapshot, pointer(point, false)).drop_without_applying_deltas();
            let commands = workbench.control.commands();
            if stop {
                assert!(
                    matches!(commands.as_slice(), [Command::StopEffectDefinition { binding: id, slot: 0 }] if *id == binding)
                );
            } else {
                assert!(
                    matches!(commands.as_slice(), [Command::TriggerEffectDefinition { binding: id, slot: 0 }] if *id == binding)
                );
                effects.trigger(binding, 0, 20.0).unwrap();
                snapshot.loaded_effects =
                    loaded_effect_fixture(&source, effects.sample(20.0).bindings);
            }
        }
    }

    #[test]
    fn definition_rows_trigger_immediately_and_retain_sources_across_file_switches() {
        let (attachment, model) = effects::tests::fixture();
        let mut document = crate::inspect::expand(&model.document, model.node).unwrap();
        document = crate::inspect::expand(&document, attachment.node).unwrap();
        let definition = document.nodes[model.node].children[0];
        let other = document.nodes[attachment.node].children[0];
        let document = Arc::new(document);
        let mut workbench = preview_fixture();
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        let (_, button, _, _) = draw_tree(&mut workbench, &context, definition, 0.0, vec![]);
        let center = button.unwrap().rect.center();
        draw_tree(
            &mut workbench,
            &context,
            definition,
            0.1,
            pointer(center, true),
        );
        draw_tree(
            &mut workbench,
            &context,
            definition,
            0.2,
            pointer(center, false),
        );
        workbench.load_node(other);
        let commands = workbench.control.commands();
        assert!(
            matches!(commands.as_slice(), [Command::LoadResource(first), Command::LoadResource(second)] if first.node == definition && second.node == other)
        );
        workbench.loaded_document(multiple_models());
        let Command::LoadResource(first) = &commands[0] else {
            panic!()
        };
        assert!(Arc::ptr_eq(&first.document, &document));
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn independent_tracks_fill_their_own_width_and_target_only_the_selected_track() {
        let (_, source) = effects::tests::fixture();
        let effects = effects::Effects {
            bindings: vec![effects::Binding::read(source.clone()).unwrap()],
            ..Default::default()
        };
        let mut bindings = effects.sample(0.0).bindings;
        let binding = bindings[0].id;
        let definition = &mut bindings[0].definitions[0];
        definition.frames = 30.0;
        definition.delay = 0;
        definition.frame = Some(5.0);
        definition.active = true;
        let mut snapshot = Snapshot {
            motions: Arc::new(vec![crate::preview::LoadedMotion {
                id: 7,
                source: ResourceRef::new(multiple_models(), 9),
                enabled: true,
                frames: 60.0,
                frame: Some(15.0),
                skeleton: None,
            }]),
            loaded_effects: loaded_effect_fixture(&source, bindings),
            ..Default::default()
        };
        let mut workbench = preview_fixture();
        let context = egui::Context::default();
        let draw = |workbench: &mut Workbench, snapshot: &Snapshot, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 200.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    workbench.motion_list(ui, snapshot);
                    for effect in snapshot.loaded_effects.iter() {
                        for entry in &effect.binding.definitions {
                            workbench.effect_entry_track(ui, &effect.binding, entry);
                        }
                    }
                },
            )
        };
        let track_rects = |output: &egui::FullOutput| {
            [
                Color32::from_rgb(75, 115, 205),
                Color32::from_rgb(40, 150, 165),
            ]
            .map(|fill| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.fill == fill => Some(rect.rect),
                        _ => None,
                    })
                    .unwrap()
            })
        };
        let output = draw(&mut workbench, &snapshot, vec![]);
        let [motion, effect] = track_rects(&output);
        let step = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "+1" => {
                    let center = text.pos + text.galley.rect.center().to_vec2();
                    ((center.y - effect.center().y).abs() < 8.0).then_some(center)
                }
                _ => None,
            })
            .unwrap();
        output.drop_without_applying_deltas();
        assert!(motion.width() > 0.0);
        assert!((motion.left() - effect.left()).abs() < 0.1);
        assert!((motion.right() - effect.right()).abs() < 0.1);
        let point = egui::pos2(effect.left() + effect.width() * 0.75, effect.center().y);
        draw(&mut workbench, &snapshot, pointer(point, true)).drop_without_applying_deltas();
        draw(&mut workbench, &snapshot, pointer(point, false)).drop_without_applying_deltas();
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::Seek { track: PlaybackTrack::Effect { binding: id, slot: 0 }, frame }] if *id == binding && (*frame - 22.5).abs() < 0.1)
        );
        draw(&mut workbench, &snapshot, pointer(step, true)).drop_without_applying_deltas();
        draw(&mut workbench, &snapshot, pointer(step, false)).drop_without_applying_deltas();
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::Step { track: PlaybackTrack::Effect { binding: id, slot: 0 }, delta: 1 }] if *id == binding)
        );
        let point = egui::pos2(motion.left() + motion.width() * 0.75, motion.center().y);
        draw(&mut workbench, &snapshot, pointer(point, true)).drop_without_applying_deltas();
        draw(&mut workbench, &snapshot, pointer(point, false)).drop_without_applying_deltas();
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::Seek { track: PlaybackTrack::Motion(7), frame }] if (*frame - 45.0).abs() < 0.1)
        );
        Arc::make_mut(&mut snapshot.loaded_effects)[0]
            .binding
            .definitions[0]
            .delay = 6;
        let output = draw(&mut workbench, &snapshot, vec![]);
        let [motion, effect] = track_rects(&output);
        output.drop_without_applying_deltas();
        assert!((effect.left() - motion.left() - motion.width() * 0.2).abs() < 0.1);
        assert!((effect.right() - motion.right()).abs() < 0.1);
        Arc::make_mut(&mut snapshot.loaded_effects)[0]
            .binding
            .definitions[0]
            .frame = None;
        let point = effect.center();
        draw(&mut workbench, &snapshot, pointer(point, true)).drop_without_applying_deltas();
        draw(&mut workbench, &snapshot, pointer(point, false)).drop_without_applying_deltas();
        draw(&mut workbench, &snapshot, pointer(step, true)).drop_without_applying_deltas();
        draw(&mut workbench, &snapshot, pointer(step, false)).drop_without_applying_deltas();
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn multiple_motion_rows_keep_controls_scoped_after_browsing_another_file() {
        let mut document = (*multiple_models()).clone();
        let mut second_motion = document.nodes[9].clone();
        second_motion.name = "second-motion".into();
        document.nodes.push(second_motion);
        document.nodes[0].children.push(12);
        let document = Arc::new(document);
        let snapshot = Snapshot {
            models: Arc::new(vec![LoadedModel {
                id: 41,
                resources: AssetBundle::find_with_nodes(document.clone()).0.remove(0),
                name: "shared model".into(),
                visible: true,
                error: None,
                meshes: Arc::default(),
            }]),
            motions: Arc::new(
                [(7, 9, 60.0), (19, 12, 120.0)]
                    .into_iter()
                    .map(|(id, node, frames)| crate::preview::LoadedMotion {
                        id,
                        source: ResourceRef::new(document.clone(), node),
                        enabled: true,
                        frames,
                        frame: Some(15.0),
                        skeleton: None,
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let mut workbench = preview_fixture();
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        let draw = |workbench: &mut Workbench, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    workbench.motion_list(ui, &snapshot);
                    workbench.timeline(ui, &snapshot);
                },
            )
        };
        draw(&mut workbench, vec![]).drop_without_applying_deltas();
        workbench.loaded_document(multiple_models());
        let output = draw(&mut workbench, vec![]);
        let tracks = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(75, 115, 205) => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tracks.len(),
            2,
            "global controls must not duplicate motion tracks"
        );
        assert!(tracks[0].width() > 0.0);
        assert!((tracks[0].width() - tracks[1].width()).abs() < 0.1);
        let second_caption = |caption| {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == caption => {
                        Some(text.pos + text.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
                .nth(1)
                .unwrap()
        };
        let step = second_caption("+1");
        let unload = second_caption("卸载");
        let checkbox = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if (rect.rect.width() - rect.rect.height()).abs() < 0.1
                        && rect.rect.width() > 2.0
                        && (rect.rect.center().y - unload.y).abs() < 2.0 =>
                {
                    Some(rect.rect.center())
                }
                _ => None,
            })
            .unwrap();
        let seek = egui::pos2(
            tracks[1].left() + tracks[1].width() * 0.75,
            tracks[1].center().y,
        );
        output.drop_without_applying_deltas();
        assert!(workbench.control.commands().is_empty());
        for pressed in [true, false] {
            draw(&mut workbench, pointer(seek, pressed)).drop_without_applying_deltas();
        }
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::Seek { track: PlaybackTrack::Motion(19), frame }] if (*frame - 90.0).abs() < 0.1)
        );
        for pressed in [true, false] {
            draw(&mut workbench, pointer(step, pressed)).drop_without_applying_deltas();
        }
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::Step {
                track: PlaybackTrack::Motion(19),
                delta: 1
            }]
        ));
        for pressed in [true, false] {
            draw(&mut workbench, pointer(checkbox, pressed)).drop_without_applying_deltas();
        }
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::MotionEnabled {
                id: 19,
                enabled: false
            }]
        ));
        for pressed in [true, false] {
            draw(&mut workbench, pointer(unload, pressed)).drop_without_applying_deltas();
        }
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::RemoveMotion(19)]
        ));
        assert!(Arc::ptr_eq(&snapshot.motions[1].source.document, &document));
    }

    #[test]
    fn expanded_skeleton_controls_keep_bone_selection_and_commands_scoped_to_their_resource() {
        let document = multiple_models();
        let snapshot = Snapshot {
            resources: Arc::new(
                [(91, 2), (92, 6)]
                    .into_iter()
                    .map(|(id, node)| crate::preview::LoadedResource {
                        id,
                        source: ResourceRef::new(document.clone(), node),
                        enabled: true,
                    })
                    .collect(),
            ),
            skeletons: Arc::new(
                [91, 92]
                    .into_iter()
                    .map(|id| crate::preview::LoadedSkeleton {
                        id,
                        bones: Arc::new(vec![
                            crate::preview::Bone {
                                index: 0,
                                parent: None,
                                position: [0.0; 3],
                            },
                            crate::preview::Bone {
                                index: 1,
                                parent: Some(0),
                                position: [1.0, 2.0, 3.0],
                            },
                        ]),
                        bone_bindings: Arc::new(vec![None, Some(0)]),
                        error: None,
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let mut workbench = preview_fixture();
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        context.all_styles_mut(|style| style.animation_time = 0.0);
        let time = std::cell::Cell::new(0.0);
        let draw = |workbench: &mut Workbench, events| {
            time.set(time.get() + 0.05);
            context.run_ui(
                egui::RawInput {
                    time: Some(time.get()),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| workbench.resource_list(ui, &snapshot, Kind::Fskl),
            )
        };
        let caption = |output: &egui::FullOutput, caption: &str| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == caption => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
        };
        let output = draw(&mut workbench, vec![]);
        assert!(caption(&output, "节点 1 · 父节点 0").is_none());
        let expand = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Path(path)
                    if path.closed
                        && path.points.len() == 3
                        && path.fill != Color32::TRANSPARENT =>
                {
                    Some(egui::Rect::from_points(&path.points).center())
                }
                _ => None,
            })
            .nth(1)
            .unwrap();
        output.drop_without_applying_deltas();
        for pressed in [true, false] {
            draw(&mut workbench, pointer(expand, pressed)).drop_without_applying_deltas();
        }
        workbench.loaded_document(multiple_models());
        let output = draw(&mut workbench, vec![]);
        let focus = caption(&output, "聚焦全部").unwrap();
        let clear = caption(&output, "清除姿态跟随").unwrap();
        let bone = caption(&output, "节点 1 · 父节点 0").unwrap();
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|shape| matches!(
                    &shape.shape,
                    egui::Shape::Text(text) if text.galley.text() == "节点 1 · 父节点 0"
                ))
                .count(),
            1,
            "opening the second skeleton must leave the first skeleton collapsed"
        );
        output.drop_without_applying_deltas();
        assert!(workbench.control.commands().is_empty());
        for pressed in [true, false] {
            draw(&mut workbench, pointer(focus, pressed)).drop_without_applying_deltas();
        }
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::FocusBone {
                skeleton: 92,
                node: None
            }]
        ));
        for pressed in [true, false] {
            draw(&mut workbench, pointer(clear, pressed)).drop_without_applying_deltas();
        }
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::ClearBoneBindings(92)]
        ));
        // Isolate this double click from the preceding toolbar clicks; egui's
        // triple-click window otherwise includes the fast clear-button click.
        time.set(time.get() + 1.0);
        for pressed in [true, false, true, false] {
            draw(&mut workbench, pointer(bone, pressed)).drop_without_applying_deltas();
        }
        assert_eq!(workbench.bone, Some((92, 1)));
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::FocusBone {
                skeleton: 92,
                node: Some(1)
            }]
        ));
        let output = draw(&mut workbench, vec![]);
        let binding = caption(&output, "节点 0").unwrap();
        output.drop_without_applying_deltas();
        for pressed in [true, false] {
            draw(&mut workbench, pointer(binding, pressed)).drop_without_applying_deltas();
        }
        let output = draw(&mut workbench, vec![]);
        let original_pose = caption(&output, "原始骨架姿态").unwrap();
        output.drop_without_applying_deltas();
        for pressed in [true, false] {
            draw(&mut workbench, pointer(original_pose, pressed)).drop_without_applying_deltas();
        }
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::BoneBinding {
                skeleton: 92,
                node: 1,
                source: None
            }]
        ));
        time.set(time.get() + 1.0);
        let output = draw(&mut workbench, vec![]);
        assert!(caption(&output, "姿态跟随").is_some());
        assert!(caption(&output, "X").is_some());
        let bone = caption(&output, "节点 1 · 父节点 0").unwrap();
        output.drop_without_applying_deltas();
        for pressed in [true, false] {
            draw(&mut workbench, pointer(bone, pressed)).drop_without_applying_deltas();
        }
        assert_eq!(workbench.bone, None);
        let output = draw(&mut workbench, vec![]);
        assert!(caption(&output, "节点 1 · 父节点 0").is_some());
        assert!(caption(&output, "姿态跟随").is_none());
        assert!(caption(&output, "X").is_none());
        output.drop_without_applying_deltas();
        assert!(workbench.control.commands().is_empty());
        assert!(Arc::ptr_eq(
            &snapshot.resources[1].source.document,
            &document
        ));
    }

    fn preview_fixture() -> Workbench {
        let root = std::env::temp_dir().join("mhf-workbench-multiple-model-fixture");
        let mut worker = Worker::start(root.clone(), root.clone()).unwrap();
        worker.stop();
        let worker = Arc::new(worker);
        let _ = worker.updates();
        let mut workbench = Workbench::new(
            Arc::new(Control::default()),
            worker,
            root,
            ViewSettings::default(),
            None,
        );
        workbench.scanning = false;
        workbench
    }

    fn multiple_models() -> Arc<Document> {
        let node = |name: &str, kind, children| Node {
            name: name.into(),
            kind,
            buffer: 0,
            range: 0..16,
            children,
            action: None,
            deferred: false,
            error: None,
            fields: Vec::new(),
            metadata: Default::default(),
        };
        let mut document = Document {
            root: 0,
            buffers: vec![Arc::from([0_u8; 16])],
            nodes: vec![
                node(
                    "models.pac",
                    Kind::Archive,
                    vec![1, 2, 3, 5, 6, 7, 9, 10, 11],
                ),
                node("geometry-1", Kind::Fmod, vec![]),
                node("skeleton-1", Kind::Fskl, vec![]),
                node("textures-1", Kind::Txb, vec![4]),
                node("image-1", Kind::Png, vec![]),
                node("geometry-2", Kind::Fmod, vec![]),
                node("skeleton-2", Kind::Fskl, vec![]),
                node("textures-2", Kind::Txb, vec![8]),
                node("image-2", Kind::Png, vec![]),
                node("motion", Kind::Motion, vec![]),
                node("effect-bank", Kind::EffectBank, vec![]),
                node("unknown", Kind::Unknown, vec![]),
            ],
        };
        for (scope, value) in crate::metadata::model_resources(&document) {
            document.nodes[scope].metadata.insert(value);
        }
        Arc::new(document)
    }

    fn encoded_models() -> Arc<Document> {
        let mut document = (*multiple_models()).clone();
        for (layer, kind) in [Kind::Ecd, Kind::Exf, Kind::Jkr].into_iter().enumerate() {
            let mut node = document.nodes[0].clone();
            node.name = format!("encoded-member-{layer}.bin");
            node.kind = kind;
            node.buffer = document.buffers.len();
            node.children = vec![if layer == 2 { 0 } else { 13 + layer }];
            document.buffers.push(Arc::from([layer as u8 + 1; 16]));
            document.nodes.push(node);
        }
        document.root = 12;
        Arc::new(document)
    }

    #[test]
    fn directory_loading_stays_inside_the_selected_subtree() {
        let mut document = (*multiple_models()).clone();
        let mut directory = document.nodes[0].clone();
        directory.name = "model-offset-directory".into();
        directory.children = vec![1, 2];
        document.nodes.push(directory);
        document.nodes[0].children = vec![12, 3, 5, 6, 7, 9, 10, 11];
        let document = Arc::new(document);
        let resources = |node| {
            ResourceRef::new(document.clone(), node)
                .loadable_resources()
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>()
        };
        assert_eq!(resources(12), [1, 2]);
        assert_eq!(resources(1), [1]);
        assert_eq!(resources(3), [4]);
        assert_eq!(resources(0), [1, 2, 4, 5, 6, 8, 9]);
        // FMOD inspector children are fields, not independently loaded resources.
        let mut nested = (*document).clone();
        nested.nodes[1].children = vec![4];
        assert_eq!(
            ResourceRef::new(Arc::new(nested), 12)
                .loadable_resources()
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn opening_or_refreshing_a_file_only_identifies_its_model_groups() {
        let mut workbench = preview_fixture();
        workbench.loaded_document(multiple_models());
        assert!(workbench.control.commands().is_empty());
        assert_eq!(workbench.resource_counts[0], 7);
        workbench.load_node(0);
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadResource(source)] if source.node == 0)
        );
        workbench.loaded_document(multiple_models());
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn leaf_load_buttons_dispatch_only_the_selected_resource() {
        let mut workbench = preview_fixture();
        let document = multiple_models();
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        for index in [1, 5] {
            let (_, button, _, _) =
                draw_tree(&mut workbench, &context, index, index as f64, vec![]);
            let point = button.unwrap().rect.center();
            draw_tree(
                &mut workbench,
                &context,
                index,
                index as f64 + 0.1,
                pointer(point, true),
            );
            draw_tree(
                &mut workbench,
                &context,
                index,
                index as f64 + 0.2,
                pointer(point, false),
            );
            assert!(
                matches!(workbench.control.commands().as_slice(), [Command::LoadResource(source)] if source.node == index)
            );
        }
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn browsing_and_document_updates_preserve_the_selected_inspector_tab() {
        for tab in [InspectorTab::Loaded, InspectorTab::Resource] {
            let mut workbench = preview_fixture();
            let document = multiple_models();
            let first = workbench.root.join("models.pac");
            let second = workbench.root.join("other.pac");
            workbench.catalog = Arc::new(Catalog {
                entries: [&first, &second]
                    .into_iter()
                    .map(|path| crate::catalog::Entry {
                        path: path.clone(),
                        relative_path: path.file_name().unwrap().into(),
                        size: 16,
                        header: Vec::new(),
                    })
                    .collect(),
                errors: Vec::new(),
            });
            workbench.filter_files();
            workbench.path = Some(first);
            workbench.tab = tab;
            workbench.loaded_document(document.clone());
            assert!(workbench.tab == tab);
            let context = egui::Context::default();
            let draw = |workbench: &mut Workbench, name: &str, events| {
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400.0, 900.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        workbench.resources(ui);
                    },
                );
                // Locate fixture resource names from their rendered geometry;
                // tab behavior does not depend on UI wording or fixed pixels.
                let position = output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with(name) => {
                        Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                    }
                    _ => None,
                });
                output.drop_without_applying_deltas();
                position
            };
            draw(&mut workbench, &document.nodes[1].name, vec![]);
            let node = draw(&mut workbench, &document.nodes[1].name, vec![]).unwrap();
            for pressed in [true, false] {
                draw(&mut workbench, "", pointer(node, pressed));
            }
            assert_eq!(workbench.node, 1);
            assert!(workbench.tab == tab);
            let file = draw(&mut workbench, "other.pac", vec![]).unwrap();
            for pressed in [true, false] {
                draw(&mut workbench, "", pointer(file, pressed));
            }
            assert_eq!(workbench.path, Some(second));
            assert!(workbench.loading);
            assert!(workbench.tab == tab);

            let mut lighting = (*document).clone();
            lighting.nodes[1].kind = Kind::Unknown;
            lighting.nodes[5].kind = Kind::Unknown;
            lighting.nodes[11].kind = Kind::StageLighting;
            let lighting = Arc::new(lighting);
            workbench.loaded_document(lighting.clone());
            assert!(workbench.tab == tab);
            workbench.refresh_document(lighting);
            assert!(workbench.tab == tab);
            assert!(workbench.control.commands().is_empty());
        }
    }

    #[test]
    fn a_file_without_a_model_does_not_replay_or_clear_the_current_preview() {
        let mut workbench = preview_fixture();
        let mut document = (*multiple_models()).clone();
        document.nodes[1].kind = Kind::Unknown;
        document.nodes[5].kind = Kind::Unknown;
        workbench.loaded_document(Arc::new(document));
        assert_eq!(workbench.resource_counts[0], 5);
        assert!(workbench.control.commands().is_empty());
        assert!(workbench.error.is_empty());
    }

    fn draw_tree(
        workbench: &mut Workbench,
        context: &egui::Context,
        index: usize,
        time: f64,
        events: Vec<egui::Event>,
    ) -> (
        egui::Response,
        Option<egui::Response>,
        egui::Id,
        Option<usize>,
    ) {
        let mut responses = None;
        let mut preview = None;
        let mut details = None;
        let mut id = egui::Id::NULL;
        context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300.0, 400.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    id = ui.make_persistent_id((
                        "resource-node",
                        visible_node(
                            workbench.document.as_ref().unwrap(),
                            index,
                            workbench.view.show_encoding_layers,
                        ),
                    ));
                    responses = tree(
                        ui,
                        &ResourceRef::new(workbench.document.as_ref().unwrap().clone(), index),
                        &workbench.resource_counts,
                        workbench.view.show_encoding_layers,
                        &mut workbench.node,
                        &mut workbench.selection,
                        &mut preview,
                        &mut details,
                    );
                },
            )
            .drop_without_applying_deltas();
        if let Some(source) = preview {
            workbench.load_source(source);
        }
        let (label, button) = responses.unwrap();
        (label, button, id, details)
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn expanded_resource_sibling_keeps_other_rows_clickable() {
        let mut workbench = preview_fixture();
        let mut document = (*multiple_models()).clone();
        for (name, children) in [
            ("directory-0", vec![1, 2]),
            ("directory-1", vec![5, 6]),
            ("directory-2", vec![9]),
        ] {
            let mut branch = document.nodes[0].clone();
            branch.name = name.into();
            branch.kind = Kind::Block;
            branch.children = children;
            document.nodes.push(branch);
        }
        document.nodes[0].children = vec![12, 13, 14];
        let document = Arc::new(document);
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        context.global_style_mut(|style| style.animation_time = 0.0);
        let draw = |workbench: &mut Workbench, time, events| {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 500.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    tree(
                        ui,
                        &ResourceRef::new(document.clone(), document.root),
                        &workbench.resource_counts,
                        false,
                        &mut workbench.node,
                        &mut workbench.selection,
                        &mut None,
                        &mut None,
                    );
                },
            );
            let positions = ["directory-0", "directory-1", "directory-2"].map(|name| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text().starts_with(name) => {
                            Some(text.pos + text.galley.rect.center().to_vec2())
                        }
                        _ => None,
                    })
                    .unwrap()
            });
            output.drop_without_applying_deltas();
            positions
        };
        draw(&mut workbench, 0.0, vec![]);
        let positions = draw(&mut workbench, 0.1, vec![]);
        draw(&mut workbench, 0.2, pointer(positions[1], true));
        draw(&mut workbench, 0.3, pointer(positions[1], false));
        assert_eq!(workbench.node, 13);
        let positions = draw(&mut workbench, 0.4, vec![]);
        draw(&mut workbench, 0.5, pointer(positions[0], true));
        draw(&mut workbench, 0.6, pointer(positions[0], false));
        assert_eq!(workbench.node, 12);
        let expanded = draw(&mut workbench, 0.7, vec![]);
        assert!(expanded[1].y > positions[1].y);
        assert!((expanded[2].y - expanded[1].y - (positions[2].y - positions[1].y)).abs() < 1.0);
        draw(&mut workbench, 0.8, pointer(expanded[2], true));
        draw(&mut workbench, 0.9, pointer(expanded[2], false));
        assert_eq!(workbench.node, 14);
    }

    #[test]
    fn resource_labels_fit_the_current_font_and_count_without_elision_or_overlap() {
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        for (size, count) in [(14.0, 1), (20.0, 798), (24.0, 123_456)] {
            let mut rectangles = None;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(420.0, 80.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.style_mut().override_font_id = Some(egui::FontId::proportional(size));
                    ui.spacing_mut().interact_size.y = size + 6.0;
                    let (name, button) = ui
                        .horizontal(|ui| {
                            tree_row(ui, &"long-resource-name".repeat(8), false, count, None)
                        })
                        .inner;
                    rectangles = Some((name.rect, button.unwrap().rect));
                },
            );
            let (name, button) = rectangles.unwrap();
            let label = format!("资源 {count}");
            let (clip, text) = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some((shape.clip_rect, text))
                    }
                    _ => None,
                })
                .unwrap();
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            assert!(!text.galley.elided);
            assert!(clip.contains_rect(rect));
            assert!(name.right() <= rect.left() && rect.right() <= button.left());
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn right_aligned_resource_actions_preserve_the_name_and_arrow_positions() {
        let mut workbench = preview_fixture();
        let mut document = (*multiple_models()).clone();
        let mut branch = document.nodes[0].clone();
        branch.name = "named-member-with-a-long-resource-name-".repeat(12);
        branch.children = vec![5, 6, 7];
        document.nodes.push(branch);
        document.nodes[0]
            .children
            .retain(|index| ![5, 6, 7].contains(index));
        document.nodes[0].children.push(12);
        let document = Arc::new(document);
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        // A large file total must not reserve extra digits on this three-resource row.
        workbench.resource_counts[0] = 123_456;
        let (before, button, id, _) = draw_tree(&mut workbench, &context, 12, 0.0, vec![]);
        assert!(
            button.is_some(),
            "directory loading does not depend on the model bundle index"
        );
        let arrow = context.read_response(id).unwrap().rect;
        workbench.refresh_document(document);
        let (label, button, _, _) = draw_tree(&mut workbench, &context, 12, 0.01, vec![]);
        let button = button.unwrap();
        assert_eq!(label.rect, before.rect);
        assert_eq!(context.read_response(id).unwrap().rect, arrow);
        assert!(button.rect.left() > label.rect.right());
        assert!(
            button.rect.left() >= 0.0 && button.rect.right() <= 300.0,
            "button must stay in the visible row: {:?}",
            button.rect
        );
        assert!(
            !egui::collapsing_header::CollapsingState::load(&context, id)
                .unwrap()
                .is_open()
        );
        draw_tree(
            &mut workbench,
            &context,
            12,
            0.1,
            pointer(button.rect.center(), true),
        );
        draw_tree(
            &mut workbench,
            &context,
            12,
            0.2,
            pointer(button.rect.center(), false),
        );
        assert_eq!(workbench.node, 12);
        assert!(workbench.tab == InspectorTab::Loaded);
        assert!(
            !egui::collapsing_header::CollapsingState::load(&context, id)
                .unwrap()
                .is_open()
        );
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadResource(source)] if source.node == 12)
        );
    }

    #[test]
    fn double_clicking_file_model_or_motion_labels_only_selects_or_expands() {
        for index in [0, 5, 9] {
            let mut workbench = preview_fixture();
            workbench.loaded_document(multiple_models());
            let context = egui::Context::default();
            let (label, _, _, _) = draw_tree(&mut workbench, &context, index, 0.0, vec![]);
            let pos = label.rect.center();
            let mut double_clicked = false;
            for (frame, pressed) in [true, false, true, false].into_iter().enumerate() {
                let (label, _, _, _) = draw_tree(
                    &mut workbench,
                    &context,
                    index,
                    (frame + 1) as f64 * 0.05,
                    pointer(pos, pressed),
                );
                double_clicked |= label.double_clicked();
            }
            assert!(
                double_clicked,
                "fixture must exercise an actual double click"
            );
            assert_eq!(workbench.node, index);
            assert!(workbench.control.commands().is_empty());
        }
    }

    #[test]
    fn encoded_layers_project_to_content_and_can_be_revealed_without_changing_sources() {
        let mut workbench = preview_fixture();
        let document = encoded_models();
        let buffers = document.buffers.clone();
        workbench.loaded_document(document.clone());
        assert!(!workbench.view.show_encoding_layers);
        assert_eq!(workbench.node, 0);
        let context = egui::Context::default();
        let (_, button, decoded_id, _) = draw_tree(&mut workbench, &context, 12, 0.0, vec![]);
        assert!(context.read_response(decoded_id).is_some());
        for layer in [12, 13, 14] {
            assert_eq!(visible_node(&document, layer, false), 0);
        }
        let button = button.unwrap();
        draw_tree(
            &mut workbench,
            &context,
            12,
            0.1,
            pointer(button.rect.center(), true),
        );
        draw_tree(
            &mut workbench,
            &context,
            12,
            0.2,
            pointer(button.rect.center(), false),
        );
        assert_eq!(workbench.node, 0);
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadResource(source)] if source.node == 0)
        );

        workbench.view.show_encoding_layers = true;
        for layer in [12, 13, 14, 0] {
            assert_eq!(visible_node(&document, layer, true), layer);
            let context = egui::Context::default();
            let (label, _, _, _) = draw_tree(&mut workbench, &context, layer, 0.0, vec![]);
            draw_tree(
                &mut workbench,
                &context,
                layer,
                0.1,
                pointer(label.rect.center(), true),
            );
            draw_tree(
                &mut workbench,
                &context,
                layer,
                0.2,
                pointer(label.rect.center(), false),
            );
            assert_eq!(workbench.node, layer);
            assert_eq!(
                document.bytes(layer).unwrap(),
                &*buffers[document.nodes[layer].buffer]
            );
        }
        assert!(
            document
                .buffers
                .iter()
                .zip(&buffers)
                .all(|(current, original)| Arc::ptr_eq(current, original))
        );
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn layer_projection_keeps_errors_and_stage_references_visible() {
        let mut document = (*encoded_models()).clone();
        document.nodes[13].error = Some("decoding failed".into());
        assert_eq!(visible_node(&document, 12, false), 13);
        assert!(
            document.nodes[visible_node(&document, 12, false)]
                .error
                .is_some()
        );
        let mut workbench = preview_fixture();
        workbench.loaded_document(Arc::new(document.clone()));
        assert_eq!(workbench.node, 13);
        let context = egui::Context::default();
        let (label, _, _, _) = draw_tree(&mut workbench, &context, 12, 0.0, vec![]);
        draw_tree(
            &mut workbench,
            &context,
            12,
            0.1,
            pointer(label.rect.center(), true),
        );
        draw_tree(
            &mut workbench,
            &context,
            12,
            0.2,
            pointer(label.rect.center(), false),
        );
        assert_eq!(workbench.node, 13);
        document.nodes[12].kind = Kind::StageResourceReference;
        assert_eq!(visible_node(&document, 12, false), 12);
    }

    #[test]
    fn loading_resources_dispatches_immediately_without_a_pending_combination() {
        let mut workbench = preview_fixture();
        let document = multiple_models();
        workbench.loaded_document(document.clone());
        for node in [5, 6, 7, 7, 9] {
            workbench.tab = InspectorTab::Resource;
            workbench.load_node(node);
            assert!(workbench.tab == InspectorTab::Loaded);
        }
        let commands = workbench.control.commands();
        assert_eq!(commands.len(), 5);
        for (command, node) in commands.iter().zip([5, 6, 7, 7, 9]) {
            assert!(
                matches!(command, Command::LoadResource(source) if source.node == node && Arc::ptr_eq(&source.document, &document))
            );
        }
        workbench.loaded_document(multiple_models());
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn resource_removal_targets_its_loaded_source_after_browsing_another_file() {
        let mut workbench = preview_fixture();
        let first = multiple_models();
        let second = multiple_models();
        let mut resources = AssetBundle::find_with_nodes(first.clone()).0.remove(0);
        resources.textures = vec![
            ResourceRef::new(first.clone(), 4),
            ResourceRef::new(second.clone(), 8),
        ];
        let model = LoadedModel {
            id: 41,
            name: "model".into(),
            resources,
            visible: true,
            error: None,
            meshes: Arc::default(),
        };
        let snapshot = Snapshot {
            resources: Arc::new(
                model
                    .resources
                    .textures
                    .iter()
                    .enumerate()
                    .map(|(index, source)| crate::preview::LoadedResource {
                        id: 91 + index as u64,
                        source: source.clone(),
                        enabled: true,
                    })
                    .collect(),
            ),
            models: Arc::new(vec![model.clone()]),
            ..Default::default()
        };
        workbench.loaded_document(second);
        let context = egui::Context::default();
        let draw = |workbench: &mut Workbench, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| workbench.resource_list(ui, &snapshot, Kind::Txb),
            )
        };
        let output = draw(&mut workbench, vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "卸载" => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap();
        output.drop_without_applying_deltas();
        draw(&mut workbench, pointer(point, true)).drop_without_applying_deltas();
        draw(&mut workbench, pointer(point, false)).drop_without_applying_deltas();
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::RemoveResource(91)]
        ));
        assert!(Arc::ptr_eq(&model.resources.textures[0].document, &first));
    }

    #[test]
    fn motion_rows_play_explicitly_while_tracks_and_channels_remain_inspection_only() {
        let mut workbench = preview_fixture();
        workbench.tab = InspectorTab::Resource;
        let mut document = (*multiple_models()).clone();
        document.nodes[9].deferred = true;
        workbench.loaded_document(Arc::new(document));
        let context = egui::Context::default();
        let (label, button, _, details) = draw_tree(&mut workbench, &context, 9, 0.0, vec![]);
        assert!(button.is_some() && details.is_none());
        draw_tree(
            &mut workbench,
            &context,
            9,
            0.1,
            pointer(label.rect.center(), true),
        );
        draw_tree(
            &mut workbench,
            &context,
            9,
            0.2,
            pointer(label.rect.center(), false),
        );
        let (_, button, id, details) = draw_tree(&mut workbench, &context, 9, 1.0, vec![]);
        assert_eq!(details, Some(9));
        assert_eq!(workbench.node, 9);
        assert!(workbench.tab == InspectorTab::Resource);
        assert!(workbench.control.commands().is_empty());
        let position = button.unwrap().rect.center();
        for (time, pressed) in [(1.1, true), (1.2, false)] {
            draw_tree(
                &mut workbench,
                &context,
                9,
                time,
                pointer(position, pressed),
            );
        }
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadResource(source)] if source.node == 9)
        );
        assert!(workbench.tab == InspectorTab::Loaded);
        assert!(
            egui::collapsing_header::CollapsingState::load(&context, id)
                .unwrap()
                .is_open()
        );
        for kind in [Kind::Track, Kind::Channel] {
            let mut document = (**workbench.document.as_ref().unwrap()).clone();
            document.nodes[9].kind = kind;
            document.nodes[9].deferred = false;
            workbench.refresh_document(Arc::new(document));
            let (_, button, _, _) = draw_tree(&mut workbench, &context, 9, 2.0, vec![]);
            assert!(button.is_none());
        }
    }

    #[test]
    fn resource_action_button_loads_only_the_selected_scope() {
        let mut workbench = preview_fixture();
        let document = multiple_models();
        workbench.loaded_document(document.clone());
        workbench.node = 0;
        let control = workbench.control.clone();
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let mut draw = |events| {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(300.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| workbench.resource_actions(ui, &document),
                )
                .drop_without_applying_deltas();
        };
        let key = |key| {
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect()
        };
        draw(vec![]);
        draw(vec![]);
        // The details action uses the same selected scope as the tree button.
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(
            matches!(control.commands().as_slice(), [Command::LoadResource(source)] if source.node == 0)
        );
    }

    #[test]
    fn mesh_controls_send_only_scoped_visibility_commands_without_focusing() {
        let mut workbench = preview_fixture();
        let control = workbench.control.clone();
        let model = LoadedModel {
            id: 41,
            resources: AssetBundle::find_with_nodes(multiple_models()).0.remove(0),
            name: "model".into(),
            visible: false,
            error: None,
            meshes: Arc::new(vec![
                crate::preview::LoadedMesh {
                    index: 7,
                    vertices: 207,
                    visible: true,
                },
                crate::preview::LoadedMesh {
                    index: 23,
                    vertices: 204,
                    visible: true,
                },
            ]),
        };
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let mut draw = |events| {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(280.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        egui_hunter::Density::Compact
                            .scope(ui, |ui| workbench.mesh_controls(ui, &model));
                    },
                )
                .drop_without_applying_deltas();
        };
        let key = |key| {
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect()
        };
        draw(Vec::new());
        draw(Vec::new());
        assert!(control.commands().is_empty());
        // Keyboard activation exercises the real buttons and checkbox without
        // depending on translated labels or fixed pixel coordinates.
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(matches!(
            control.commands().as_slice(),
            [Command::ShowAllMeshes(41)]
        ));
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(matches!(
            control.commands().as_slice(),
            [Command::MeshVisible {
                model: 41,
                mesh: 7,
                visible: false
            }]
        ));
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(matches!(
            control.commands().as_slice(),
            [Command::IsolateMesh { model: 41, mesh: 7 }]
        ));
        assert!(!model.visible);
    }

    #[test]
    fn docked_layout_keeps_preview_space_and_scoped_density_while_resizing() {
        let mut workbench = preview_fixture();
        let mut document = (*multiple_models()).clone();
        document.nodes[0].name = "long-resource-file-name".repeat(12);
        document.nodes[0].fields.push(Field {
            writable: false,
            binding: crate::field::Binding {
                buffer: 0,
                range: 0..16,
                format: crate::field::FieldType::ReadOnly,
                endian: mhf_resource::binary::Endian::Little,
            },
            name: "unknown_00000010".repeat(4),
            value: "Long resource field content ".repeat(20),
        });
        workbench.refresh_document(Arc::new(document));
        workbench.tab = InspectorTab::Resource;
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let standard = context.global_style().spacing.clone();
        for size in [
            egui::vec2(1440.0, 900.0),
            egui::vec2(960.0, 640.0),
            egui::vec2(1920.0, 1080.0),
        ] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..6 {
                context
                    .run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ui| {
                            let spacing = ui.spacing().clone();
                            workbench.show(ui);
                            assert_eq!(ui.spacing().interact_size, spacing.interact_size);
                            assert_eq!(ui.spacing().button_padding, spacing.button_padding);
                        },
                    )
                    .drop_without_applying_deltas();
            }
            let viewport = workbench.viewport_rect;
            assert!(screen.contains_rect(viewport), "{screen:?}: {viewport:?}");
            assert!(
                viewport.width() >= 295.0 && viewport.height() >= 200.0,
                "{screen:?}: {viewport:?}"
            );
            let requested = workbench.control.viewport();
            assert!((requested.width * size.x - viewport.width()).abs() < 0.01);
            assert!((requested.height * size.y - viewport.height()).abs() < 0.01);
            assert_eq!(
                context.global_style().spacing.interact_size,
                standard.interact_size
            );
        }
        // Merely painting layout must not fill the native command queue.
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn focus_mode_expands_preview_and_hidden_workbench_restores_full_native_viewport() {
        let mut workbench = preview_fixture();
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            ..Default::default()
        };
        context
            .run_ui(input(), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        let docked = workbench.viewport_rect;
        workbench.view.preview_only = true;
        context
            .run_ui(input(), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        assert!(workbench.viewport_rect.width() > docked.width() + 400.0);
        assert!(workbench.viewport_rect.height() > docked.height());
        workbench.open = false;
        context
            .run_ui(input(), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        let viewport = workbench.control.viewport();
        assert_eq!(
            (viewport.x, viewport.y, viewport.width, viewport.height),
            (0.0, 0.0, 1.0, 1.0)
        );
    }

    #[test]
    fn viewport_drag_gestures_choose_pan_or_orbit_without_repeating_stationary_motion() {
        for (button, modifiers, pan) in [
            (egui::PointerButton::Middle, egui::Modifiers::NONE, true),
            (egui::PointerButton::Primary, egui::Modifiers::SHIFT, true),
            (egui::PointerButton::Primary, egui::Modifiers::NONE, false),
        ] {
            let mut workbench = preview_fixture();
            let context = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
            let snapshot = Snapshot {
                camera: Some(crate::preview::Camera {
                    eye: [0.0, 0.0, 350.0],
                    target: [0.0; 3],
                    up: [0.0, 1.0, 0.0],
                    fov_y: std::f32::consts::FRAC_PI_3,
                    aspect: 800.0 / 600.0,
                }),
                ..Snapshot::default()
            };
            let control = workbench.control.clone();
            let mut draw = |events| {
                context
                    .run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |ui| workbench.viewport(ui, &snapshot, screen),
                    )
                    .drop_without_applying_deltas();
            };
            let start = screen.center();
            let end = start + egui::vec2(40.0, 20.0);
            draw(vec![egui::Event::ModifiersChanged(modifiers)]);
            draw(vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button,
                    pressed: true,
                    modifiers,
                },
            ]);
            draw(vec![egui::Event::PointerMoved(end)]);
            let commands = control.commands();
            if pan {
                assert!(
                    matches!(commands.as_slice(), [Command::Pan([x, y, z])] if *x < 0.0 && *y > 0.0 && *z == 0.0)
                );
            } else {
                assert!(
                    matches!(commands.as_slice(), [Command::Camera { yaw, pitch, .. }] if *yaw < 0.0 && *pitch > snapshot.pitch)
                );
            }
            draw(vec![]);
            draw(vec![egui::Event::PointerButton {
                pos: end,
                button,
                pressed: false,
                modifiers,
            }]);
            assert!(control.commands().is_empty());
        }
    }

    #[test]
    fn fullscreen_shortcut_works_in_search_and_when_hidden_without_repeating() {
        let mut workbench = preview_fixture();
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let input = |pressed, repeat| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat,
                modifiers: egui::Modifiers::ALT,
            }],
            ..Default::default()
        };
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("workbench-filter")));
        context
            .run_ui(input(true, false), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::ToggleFullscreen]
        ));
        assert!(!workbench.view.preview_only);
        assert!(workbench.filter.is_empty());
        context
            .run_ui(input(true, true), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        assert!(workbench.control.commands().is_empty());
        workbench.open = false;
        context
            .run_ui(input(false, false), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        context
            .run_ui(input(true, false), |ui| workbench.show(ui))
            .drop_without_applying_deltas();
        assert!(matches!(
            workbench.control.commands().as_slice(),
            [Command::ToggleFullscreen]
        ));
        assert!(!workbench.open);
    }

    #[test]
    fn output_log_records_changes_once_and_keeps_recent_entries() {
        let mut workbench = preview_fixture();
        let mut snapshot = Snapshot::default();
        for _ in 0..20 {
            workbench.record_messages(&snapshot);
        }
        assert_eq!(workbench.log.len(), 1);
        for index in 0..220 {
            snapshot.message = format!("model {index}").into();
            workbench.record_messages(&snapshot);
        }
        assert_eq!(workbench.log.len(), 200);
        assert_eq!(workbench.log.front().unwrap().1, "model 20");
        assert_eq!(workbench.log.back().unwrap().1, "model 219");
    }

    #[test]
    fn disabled_or_failed_skeletons_do_not_drive_focus_or_coordinate_overlays() {
        let mut workbench = preview_fixture();
        workbench.bone = Some((91, 0));
        workbench.view.show_axes = true;
        workbench.view.show_grid = false;
        let mut snapshot = Snapshot {
            ready: true,
            camera: Some(crate::preview::Camera {
                eye: [0.0, 0.0, 10.0],
                target: [0.0; 3],
                up: [0.0, 1.0, 0.0],
                fov_y: std::f32::consts::FRAC_PI_3,
                aspect: 1.0,
            }),
            resources: Arc::new(vec![crate::preview::LoadedResource {
                id: 91,
                source: ResourceRef::new(multiple_models(), 2),
                enabled: true,
            }]),
            skeletons: Arc::new(vec![crate::preview::LoadedSkeleton {
                id: 91,
                bones: Arc::new(vec![crate::preview::Bone {
                    index: 0,
                    parent: None,
                    position: [1.0, 2.0, 3.0],
                }]),
                bone_bindings: Arc::new(vec![None]),
                error: None,
            }]),
            ..Default::default()
        };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0));
        let context = egui::Context::default();
        for (enabled, failed) in [(false, false), (true, true), (true, false)] {
            Arc::make_mut(&mut snapshot.resources)[0].enabled = enabled;
            Arc::make_mut(&mut snapshot.skeletons)[0].error =
                failed.then(|| "invalid skeleton".into());
            let visible = enabled && !failed;
            assert_eq!(has_visible_resources(&snapshot), visible);
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(screen.center()),
                        egui::Event::Key {
                            key: egui::Key::F,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| workbench.viewport(ui, &snapshot, screen),
            );
            let selected_coordinates = output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().starts_with("骨骼 0"))
            });
            assert_eq!(selected_coordinates, visible);
            assert_eq!(workbench.bone, Some((91, 0)));
            let commands = workbench.control.commands();
            if visible {
                assert!(matches!(commands.as_slice(), [Command::FocusAll]));
            } else {
                assert!(commands.is_empty());
            }
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn viewport_shortcuts_preserve_f_and_space_in_resource_search() {
        let mut workbench = preview_fixture();
        workbench.control.publish(Snapshot {
            motions: Arc::new(vec![crate::preview::LoadedMotion {
                id: 7,
                source: ResourceRef::new(multiple_models(), 9),
                enabled: true,
                frames: 60.0,
                frame: Some(15.0),
                skeleton: None,
            }]),
            ..Snapshot::default()
        });
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| workbench.show(ui),
            )
            .drop_without_applying_deltas();
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("workbench-filter")));
        let mut events = vec![egui::Event::PointerMoved(workbench.viewport_rect.center())];
        for (key, text) in [(egui::Key::F, "f"), (egui::Key::Space, " ")] {
            events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            events.push(egui::Event::Text(text.into()));
        }
        context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| workbench.show(ui),
            )
            .drop_without_applying_deltas();
        assert_eq!(workbench.filter, "f ");
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn resource_window_width_stays_stable_across_frames() {
        let root = std::env::temp_dir().join("mhf-workbench-layout-fixture");
        let mut worker = Worker::start(root.clone(), root.clone()).unwrap();
        worker.stop();
        let worker = Arc::new(worker);
        let _ = worker.updates();
        let mut workbench = Workbench::new(
            Arc::new(Control::default()),
            worker,
            root,
            ViewSettings::default(),
            None,
        );
        workbench.scanning = false;
        workbench.refresh_document(Arc::new(Document {
            root: 0, buffers: vec![Arc::from([0_u8; 16])],
            nodes: vec![Node {
                name: "Z:\\game\\dat\\model\\long-resource-file-name.bin".into(),
                kind: Kind::Unknown, buffer: 0, range: 0..16, children: vec![], action: None, deferred: false, error: None,
                metadata: Default::default(),
                fields: vec![Field {writable: false, binding: crate::field::Binding { buffer: 0, range: 0..16, format: crate::field::FieldType::ReadOnly, endian: mhf_resource::binary::Endian::Little }, name: "unknown_00000010".into(), value: "A long resource value with enough words to wrap within the inspector column".repeat(3)}],
            }],
        }));
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        mhf_font::install(&context);
        let mut widths = Vec::new();
        let mut directories = Vec::new();
        for frame in 0..30 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            context
                .run_ui(input, |ui| {
                    let window = egui::Window::new("资源浏览 · F8")
                        .id(egui::Id::new("width-test"))
                        .default_width(380.0)
                        .default_height(700.0)
                        .max_width(1440.0)
                        .show(ui.ctx(), |ui| workbench.resources(ui))
                        .unwrap();
                    widths.push(window.response.rect.width());
                    directories.push(window.inner.unwrap());
                })
                .drop_without_applying_deltas();
        }
        assert!(
            directories[5..].iter().all(|rect| rect.height() > 500.0),
            "directory rectangles: {directories:?}"
        );
        assert!(
            widths[5..].iter().all(|&width| width <= 430.0),
            "resource widths: {widths:?}"
        );
        assert!(
            (widths[29] - widths[5]).abs() <= 0.5,
            "resource widths: {widths:?}"
        );
    }
}
