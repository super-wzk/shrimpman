use super::{InspectorTab, Workbench};
use crate::{inspect::resource_path::Location, preview::ResourceRef};
use mhf_resource::ResourcePath;

pub(super) fn reveal_id() -> egui::Id {
    egui::Id::new("workbench-resource-path-reveal")
}

impl Workbench {
    pub(super) fn sync_address(&mut self) {
        self.address_error.clear();
        if let Some(navigation) = &self.navigation {
            self.address_input = navigation.to_string();
            return;
        }
        self.address_input = self
            .selection
            .as_ref()
            .filter(|source| source.node == self.node)
            .and_then(|source| {
                source.resource_address(&self.editing.source_root, self.address_field)
            })
            .map_or_else(String::new, |address| address.path.to_string());
    }

    pub(super) fn address_bar(&mut self, ui: &mut egui::Ui) {
        let address = self.selected_source().and_then(|source| {
            source.resource_address(&self.editing.source_root, self.address_field)
        });
        let enter = {
            let response = mhf_ui::resource_reference::ResourceReference::editor(
                egui::Id::new("workbench-resource-path"),
                &mut self.address_input,
            )
            .help(match &address {
                Some(address) if !address.exact => {
                    "当前条目无独立地址，显示所属资源地址。输入资源路径后按 Enter 定位。"
                }
                None if self.document.is_some() => {
                    "源文件无法表示为 DAT 内的 UTF-8 路径。输入资源路径后按 Enter 定位。"
                }
                _ => "相对 DAT 的文件路径，可接 #原始索引/字段。按 Enter 定位资源或字段。",
            })
            .show(ui)
            .response;
            let enter =
                response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if enter {
                ui.memory_mut(|memory| memory.surrender_focus(response.id));
            }
            enter
        };
        if enter {
            self.navigate_address();
        }
        if !self.address_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.address_error);
        }
    }

    pub(super) fn navigate_address(&mut self) {
        match self.address_input.parse::<ResourcePath>() {
            Ok(path) => self.navigate_resource(path),
            Err(error) => self.address_error = error.to_string(),
        }
    }

    pub(super) fn navigate_resource(&mut self, path: ResourcePath) {
        let source = self.editing.source_root.join(path.source());
        self.address_error.clear();
        if self.path.as_ref() != Some(&source) {
            self.open_document(source);
        }
        self.navigation = Some(path);
        self.advance_navigation();
    }

    pub(super) fn advance_navigation(&mut self) {
        let Some(navigation) = &self.navigation else {
            return;
        };
        if self.loading
            || self.expanding.is_some()
            || self.editing.busy
            || self.path.as_ref() != Some(&self.editing.source_root.join(navigation.source()))
        {
            return;
        }
        let Some(document) = self.document.clone() else {
            self.navigation = None;
            return;
        };
        match document.locate_resource(&self.editing.source_root, navigation) {
            Location::Resolved {
                node,
                context,
                field,
            } => {
                self.navigation = None;
                self.address_field = None;
                self.select_source(ResourceRef::at_context(document.clone(), node, context));
                self.tab = InspectorTab::Resource;
                self.reveal_resource = true;
                self.reveal_field = field.is_some();
                self.ensure_catalog_source();
                self.filter.clear();
                self.filter_files();
                self.hex_start = 0;
                self.hex_buffer = false;
                self.hex_selection = None;
                if let Some(field) = field {
                    self.select_field(&document.nodes[node].fields[field], field);
                }
            }
            Location::Expand(node) => self.expand_node(node),
            Location::Missing => {
                self.address_error = format!("找不到资源路径：{navigation}");
                self.navigation = None;
            }
        }
    }

    pub(super) fn ensure_catalog_source(&mut self) {
        let Some(document) = &self.document else {
            return;
        };
        if self
            .catalog
            .entries
            .iter()
            .any(|entry| entry.path == document.source)
        {
            return;
        }
        let Ok(relative) = document
            .source
            .strip_prefix(&self.root)
            .or_else(|_| document.source.strip_prefix(&self.editing.source_root))
        else {
            return;
        };
        if relative.as_os_str().is_empty() {
            return;
        }
        let Some(bytes) = document.buffers.first() else {
            return;
        };
        let mut entries = self.catalog.entries.clone();
        entries.push(crate::catalog::Entry {
            path: document.source.clone(),
            relative_path: relative.into(),
            size: bytes.len() as u64,
            header: bytes[..bytes.len().min(16)].to_vec(),
        });
        entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.catalog = std::sync::Arc::new(crate::catalog::Catalog {
            entries,
            errors: self.catalog.errors.clone(),
        });
    }

    pub(super) fn select_field(&mut self, field: &crate::field::Field, index: usize) {
        self.navigation = None;
        self.address_field = Some(index);
        self.hex_buffer = true;
        self.hex_start = field.binding.range.start / 16 * 16;
        self.hex_selection = Some(field.binding.range.clone());
        self.sync_address();
    }
}
