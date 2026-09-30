use super::Workbench;
use crate::{
    field::{FieldReference, ReferenceCollection},
    inspect::{Kind, NativeId, NodeAddress},
    metadata::ModelResources,
    preview::ResourceRef,
};
use mhf_resource::PathSegment::{Field as Key, Index};
use mhf_resource::ResourcePath;
use mhf_ui::resource_reference::{ResourceReference, ResourceReferenceResponse, ResourceTarget};
use std::{borrow::Cow, path::Path};

pub(super) fn show_source(
    ui: &mut egui::Ui,
    source: &ResourceRef,
    root: &Path,
    name: Option<&str>,
    help: &str,
    activate: bool,
) -> ResourceReferenceResponse {
    let name = name
        .map(Cow::Borrowed)
        .unwrap_or_else(|| Cow::Owned(source.short_name()));
    let address = source.resource_address(root, None);
    let target = address.as_ref().map_or_else(
        || ResourceTarget::Source(&source.document.source),
        |address| ResourceTarget::from(&address.path),
    );
    let mut widget = ResourceReference::new(target)
        .label(&name)
        .help(help)
        .compact(true)
        .activate(activate && address.as_ref().is_some_and(|address| address.exact));
    if let Some(payload) = source.document.payload(source.node) {
        widget = widget.source_range(source.document.nodes[payload].range.clone());
    }
    widget.show(ui)
}

impl Workbench {
    pub(super) fn show_reference(
        &self,
        ui: &mut egui::Ui,
        source: &ResourceRef,
        reference: &FieldReference,
        span: Option<std::ops::Range<usize>>,
        activate: bool,
    ) -> Option<ResourcePath> {
        if let FieldReference::Many(references) = reference {
            let mut activated = None;
            if let Some(first) = references.first() {
                ui.horizontal(|ui| {
                    if references.len() > 1 {
                        let button = ui.small_button(format!("{} 项", references.len()));
                        egui::Popup::from_toggle_button_response(&button)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .width(
                                360.0_f32
                                    .min(ui.ctx().content_rect().width() - 24.0)
                                    .max(24.0),
                            )
                            .show(|ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("resource-references")
                                    .max_height(240.0)
                                    .show_rows(
                                        ui,
                                        ui.spacing().interact_size.y,
                                        references.len(),
                                        |ui, rows| {
                                            for index in rows {
                                                ui.push_id(index, |ui| {
                                                    activated = self
                                                        .show_reference(
                                                            ui,
                                                            source,
                                                            &references[index],
                                                            span.clone(),
                                                            activate,
                                                        )
                                                        .or(activated.take());
                                                });
                                            }
                                        },
                                    );
                            });
                    }
                    activated = self
                        .show_reference(ui, source, first, span, activate)
                        .or(activated.take());
                });
            } else {
                ui.weak("无资源引用");
            }
            return activated;
        }
        let root = &self.editing.source_root;
        let mut resolved: Result<Option<ResourcePath>, String> = Ok(None);
        let target = match reference {
            FieldReference::Address(address) => {
                let path = (|| {
                    let mut path = exact_path(&source.related(address.anchor)?, root)?;
                    for segment in &address.segments {
                        path.push(segment.clone()).ok()?;
                    }
                    Some(path)
                })();
                path.map(ResourceTarget::from)
                    .unwrap_or_else(|| ResourceTarget::Source(&source.document.source))
            }
            FieldReference::Motion(reference) => ResourceTarget::Motion(*reference),
            FieldReference::Attack(reference) => {
                let directory = self
                    .editing
                    .sessions
                    .get(&root.join("mhfsdt.bin"))
                    .and_then(|session| session.document.attack_directory.as_deref())
                    .or(source.document.attack_directory.as_deref());
                resolved = match directory {
                    Some(Ok(directory)) => directory
                        .resolve(*reference)
                        .map_err(|error| error.to_string()),
                    Some(Err(error)) => Err(error.clone()),
                    None => Ok(None),
                };
                ResourceTarget::Attack(*reference)
            }
            FieldReference::NativeScript { table, index } => ResourceTarget::NativeScript {
                table: *table,
                index: *index,
            },
            FieldReference::Curve {
                reference,
                lookup,
                target,
            } => {
                resolved = Ok(target
                    .and_then(|node| source.related(node))
                    .and_then(|source| exact_path(&source, root)));
                ResourceTarget::Curve {
                    reference: *reference,
                    lookup,
                }
            }
            FieldReference::Indexed(collection, index, target) => {
                let target = match target {
                    Some(node) => source.related(*node),
                    None => indexed_source(source, *collection, *index),
                };
                resolved = Ok(target.and_then(|source| exact_path(&source, root)));
                ResourceTarget::Index {
                    collection: collection.schema_name(),
                    index: *index,
                }
            }
            FieldReference::Many(_) => unreachable!(),
        };
        let mut widget = ResourceReference::new(target)
            .compact(true)
            .activate(activate);
        match &resolved {
            Ok(Some(path)) => widget = widget.resolved_path(path),
            Err(error) => widget = widget.help(error),
            Ok(None) => {}
        }
        if let Some(span) = span {
            widget = widget.source_range(span);
        }
        widget.show(ui).activated
    }
}

