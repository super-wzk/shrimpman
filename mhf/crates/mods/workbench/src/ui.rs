use crate::{
    catalog::Catalog,
    inspect::{Document, Kind, Node},
    preview::{AssetBundle, Command, Control, LoadedModel, ResourceRef, Snapshot, Viewport},
    worker::Worker,
};
use egui::{Color32, RichText};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Models,
    Resource,
    Bones,
}

pub(crate) struct Workbench {
    control: Arc<Control>,
    worker: Arc<Worker>,
    root: PathBuf,
    open: bool,
    compact: bool,
    show_resources: bool,
    show_encoding_layers: bool,
    show_inspector: bool,
    show_log: bool,
    preview_only: bool,
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
    assets: Vec<AssetBundle>,
    asset_nodes: Vec<Vec<usize>>,
    model: Option<ResourceRef>,
    skeleton: Option<ResourceRef>,
    textures: Vec<ResourceRef>,
    node: usize,
    hex_start: usize,
    hex_buffer: bool,
    hex_selection: Option<std::ops::Range<usize>>,
    active_model: Option<u64>,
    bone: Option<usize>,
    show_bones: bool,
    error: String,
    status: String,
}

impl Workbench {
    pub fn new(control: Arc<Control>, worker: Arc<Worker>, root: PathBuf) -> Self {
        Self {
            control,
            worker,
            root,
            open: true,
            compact: true,
            show_resources: true,
            show_encoding_layers: false,
            show_inspector: true,
            show_log: true,
            preview_only: false,
            tab: InspectorTab::Models,
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
            hex_start: 0,
            assets: Vec::new(),
            asset_nodes: Vec::new(),
            model: None,
            skeleton: None,
            textures: Vec::new(),
            hex_buffer: false,
            hex_selection: None,
            active_model: None,
            bone: None,
            show_bones: false,
            error: String::new(),
            status: String::new(),
        }
    }