fn exact_path(source: &ResourceRef, root: &Path) -> Option<ResourcePath> {
    source
        .resource_address(root, None)
        .filter(|address| address.exact)
        .map(|address| address.path)
}

fn nearest(source: &ResourceRef, kind: Kind) -> Option<ResourceRef> {
    source
        .context()
        .iter()
        .rev()
        .find(|&&node| source.document.nodes[node].kind == kind)
        .and_then(|&node| source.related(node))
}

fn native_child(source: &ResourceRef, key: &str, index: u32) -> Option<ResourceRef> {
    let payload = source.document.payload(source.node)?;
    source.document.nodes[payload]
        .children
        .iter()
        .find_map(|&node| {
            let address = source.document.nodes[node].address.as_ref()?;
            (address.anchor == payload
                && matches!(address.segments.as_slice(), [Key(field), Index(member)] if field == key && *member == index))
            .then(|| source.related(node))
            .flatten()
        })
}

pub(super) fn indexed_source(
    source: &ResourceRef,
    collection: ReferenceCollection,
    index: u32,
) -> Option<ResourceRef> {
    match collection {
        ReferenceCollection::SkeletonNodes | ReferenceCollection::SkeletonNodeIds => {
            let skeleton = nearest(source, Kind::Fskl).or_else(|| {
                let target = source.scope().get::<ModelResources>()?.value.skeleton?;
                source.related(target)
            })?;
            if collection == ReferenceCollection::SkeletonNodes {
                native_child(&skeleton, "nodes", index)
            } else {
                let payload = skeleton.document.payload(skeleton.node)?;
                skeleton.document.nodes[payload]
                    .children
                    .iter()
                    .find_map(|&node| {
                        let Some(NativeId::Skeleton(id)) = skeleton.document.nodes[node].native_id
                        else {
                            return None;
                        };
                        (i64::from(id) == i64::from(index))
                            .then(|| skeleton.related(node))
                            .flatten()
                    })
            }
        }
        ReferenceCollection::ModelMaterials
        | ReferenceCollection::ModelTextures
        | ReferenceCollection::ModelMeshes => {
            let model = nearest(source, Kind::Fmod)?;
            let key = match collection {
                ReferenceCollection::ModelMaterials => "materials",
                ReferenceCollection::ModelMeshes => "objects",
                _ => "textures",
            };
            model.document.nodes[model.node]
                .children
                .iter()
                .find_map(|&section| native_child(&model.related(section)?, key, index))
        }
        ReferenceCollection::ModelMaterialSlots => {
            let object = nearest(source, Kind::Object)?;
            let material = *object.document.nodes[object.node]
                .material_slots
                .get(index as usize)?;
            indexed_source(&object, ReferenceCollection::ModelMaterials, material)
        }
        ReferenceCollection::TextureImages => {
            let textures = &source.scope().get::<ModelResources>()?.value.textures;
            let mut remaining = index as usize;
            for &texture in textures {
                let texture = source.related(texture)?;
                let payload = texture.document.payload(texture.node)?;
                let node = &texture.document.nodes[payload];
                if matches!(node.kind, Kind::Txb | Kind::Archive) {
                    if remaining < node.children.len() {
                        return texture.related(node.children[remaining]);
                    }
                    remaining -= node.children.len();
                } else if remaining == 0 {
                    return Some(texture);
                } else {
                    remaining -= 1;
                }
            }
            None
        }
        ReferenceCollection::EffectResources => {
            let archive = nearest(source, Kind::EffectArchive)?;
            let mut matches =
                archive.document.nodes[archive.node]
                    .children
                    .iter()
                    .filter(|&&node| {
                        matches!(archive.document.nodes[node].native_id, Some(NativeId::Effect(id)) if u32::from(id) == index)
                    });
            let node = *matches.next()?;
            matches
                .next()
                .is_none()
                .then(|| archive.related(node))
                .flatten()
        }
        _ => None,
    }
}