    fn send(&mut self, command: Command) {
        let bundle = match &command {
            Command::LoadAssets(bundles) => bundles.first(),
            Command::AddAsset(bundle) => Some(bundle),
            _ => None,
        };
        if let Some(bundle) = bundle {
            self.model = Some(bundle.model.clone());
            self.skeleton = bundle.skeleton.clone();
            self.textures.clone_from(&bundle.textures);
        }
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
                Ok(document) => self.loaded_document(document),
                Err(error) => {
                    self.document = None;
                    self.assets.clear();
                    self.asset_nodes.clear();
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
    }

    fn loaded_document(&mut self, document: Arc<Document>) {
        self.node = visible_node(&document, document.root, self.show_encoding_layers);
        self.refresh_document(document);
    }

    fn refresh_document(&mut self, document: Arc<Document>) {
        (self.assets, self.asset_nodes) = AssetBundle::find_with_nodes(document.clone());
        self.status.clear();
        if self.assets.is_empty()
            && document
                .nodes
                .iter()
                .any(|node| node.kind == Kind::StageLighting)
        {
            self.status =
                "此包包含高清场景光照与环境贴图；几何模型请打开 stage 中对应编号的主场景资源。"
                    .into();
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
        let context = ui.ctx().clone();
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F8)) {
            self.open = !self.open;
        }
        if !self.open {
            self.control.set_viewport(Viewport::default());
            return;
        }
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            self.preview_only = !self.preview_only;
        }
        let snapshot = self.control.snapshot();
        if self.active_model != snapshot.active_model {
            self.active_model = snapshot.active_model;
            self.bone = None;
        }
        self.record_messages(&snapshot);
        ui.scope(|ui| {
            if self.compact {
                egui_hunter::Density::Compact.scope(ui, |ui| self.layout(ui, &snapshot));
            } else {
                self.layout(ui, &snapshot);
            }
        });
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
                        ui.checkbox(&mut self.compact, "紧凑模式");
                        ui.separator();
                        ui.checkbox(&mut self.show_resources, "资源目录");
                        if ui
                            .checkbox(&mut self.show_encoding_layers, "显示编码层")
                            .changed()
                            && !self.show_encoding_layers
                            && let Some(document) = &self.document
                        {
                            self.node = visible_node(document, self.node, false);
                            self.hex_start = 0;
                            self.hex_buffer = false;
                            self.hex_selection = None;
                        }
                        ui.checkbox(&mut self.show_inspector, "检查器");
                        ui.checkbox(&mut self.show_log, "输出日志");
                        ui.checkbox(&mut self.preview_only, "专注预览 · F11");
                    });
                    if ui
                        .button(if self.preview_only {
                            "恢复布局 · F11"
                        } else {
                            "专注预览 · F11"
                        })
                        .clicked()
                    {
                        self.preview_only = !self.preview_only;
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
        if self.show_resources && !self.preview_only {
            egui::Panel::left("workbench-resources")
                .default_size(300.0)
                .size_range(200.0_f32.min(side_limit)..=side_limit)
                .frame(frame)
                .show(ui, |ui| {
                    self.resources(ui);
                });
        }
        if self.show_inspector && !self.preview_only {
            egui::Panel::right("workbench-inspector")
                .default_size(320.0)
                .size_range(220.0_f32.min(side_limit)..=side_limit)
                .frame(frame)
                .show(ui, |ui| self.inspector_panel(ui, snapshot));
        }
        if self.show_log && !self.preview_only {
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
                    ui.strong("模型预览");
                    if ui
                        .add_enabled(
                            !snapshot.models.is_empty(),
                            egui_hunter::Button::new("聚焦全部 · F"),
                        )
                        .clicked()
                    {
                        self.send(Command::FocusAll);
                    }
                    ui.checkbox(&mut self.show_bones, "骨架");
                    ui.weak("拖动环绕 · 滚轮缩放");
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport(ui, snapshot, screen));
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (InspectorTab::Models, "模型"),
                (InspectorTab::Resource, "资源"),
                (InspectorTab::Bones, "骨骼"),
            ] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("workbench-inspector-content")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                InspectorTab::Models => self.composition(ui, snapshot),
                InspectorTab::Resource => {
                    if let Some(document) = self.document.clone() {
                        egui::CollapsingHeader::new("预览与资源选择")
                            .show(ui, |ui| self.resource_actions(ui, &document));
                        if let Some(node) = document.nodes.get(self.node) {
                            self.inspector(ui, &document, node);
                        }
                    } else {
                        ui.weak("单击资源查看字段与原始字节，点击模型组旁的预览按钮加载。");
                    }
                }
                InspectorTab::Bones => {
                    if snapshot.bones.is_empty() {
                        ui.weak("当前模型没有可显示的骨骼。");
                    } else {
                        self.bones(ui, snapshot);
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
        if snapshot.models.is_empty() && snapshot.scene.is_none() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "选择左侧资源，点击模型组旁的预览按钮",
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
        self.draw_bones(ui, snapshot);
        let mut distance = snapshot.distance;
        let mut pitch = snapshot.pitch;
        let mut yaw = snapshot.yaw;
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            yaw = (yaw - delta.x * 0.4 + 180.0).rem_euclid(360.0) - 180.0;
            pitch = (pitch + delta.y * 0.4).clamp(-80.0, 80.0);
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
                && !snapshot.models.is_empty()
            {
                self.send(Command::FocusAll);
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Space))
                && snapshot.motion.is_some()
            {
                self.send(Command::Playing(!snapshot.playing));
            }
        }
    }

    fn timeline(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.horizontal(|ui| {
            ui.strong("动画");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(snapshot.motion.is_some(), egui::Button::new("卸载"))
                    .clicked()
                {
                    self.send(Command::UnloadMotion);
                }
                ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::Label::new(
                        snapshot
                            .motion
                            .as_deref()
                            .unwrap_or("点击动画节点右侧的播放按钮"),
                    )
                    .truncate(),
                )
                .on_hover_text(snapshot.motion.as_deref().unwrap_or(""));
            });
        });
        ui.add_enabled_ui(snapshot.motion.is_some(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if snapshot.playing && snapshot.motion.is_some() {
                        "暂停"
                    } else {
                        "播放"
                    })
                    .clicked()
                {
                    self.send(Command::Playing(!snapshot.playing));
                }
                if ui.button("-1").on_hover_text("上一帧").clicked() {
                    self.send(Command::Seek((snapshot.frame - 1.0).max(0.0)));
                }
                if ui.button("+1").on_hover_text("下一帧").clicked() {
                    self.send(Command::Seek((snapshot.frame + 1.0).min(snapshot.frames)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("{:.0} / {:.0} 帧", snapshot.frame, snapshot.frames));
                    let mut frame = snapshot.frame;
                    ui.spacing_mut().slider_width = ui.available_width().max(1.0);
                    if ui
                        .add(
                            egui::Slider::new(&mut frame, 0.0..=snapshot.frames.max(0.0))
                                .show_value(false),
                        )
                        .changed()
                    {
                        self.send(Command::Seek(frame));
                    }
                });
            });
        });
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
        let mut preview_node = None;
        let mut details = None;
        let action_width = group_action_width(ui, self.assets.len());
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
                    &self.asset_nodes,
                    action_width,
                    self.show_encoding_layers,
                    &mut self.node,
                    &mut load,
                    &mut preview_node,
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
        if let Some(index) = preview_node {
            self.preview_node(index);
        }
        if let Some(path) = load {
            self.request = self.request.wrapping_add(1);
            self.path = Some(path.clone());
            self.document = None;
            self.loading = true;
            self.expanding = None;
            self.error.clear();
            self.assets.clear();
            self.asset_nodes.clear();
            self.worker.load(self.request, path);
        }
        browser.inner_rect
    }

    fn expand_node(&mut self, index: usize) {
        if self.expanding.is_none()
            && let Some(document) = &self.document
            && document.nodes[index].deferred
        {
            self.expanding = Some(index);
            self.worker.expand(self.request, document.clone(), index);
        }
    }

    fn resource_actions(&mut self, ui: &mut egui::Ui, document: &Arc<Document>) {
        let source = ResourceRef {
            document: document.clone(),
            node: self.node,
        };
        let kind = source.kind();
        let valid = document.nodes[self.node].error.is_none();
        if document.nodes[self.node].deferred && ui.button("展开明细").clicked() {
            self.expand_node(self.node);
        }
        ui.add_enabled_ui(valid, |ui| {
            let action = match kind {
                Kind::Fmod => Some("选择此模型"),
                Kind::Fskl => Some("选择此骨架"),
                Kind::Txb => Some("选择此贴图组"),
                Kind::Png | Kind::Dds => Some("选择此贴图"),
                Kind::Motion => Some("加载此动画"),
                _ => None,
            };
            if let Some(action) = action
                && ui.button(action).clicked()
            {
                self.select_resource(source.clone());
            }
            if matches!(kind, Kind::Txb | Kind::Png | Kind::Dds)
                && ui
                    .button("追加贴图")
                    .on_hover_text("按顺序追加到已选贴图来源之后")
                    .clicked()
            {
                self.textures.push(source);
            }
            let mut asset_indices = self.asset_nodes.get(self.node).cloned().unwrap_or_default();
            asset_indices.retain(|&index| self.assets.get(index).is_some());
            if !asset_indices.is_empty() {
                ui.horizontal(|ui| {
                    ui.strong(format!("模型组 {}", asset_indices.len()));
                    if ui
                        .small_button("预览")
                        .on_hover_text("预览此节点下的全部模型组")
                        .clicked()
                    {
                        self.preview_node(self.node);
                    }
                });
            }
            for (index, asset_index) in asset_indices.into_iter().enumerate() {
                let asset = &self.assets[asset_index];
                let commands = ui
                    .push_id(("resource-asset", index), |ui| {
                        let name = asset.name.rsplit(['/', '\\']).next().unwrap_or(&asset.name);
                        ui.add(egui::Label::new(name).truncate())
                            .on_hover_text(&asset.name);
                        ui.horizontal_wrapped(|ui| {
                            [
                                ui.button("加入预览")
                                    .clicked()
                                    .then(|| Command::AddAsset(asset.clone())),
                                ui.button("作为场景载入")
                                    .clicked()
                                    .then(|| Command::LoadScene(asset.clone())),
                            ]
                        })
                        .inner
                    })
                    .inner;
                for command in commands.into_iter().flatten() {
                    self.send(command);
                }
            }
        });
    }

    fn select_resource(&mut self, source: ResourceRef) {
        match source.kind() {
            Kind::Fmod => self.model = Some(source),
            Kind::Fskl => self.skeleton = Some(source),
            Kind::Txb | Kind::Png | Kind::Dds => self.textures = vec![source],
            Kind::Motion => self.send(Command::LoadMotion(source)),
            _ => {}
        }
    }

    fn scoped_assets(&self, node: usize) -> Vec<AssetBundle> {
        self.asset_nodes
            .get(node)
            .into_iter()
            .flatten()
            .filter_map(|&index| self.assets.get(index).cloned())
            .collect()
    }

    fn preview_node(&mut self, node: usize) {
        if let Some(document) = &self.document {
            let source = ResourceRef {
                document: document.clone(),
                node,
            };
            if source.kind() == Kind::Motion {
                self.node = node;
                self.send(Command::LoadMotion(source));
                return;
            }
        }
        let assets = self.scoped_assets(node);
        if !assets.is_empty() {
            self.node = node;
            self.tab = InspectorTab::Models;
            self.send(Command::LoadAssets(assets));
        }
    }

    fn selected_bundle(&self) -> Option<AssetBundle> {
        if self.textures.is_empty() {
            return None;
        }
        let model = self.model.clone()?;
        Some(AssetBundle {
            name: model.name(),
            model,
            skeleton: self.skeleton.clone(),
            textures: self.textures.clone(),
        })
    }

    fn inspector(&mut self, ui: &mut egui::Ui, document: &Document, node: &Node) {
        ui.horizontal_wrapped(|ui| {
            ui.strong(&node.name);
            ui.weak(node.kind.label());
            if ui.button("导出原始字节").clicked() {
                self.error = self
                    .worker
                    .export(document, self.node)
                    .err()
                    .unwrap_or_default();
            }
        });
        ui.monospace(format!(
            "0x{:08X} · {} 字节 · 数据层 {}",
            node.range.start,
            node.range.len(),
            node.buffer
        ));
        if let Some(error) = &node.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        let column_width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(40.0);
        egui::Grid::new(("resource-fields", self.node))
            .num_columns(2)
            .max_col_width(column_width)
            .striped(true)
            .show(ui, |ui| {
                for field in &node.fields {
                    if ui
                        .selectable_label(false, &field.name)
                        .on_hover_text(format!("0x{:08X} · {} 字节", field.offset, field.size))
                        .clicked()
                    {
                        self.hex_buffer = true;
                        self.hex_start = field.offset / 16 * 16;
                        self.hex_selection =
                            Some(field.offset..field.offset.saturating_add(field.size));
                    }
                    ui.add(egui::Label::new(&field.value).wrap());
                    ui.end_row();
                }
            });
        ui.collapsing("十六进制", |ui| {
            if ui
                .checkbox(&mut self.hex_buffer, "查看整个数据层（含目录字段）")
                .changed()
            {
                self.hex_start = 0;
            }
            let range = if self.hex_buffer {
                0..document.buffers[node.buffer].len()
            } else {
                node.range.clone()
            };
            if let Some(bytes) = document
                .buffers
                .get(node.buffer)
                .and_then(|bytes| bytes.get(range.clone()))
            {
                self.hex_start = self.hex_start.min(bytes.len().saturating_sub(1) / 16 * 16);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.hex_start > 0, egui::Button::new("上一页"))
                        .clicked()
                    {
                        self.hex_start = self.hex_start.saturating_sub(256);
                    }
                    if ui
                        .add_enabled(
                            self.hex_start + 256 < bytes.len(),
                            egui::Button::new("下一页"),
                        )
                        .clicked()
                    {
                        self.hex_start += 256;
                    }
                    ui.small(format!("+0x{:X}", self.hex_start));
                });
                egui::ScrollArea::both()
                    .id_salt("workbench-hex")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for (row, bytes) in bytes
                            [self.hex_start..bytes.len().min(self.hex_start + 256)]
                            .chunks(16)
                            .enumerate()
                        {
                            let hex = bytes
                                .iter()
                                .map(|byte| format!("{byte:02X}"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            let ascii: String = bytes
                                .iter()
                                .map(|&byte| {
                                    if byte.is_ascii_graphic() || byte == b' ' {
                                        byte as char
                                    } else {
                                        '.'
                                    }
                                })
                                .collect();
                            let offset = range.start + self.hex_start + 16 * row;
                            let mut text =
                                RichText::new(format!("{offset:08X}  {hex:47}  {ascii}"))
                                    .monospace();
                            if self.hex_selection.as_ref().is_some_and(|selected| {
                                selected.start < offset + bytes.len() && selected.end > offset
                            }) {
                                text = text.background_color(ui.visuals().selection.bg_fill);
                            }
                            ui.label(text);
                        }
                    });
            }
        });
    }

    fn composition(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        egui::CollapsingHeader::new("资源组合")
            .default_open(false)
            .show(ui, |ui| {
                for (label, source) in [("模型", &self.model), ("骨架", &self.skeleton)] {
                    ui.add(
                        egui::Label::new(format!(
                            "{label}：{}",
                            source
                                .as_ref()
                                .map_or_else(|| "未选择".into(), ResourceRef::name)
                        ))
                        .wrap(),
                    );
                }
                self.texture_sources(ui);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("清除骨架").clicked() {
                        self.skeleton = None;
                    }
                    if ui.button("清空组合").clicked() {
                        self.model = None;
                        self.skeleton = None;
                        self.textures.clear();
                    }
                });
                if let Some(bundle) = self.selected_bundle() {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("加入预览").clicked() {
                            self.send(Command::AddAsset(bundle.clone()));
                        }
                        if ui.button("作为场景载入").clicked() {
                            self.send(Command::LoadScene(bundle));
                        }
                    });
                }
            });
        ui.separator();
        ui.strong("预览模型");
        if snapshot.models.is_empty() {
            ui.weak("从资源浏览选择模型组，或在资源组合中加入预览。");
        } else {
            ui.horizontal_wrapped(|ui| {
                let visible = snapshot
                    .models
                    .iter()
                    .any(|model| model.visible && model.error.is_none());
                if ui
                    .add_enabled(visible, egui::Button::new("聚焦全部"))
                    .clicked()
                {
                    self.send(Command::FocusAll);
                }
                if ui.button("清空全部").clicked() {
                    self.send(Command::ClearAssets);
                }
            });
            for model in snapshot.models.iter() {
                ui.push_id(("preview-model", model.id), |ui| {
                    ui.horizontal(|ui| {
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
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("移除").clicked() {
                                self.send(Command::RemoveAsset(model.id));
                            }
                            let name = model
                                .name
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or(model.name.as_ref());
                            if ui
                                .add_sized(
                                    [ui.available_width(), ui.spacing().interact_size.y],
                                    egui::Button::selectable(
                                        snapshot.active_model == Some(model.id),
                                        name,
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(model.name.as_ref())
                                .clicked()
                            {
                                self.send(Command::SelectModel(model.id));
                            }
                        });
                    });
                    if let Some(error) = &model.error {
                        ui.colored_label(Color32::LIGHT_RED, format!("无法预览：{error}"));
                    }
                });
            }
        }
        let active = snapshot
            .active_model
            .and_then(|id| snapshot.models.iter().find(|model| model.id == id));
        if let Some(model) = active.filter(|model| model.error.is_none()) {
            ui.separator();
            ui.strong("当前模型");
            self.mesh_controls(ui, model);
            ui.collapsing("相机参数", |ui| self.camera_controls(ui, snapshot));
        }
        ui.separator();
        ui.collapsing("场景", |ui| {
            if let Some(scene) = &snapshot.scene {
                ui.add(egui::Label::new(scene.as_ref()).wrap());
                let mut enabled = snapshot.scene_visible;
                if ui.checkbox(&mut enabled, "启用场景").changed() {
                    self.send(Command::SceneVisible(enabled));
                }
                if ui.button("卸载场景").clicked() {
                    self.send(Command::UnloadScene);
                }
            } else {
                ui.weak("未加载场景；可从资源组合按需载入。");
            }
        });
    }

    fn texture_sources(&mut self, ui: &mut egui::Ui) {
        ui.strong("贴图来源（按图槽顺序）");
        if self.textures.is_empty() {
            ui.weak("未选择贴图；可选择贴图组或单张 PNG / DDS，再追加其他贴图。");
        }
        let mut slot = 0;
        let mut remove = None;
        for (index, source) in self.textures.iter().enumerate() {
            let count = texture_count(source);
            ui.push_id(("texture-source", index), |ui| {
                ui.horizontal(|ui| {
                    ui.small(match count {
                        0 => format!("来源 {} · 空贴图组", index + 1),
                        1 => format!("来源 {} · 图槽 {slot}", index + 1),
                        _ => format!("来源 {} · 图槽 {slot}–{}", index + 1, slot + count - 1),
                    });
                    if ui.small_button("移除").clicked() {
                        remove = Some(index);
                    }
                });
                let name = source.name();
                ui.add(egui::Label::new(&name).truncate())
                    .on_hover_text(name);
            });
            slot += count;
        }
        if let Some(index) = remove {
            self.textures.remove(index);
        }
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

    fn bones(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.checkbox(&mut self.show_bones, "显示骨架");
        ui.small(format!("{} 个原生骨骼 · 双击聚焦", snapshot.bones.len()));
        egui::ScrollArea::vertical()
            .id_salt("workbench-bones")
            .max_height(ui.available_height().max(120.0))
            .show(ui, |ui| {
                for bone in snapshot.bones.iter() {
                    let response = ui.selectable_label(
                        self.bone == Some(bone.index),
                        format!(
                            "节点 {} · 父节点 {}",
                            bone.index,
                            bone.parent
                                .map_or_else(|| "—".into(), |parent| parent.to_string())
                        ),
                    );
                    if response.clicked() {
                        self.bone = Some(bone.index);
                    }
                    if response.double_clicked() {
                        self.send(Command::FocusBone(Some(bone.index)));
                    }
                    if self.bone == Some(bone.index) {
                        ui.monospace(format!(
                            "X {:.2}  Y {:.2}  Z {:.2}",
                            bone.position[0], bone.position[1], bone.position[2]
                        ));
                        self.bone_binding_controls(ui, snapshot, bone.index);
                    }
                }
            });
        if ui.button("聚焦整个模型").clicked() {
            self.send(Command::FocusBone(None));
        }
        if let Some(model) = snapshot.active_model
            && snapshot.bone_bindings.iter().any(Option::is_some)
            && ui.button("清除全部姿态跟随").clicked()
        {
            self.send(Command::ClearBoneBindings(model));
        }
    }

    fn bone_binding_controls(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, node: usize) {
        let Some(model) = snapshot.active_model else {
            return;
        };
        let previous = snapshot.bone_bindings.get(node).copied().flatten();
        let mut source = previous;
        ui.horizontal(|ui| {
            ui.label("姿态跟随");
            egui::ComboBox::from_id_salt(("workbench-bone-binding", model, node))
                .selected_text(source.map_or_else(|| "原始骨架姿态".into(), |index| format!("节点 {index}")))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut source, None, "原始骨架姿态");
                    for bone in snapshot.bones.iter().filter(|bone| bone.index != node) {
                        ui.selectable_value(&mut source, Some(bone.index), format!("节点 {}", bone.index));
                    }
                }).response.on_hover_text("使用来源节点的世界姿态，并保留当前节点自身的逆绑定矩阵。仅作用于当前预览模型。");
        });
        if source != previous {
            self.send(Command::BoneBinding {
                model,
                node,
                source,
            });
        }
    }

    fn draw_bones(&self, ui: &egui::Ui, snapshot: &Snapshot) {
        let Some(camera) = snapshot.camera.filter(|_| snapshot.ready) else {
            return;
        };
        if !snapshot.models.iter().any(|model| {
            Some(model.id) == snapshot.active_model && model.visible && model.error.is_none()
        }) {
            return;
        }
        if !self.show_bones && self.bone.is_none() {
            return;
        }
        let screen = ui.ctx().content_rect();
        let region = snapshot.viewport;
        let viewport = egui::Rect::from_min_size(
            screen.min + egui::vec2(region.x * screen.width(), region.y * screen.height()),
            egui::vec2(
                region.width * screen.width(),
                region.height * screen.height(),
            ),
        );
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
        for bone in snapshot.bones.iter() {
            let selected = self.bone == Some(bone.index);
            if !self.show_bones && !selected {
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
                .and_then(|index| snapshot.bones.iter().find(|bone| bone.index == index))
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
    asset_nodes: &[Vec<usize>],
    action_width: f32,
    show_encoding_layers: bool,
    node: &mut usize,
    load: &mut Option<PathBuf>,
    preview: &mut Option<usize>,
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
                        asset_nodes,
                        action_width,
                        show_encoding_layers,
                        node,
                        load,
                        preview,
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
                    document,
                    document.root,
                    asset_nodes,
                    action_width,
                    show_encoding_layers,
                    node,
                    preview,
                    details,
                );
            });
        } else {
            let response = ui
                .horizontal(|ui| {
                    ui.add_space(ui.spacing().indent);
                    tree_row(ui, name.as_ref(), selected, 0, false, action_width).0
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
    document: &Arc<Document>,
    index: usize,
    asset_nodes: &[Vec<usize>],
    action_width: f32,
    show_encoding_layers: bool,
    selected: &mut usize,
    preview: &mut Option<usize>,
    details: &mut Option<usize>,
) -> Option<(egui::Response, Option<egui::Response>)> {
    let original = document.nodes.get(index)?;
    let root = index == document.root;
    let index = visible_node(document, index, show_encoding_layers);
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
    let group_count = asset_nodes.get(index).map_or(0, Vec::len);
    let motion = ResourceRef {
        document: document.clone(),
        node: index,
    }
    .kind()
        == Kind::Motion;
    let response = if node.children.is_empty() && !node.deferred {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            tree_row(
                ui,
                &label,
                *selected == index,
                group_count,
                motion,
                action_width,
            )
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
            let response = tree_row(
                ui,
                &label,
                *selected == index,
                group_count,
                motion,
                action_width,
            );
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
            for &child in &node.children {
                let _ = tree(
                    ui,
                    document,
                    child,
                    asset_nodes,
                    action_width,
                    show_encoding_layers,
                    selected,
                    preview,
                    details,
                );
            }
        });
        header.inner
    };
    if response.0.clicked() {
        *selected = index;
    }
    if response.1.as_ref().is_some_and(egui::Response::clicked) {
        *selected = index;
        *preview = Some(index);
    }
    Some(response)
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

fn texture_count(source: &ResourceRef) -> usize {
    let Some(node) = source
        .document
        .payload(source.node)
        .and_then(|index| source.document.nodes.get(index))
    else {
        return 0;
    };
    match node.kind {
        Kind::Png | Kind::Dds => 1,
        Kind::Txb | Kind::Archive => node.children.len(),
        _ => 0,
    }
}

fn group_action_width(ui: &egui::Ui, count: usize) -> f32 {
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
    text_width(
        RichText::new(format!("模型组 {}", count.max(1)))
            .strong()
            .into(),
    ) + text_width("预览".into())
        + ui.spacing().button_padding.x * 2.0
        + ui.spacing().item_spacing.x * 2.0
}

fn tree_row(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    group_count: usize,
    motion: bool,
    action_width: f32,
) -> (egui::Response, Option<egui::Response>) {
    // Reserve this trailing area even when no group is present. Its contents
    // never move the name's click target or the branch's expansion arrow.
    let height = ui.spacing().interact_size.y;
    let width =
        (ui.clip_rect().right().min(ui.max_rect().right()) - ui.next_widget_position().x).max(0.0);
    let action_width = action_width.min(width);
    let name_width = (width - action_width - ui.spacing().item_spacing.x).max(0.0);
    let label = ui
        .add_sized(
            [name_width, height],
            egui::Button::selectable(selected, ())
                .left_text(label)
                .truncate(),
        )
        .on_hover_text(label);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(action_width, height), egui::Sense::hover());
    let button = (group_count != 0 || motion).then(|| {
        let mut actions = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let (caption, help) = if motion {
            ("播放", "将此动画加载到当前模型并从头播放".to_owned())
        } else {
            ("预览", format!("预览此节点下的全部 {group_count} 个模型组"))
        };
        let response = actions.small_button(caption).on_hover_text(help);
        if group_count != 0 {
            actions.add(
                egui::Label::new(
                    RichText::new(format!("模型组 {group_count}"))
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
    use crate::inspect::Field;

    fn preview_fixture() -> Workbench {
        let root = std::env::temp_dir().join("mhf-workbench-multiple-model-fixture");
        let mut worker = Worker::start(root.clone(), root.clone()).unwrap();
        worker.stop();
        let worker = Arc::new(worker);
        let _ = worker.updates();
        let mut workbench = Workbench::new(Arc::new(Control::default()), worker, root);
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
            deferred: false,
            error: None,
            fields: Vec::new(),
        };
        Arc::new(Document {
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
        })
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
    fn opening_or_refreshing_a_file_only_identifies_its_model_groups() {
        let mut workbench = preview_fixture();
        workbench.loaded_document(multiple_models());
        assert!(workbench.control.commands().is_empty());
        assert_eq!(
            workbench
                .scoped_assets(0)
                .iter()
                .map(|bundle| bundle.model.node)
                .collect::<Vec<_>>(),
            [1, 5]
        );
        workbench.preview_node(0);
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadAssets(bundles)] if bundles.len() == 2)
        );
        workbench.loaded_document(multiple_models());
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn model_nodes_keep_manual_selection_without_group_preview_actions() {
        let mut workbench = preview_fixture();
        let document = multiple_models();
        workbench.loaded_document(document.clone());
        let context = egui::Context::default();
        for index in [1, 5] {
            let (_, button, _, _) = draw_tree(&mut workbench, &context, index, 0.0, vec![]);
            assert!(button.is_none());
            workbench.preview_node(index);
            workbench.select_resource(ResourceRef {
                document: document.clone(),
                node: index,
            });
            assert_eq!(workbench.model.as_ref().unwrap().node, index);
        }
        assert!(workbench.control.commands().is_empty());
    }

    #[test]
    fn browsing_and_document_updates_preserve_the_selected_inspector_tab() {
        for tab in [
            InspectorTab::Models,
            InspectorTab::Bones,
            InspectorTab::Resource,
        ] {
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
        assert!(workbench.assets.is_empty());
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
                            workbench.show_encoding_layers,
                        ),
                    ));
                    let action_width = group_action_width(ui, workbench.assets.len());
                    responses = tree(
                        ui,
                        workbench.document.as_ref().unwrap(),
                        index,
                        &workbench.asset_nodes,
                        action_width,
                        workbench.show_encoding_layers,
                        &mut workbench.node,
                        &mut preview,
                        &mut details,
                    );
                },
            )
            .drop_without_applying_deltas();
        if let Some(index) = preview {
            workbench.preview_node(index);
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
    fn group_labels_fit_the_current_font_and_count_without_elision_or_overlap() {
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
                    let action_width = group_action_width(ui, count);
                    let (name, button) = ui
                        .horizontal(|ui| {
                            tree_row(
                                ui,
                                &"long-resource-name".repeat(8),
                                false,
                                count,
                                false,
                                action_width,
                            )
                        })
                        .inner;
                    rectangles = Some((name.rect, button.unwrap().rect));
                },
            );
            let (name, button) = rectangles.unwrap();
            let label = format!("模型组 {count}");
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
    fn right_aligned_group_actions_preserve_the_name_and_arrow_positions() {
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
        workbench.asset_nodes.clear();
        let context = egui::Context::default();
        let (before, button, id, _) = draw_tree(&mut workbench, &context, 12, 0.0, vec![]);
        assert!(button.is_none());
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
        assert!(workbench.tab == InspectorTab::Models);
        assert!(
            !egui::collapsing_header::CollapsingState::load(&context, id)
                .unwrap()
                .is_open()
        );
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadAssets(bundles)] if bundles.len() == 1 && bundles[0].model.node == 5)
        );
        assert_eq!(workbench.model.as_ref().unwrap().node, 5);
        assert_eq!(workbench.skeleton.as_ref().unwrap().node, 6);
        assert_eq!(workbench.textures[0].node, 7);
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
        assert!(!workbench.show_encoding_layers);
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
            matches!(workbench.control.commands().as_slice(), [Command::LoadAssets(bundles)] if bundles.len() == 2)
        );

        workbench.show_encoding_layers = true;
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
    fn selecting_parts_and_loading_motion_remain_explicit() {
        let mut workbench = preview_fixture();
        let document = multiple_models();
        workbench.loaded_document(document.clone());
        for node in [5, 6, 7] {
            workbench.select_resource(ResourceRef {
                document: document.clone(),
                node,
            });
        }
        assert_eq!(workbench.model.as_ref().unwrap().node, 5);
        assert_eq!(workbench.skeleton.as_ref().unwrap().node, 6);
        assert_eq!(workbench.textures[0].node, 7);
        assert!(workbench.control.commands().is_empty());
        workbench.select_resource(ResourceRef { document, node: 9 });
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::LoadMotion(source)] if source.node == 9)
        );
    }

    #[test]
    fn texture_selection_replaces_sources_and_append_buttons_keep_duplicates() {
        for node in [3, 4, 8] {
            let mut workbench = preview_fixture();
            let mut document = (*multiple_models()).clone();
            document.nodes[8].kind = Kind::Dds;
            let document = Arc::new(document);
            workbench.loaded_document(document.clone());
            workbench.node = node;
            workbench.textures = vec![ResourceRef {
                document: document.clone(),
                node: 7,
            }];
            let context = egui::Context::default();
            let draw = |workbench: &mut Workbench, events| {
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
            draw(&mut workbench, vec![]);
            draw(&mut workbench, vec![]);
            draw(&mut workbench, key(egui::Key::Tab));
            draw(&mut workbench, key(egui::Key::Space));
            assert_eq!(workbench.textures.len(), 1);
            assert_eq!(workbench.textures[0].node, node);
            draw(&mut workbench, key(egui::Key::Tab));
            draw(&mut workbench, key(egui::Key::Space));
            draw(&mut workbench, key(egui::Key::Space));
            assert_eq!(workbench.textures.len(), 3);
            assert!(workbench.textures.iter().all(|source| source.node == node));
            assert!(workbench.control.commands().is_empty());
        }
    }

    #[test]
    fn ordered_texture_sources_survive_file_changes_and_removal_keeps_the_remaining_order() {
        let mut workbench = preview_fixture();
        let first = multiple_models();
        let second = multiple_models();
        workbench.loaded_document(first.clone());
        workbench.select_resource(ResourceRef {
            document: first.clone(),
            node: 1,
        });
        workbench.textures = vec![
            ResourceRef {
                document: first.clone(),
                node: 3,
            },
            ResourceRef {
                document: second.clone(),
                node: 8,
            },
            ResourceRef {
                document: first.clone(),
                node: 3,
            },
        ];
        workbench.loaded_document(second.clone());
        let bundle = workbench.selected_bundle().unwrap();
        assert!(Arc::ptr_eq(&bundle.model.document, &first));
        assert_eq!(
            bundle
                .textures
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>(),
            [3, 8, 3]
        );
        let context = egui::Context::default();
        let draw = |workbench: &mut Workbench, events| {
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
                    |ui| workbench.texture_sources(ui),
                )
                .drop_without_applying_deltas();
        };
        draw(&mut workbench, vec![]);
        draw(&mut workbench, vec![]);
        for key in [egui::Key::Tab, egui::Key::Space] {
            draw(
                &mut workbench,
                [true, false]
                    .into_iter()
                    .map(|pressed| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    })
                    .collect(),
            );
        }
        assert_eq!(
            workbench
                .textures
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>(),
            [8, 3]
        );
        assert!(Arc::ptr_eq(&workbench.textures[0].document, &second));
        assert!(Arc::ptr_eq(&workbench.textures[1].document, &first));
        assert!(workbench.control.commands().is_empty());
        workbench.send(Command::AddAsset(workbench.selected_bundle().unwrap()));
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::AddAsset(bundle)] if bundle.textures.iter().map(|source| source.node).collect::<Vec<_>>() == [8, 3])
        );
    }

    #[test]
    fn texture_slot_counts_follow_payloads_and_an_explicit_empty_group_is_valid() {
        let mut workbench = preview_fixture();
        let mut document = (*multiple_models()).clone();
        document.nodes[3].children = vec![4, 8];
        document.nodes[7].children.clear();
        document.nodes[8].kind = Kind::Dds;
        let mut wrapper = document.nodes[0].clone();
        wrapper.kind = Kind::Ecd;
        wrapper.children = vec![3];
        document.nodes.push(wrapper);
        let document = Arc::new(document);
        let source = |node| ResourceRef {
            document: document.clone(),
            node,
        };
        for (node, count) in [(3, 2), (4, 1), (7, 0), (8, 1), (12, 2)] {
            assert_eq!(texture_count(&source(node)), count);
        }
        workbench.select_resource(source(1));
        assert!(workbench.selected_bundle().is_none());
        workbench.select_resource(source(7));
        let bundle = workbench.selected_bundle().unwrap();
        assert_eq!(bundle.textures.len(), 1);
        assert_eq!(bundle.textures[0].node, 7);
    }

    #[test]
    fn motion_rows_play_explicitly_while_tracks_and_channels_remain_inspection_only() {
        let mut workbench = preview_fixture();
        workbench.tab = InspectorTab::Bones;
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
            matches!(workbench.control.commands().as_slice(), [Command::LoadMotion(source)] if source.node == 9)
        );
        assert!(workbench.tab == InspectorTab::Bones);
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
    fn resource_action_buttons_keep_add_and_scene_commands_scoped_to_the_selection() {
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
        // The container previews its groups; each following action targets
        // the bundle next to that button.
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(
            matches!(control.commands().as_slice(), [Command::LoadAssets(bundles)] if bundles.len() == 2)
        );
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(
            matches!(control.commands().as_slice(), [Command::AddAsset(bundle)] if bundle.model.node == 1)
        );
        draw(key(egui::Key::Tab));
        draw(key(egui::Key::Space));
        assert!(
            matches!(control.commands().as_slice(), [Command::LoadScene(bundle)] if bundle.model.node == 1)
        );
    }

    #[test]
    fn mesh_controls_send_only_scoped_visibility_commands_without_focusing() {
        let mut workbench = preview_fixture();
        let control = workbench.control.clone();
        let model = LoadedModel {
            id: 41,
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
            name: "unknown_00000010".repeat(4),
            value: "Long resource field content ".repeat(20),
            offset: 0,
            size: 16,
        });
        workbench.document = Some(Arc::new(document));
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
        workbench.preview_only = true;
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
    fn viewport_shortcuts_preserve_f_and_space_in_resource_search() {
        let mut workbench = preview_fixture();
        workbench.control.publish(Snapshot {
            motion: Some("test motion".into()),
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
        let mut workbench = Workbench::new(Arc::new(Control::default()), worker, root);
        workbench.scanning = false;
        workbench.document = Some(Arc::new(Document {
            root: 0, buffers: vec![Arc::from([0_u8; 16])],
            nodes: vec![Node {
                name: "Z:\\game\\dat\\model\\long-resource-file-name.bin".into(),
                kind: Kind::Unknown, buffer: 0, range: 0..16, children: vec![], deferred: false, error: None,
                fields: vec![Field {name: "unknown_00000010".into(), value: "A long resource value with enough words to wrap within the inspector column".repeat(3), offset: 0, size: 16}],
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