impl Workbench {
    pub(super) fn show_effect_definition_reference(
        &mut self,
        ui: &mut egui::Ui,
        source: &ResourceRef,
        entry: &crate::preview::effects::DefinitionSnapshot,
    ) {
        let reference = source.document.nodes[source.node]
            .children
            .iter()
            .find_map(|&node| {
                let child = &source.document.nodes[node];
                let address = child.address.as_ref()?;
                (address.anchor == source.node
                    && matches!(address.segments.as_slice(), [Key(field), Index(slot)] if field == "definitions" && *slot == entry.slot as u32))
                .then(|| child.fields.first()?.reference.clone())
                .flatten()
            })
            .or_else(|| {
                crate::preview::effects::is_definition(source.kind()).then(|| {
                    source.document.nodes[source.node]
                        .fields
                        .first()
                        .and_then(|field| field.reference.clone())
                        .unwrap_or(FieldReference::Address(NodeAddress {
                            anchor: source.node,
                            segments: Vec::new(),
                        }))
                })
            })
            .unwrap_or(FieldReference::Indexed(
                ReferenceCollection::EffectDefinitions,
                u32::from(entry.id),
                None,
            ));
        if let Some(path) = self.show_reference(ui, source, &reference, None, true) {
            self.address_input = path.to_string();
            self.navigate_resource(path);
        }
    }

    pub(super) fn show_source_reference(
        &mut self,
        ui: &mut egui::Ui,
        source: &ResourceRef,
        name: Option<&str>,
        help: &str,
    ) -> egui::Response {
        let output = show_source(ui, source, &self.editing.source_root, name, help, true);
        if let Some(path) = output.activated {
            self.address_input = path.to_string();
            self.navigate_resource(path);
        }
        output.response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Document, Node, NodeAddress};
    use mhf_resource::PathSegment;
    use std::sync::Arc;

    fn node(kind: Kind, children: Vec<usize>, anchor: usize, segments: Vec<PathSegment>) -> Node {
        Node {
            native_id: None,
            material_slots: Vec::new(),
            name: "显示标签 900".into(),
            address: Some(NodeAddress { anchor, segments }),
            kind,
            buffer: 0,
            range: 0..1,
            fields: Vec::new(),
            metadata: Default::default(),
            children,
            action: None,
            deferred: false,
            error: None,
        }
    }

    fn model() -> ResourceRef {
        let field =
            |key: &str, index| vec![PathSegment::Field(key.into()), PathSegment::Index(index)];
        let mut document = Document {
            attack_directory: None,
            source: "dat/file.bin".into(),
            root: 0,
            buffers: vec![Arc::from([42])],
            nodes: vec![
                node(Kind::Archive, vec![1, 2, 3], 0, vec![]),
                node(Kind::Fmod, vec![6, 7, 12], 0, vec![PathSegment::Index(0)]),
                node(Kind::Fskl, vec![4, 5], 0, vec![PathSegment::Index(1)]),
                node(Kind::Txb, vec![10, 11], 0, vec![PathSegment::Index(2)]),
                node(Kind::Block, vec![], 2, field("nodes", 0)),
                node(Kind::Bone, vec![], 2, field("nodes", 3)),
                node(Kind::Block, vec![8], 1, vec![PathSegment::Index(8)]),
                node(Kind::Block, vec![9], 1, vec![PathSegment::Index(9)]),
                node(Kind::Material, vec![], 6, field("materials", 4)),
                node(Kind::Texture, vec![], 7, field("textures", 2)),
                node(Kind::Empty, vec![], 3, vec![PathSegment::Index(0)]),
                node(Kind::Dds, vec![], 3, vec![PathSegment::Index(1)]),
                node(Kind::Object, vec![], 1, field("objects", 7)),
            ],
        };
        document.nodes[1].metadata.insert(ModelResources {
            skeleton: Some(2),
            textures: vec![3],
        });
        document.nodes[5].native_id = Some(NativeId::Skeleton(17));
        document.nodes[12].material_slots = vec![4];
        ResourceRef::new(Arc::new(document), 1)
    }

    #[test]
    fn resource_targets_use_native_indices_ids_and_slot_maps_instead_of_ui_positions() {
        let source = model();
        let path = |source: &ResourceRef, collection, index| {
            indexed_source(source, collection, index)
                .and_then(|source| exact_path(&source, Path::new("dat")))
                .map(|path| path.to_string())
        };
        assert_eq!(
            path(&source, ReferenceCollection::SkeletonNodeIds, 17).as_deref(),
            Some("file.bin#1/nodes/3")
        );
        assert_eq!(
            path(&source, ReferenceCollection::SkeletonNodes, 3).as_deref(),
            Some("file.bin#1/nodes/3")
        );
        assert!(path(&source, ReferenceCollection::SkeletonNodes, 17).is_none());
        assert_eq!(
            path(&source, ReferenceCollection::ModelMaterials, 4).as_deref(),
            Some("file.bin#0/8/materials/4")
        );
        assert_eq!(
            path(&source, ReferenceCollection::ModelTextures, 2).as_deref(),
            Some("file.bin#0/9/textures/2")
        );
        assert_eq!(
            path(&source, ReferenceCollection::TextureImages, 1).as_deref(),
            Some("file.bin#2/1")
        );
        assert_eq!(
            path(
                &source.related(12).unwrap(),
                ReferenceCollection::ModelMaterialSlots,
                0
            )
            .as_deref(),
            Some("file.bin#0/8/materials/4")
        );
        assert!(path(&source, ReferenceCollection::ModelMaterialSlots, 0).is_none());
    }

    #[test]
    fn field_reference_copy_and_activation_share_the_actual_target_path() {
        let source = model();
        let reference = FieldReference::Indexed(ReferenceCollection::SkeletonNodeIds, 17, None);
        let mut workbench = super::super::tests::preview_fixture();
        workbench.editing.source_root = "dat".into();
        let context = egui::Context::default();
        let draw = |events| {
            let mut id = egui::Id::NULL;
            let mut activated = None;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 100.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    id = ui.next_auto_id();
                    activated = workbench.show_reference(ui, &source, &reference, Some(0..1), true);
                },
            );
            (output, id, activated)
        };
        let (output, id, _) = draw(vec![]);
        let value = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "file.bin#1/nodes/3" => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        });
        output.drop_without_applying_deltas();
        let value = value.expect("reference main value must be the resolved canonical resource");
        let copy = context
            .read_response(id.with("copy"))
            .unwrap()
            .rect
            .center();
        let click = |point, pressed| {
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        draw(click(copy, true)).0.drop_without_applying_deltas();
        let (output, _, activated) = draw(click(copy, false));
        let copied = output.platform_output.commands.iter().any(|command| matches!(command,egui::OutputCommand::CopyText(text) if text=="file.bin#1/nodes/3"));
        output.drop_without_applying_deltas();
        assert!(copied);
        assert!(activated.is_none(), "copy must not activate the target");
        draw(click(value, true)).0.drop_without_applying_deltas();
        let (output, _, activated) = draw(click(value, false));
        output.drop_without_applying_deltas();
        assert_eq!(
            activated.as_ref().map(ToString::to_string).as_deref(),
            Some("file.bin#1/nodes/3")
        );
    }
}
