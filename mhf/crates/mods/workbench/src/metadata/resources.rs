//! Model dependencies declared by validated package layouts. Member indices
//! retain wrappers and reference sites; consumers resolve their bytes separately.

use crate::inspect::{Document, Kind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelResources {
    pub skeleton: Option<usize>,
    pub textures: Vec<usize>,
}

/// Recognize the same complete model/skeleton/texture combinations as the
/// workbench loader. A declaration belongs to one model member, or to its
/// dedicated inner geometry directory, never to an unrelated sibling model.
pub(crate) fn model_resources(document: &Document) -> Vec<(usize, ModelResources)> {
    let mut declarations = Vec::new();
    let mut declare = |scope, model, skeleton, texture| {
        if valid_bundle(document, model, skeleton, texture) {
            declarations.push((
                scope,
                ModelResources {
                    skeleton,
                    textures: vec![texture],
                },
            ));
        }
    };
    for (index, node) in document.nodes.iter().enumerate() {
        if node.kind == Kind::StageObjectPackage {
            if node.error.is_some() {
                continue;
            }
            let Some(bytes) = document.bytes(index) else {
                continue;
            };
            let Ok(package) = mhf_resource::stage::ObjectPackage::parse(bytes, node.children.len())
            else {
                continue;
            };
            let member = |kind| {
                package
                    .member(kind)
                    .and_then(|member| node.children.get(member.entry.index).copied())
            };
            let Some(texture) = member(3) else { continue };
            let skeleton = match member(2) {
                Some(index) => {
                    let Some(payload) = payload(document, index) else {
                        continue;
                    };
                    (document.nodes[payload].kind != Kind::Empty).then_some(index)
                }
                None => None,
            };
            for model in package.members.iter().filter(|member| member.kind == 1) {
                if let Some(&model) = node.children.get(model.entry.index) {
                    declare(model, model, skeleton, texture);
                }
            }
            continue;
        }
        if !matches!(
            node.kind,
            Kind::Archive | Kind::Momo | Kind::Mha | Kind::Stage
        ) {
            continue;
        }
        let resolved: Vec<_> = node
            .children
            .iter()
            .map(|&index| payload(document, index).unwrap_or(index))
            .collect();
        let mut model_run_end = 0;
        let mut column_width = 0;
        for (position, &member) in resolved.iter().enumerate() {
            let raw = node.children[position];
            let member_node = &document.nodes[member];
            let (scope, model, skeleton, texture_position) = if member_node.kind == Kind::Fmod {
                // Every model in a contiguous run shares the same column
                // layout decision. Inspect that run and its companions once.
                if position >= model_run_end {
                    let count = resolved[position..]
                        .iter()
                        .take_while(|&&index| document.nodes[index].kind == Kind::Fmod)
                        .count();
                    model_run_end = position + count;
                    let columns = count > 1
                        && resolved
                            .get(model_run_end..model_run_end + 2 * count)
                            .is_some_and(|remaining| {
                                remaining[..count]
                                    .iter()
                                    .all(|&index| document.nodes[index].kind == Kind::Fskl)
                                    && remaining[count..].iter().all(|&index| {
                                        matches!(
                                            document.nodes[index].kind,
                                            Kind::Txb | Kind::Png | Kind::Dds
                                        )
                                    })
                            });
                    column_width = if columns { count } else { 0 };
                }
                if column_width != 0 {
                    (
                        raw,
                        raw,
                        Some(node.children[position + column_width]),
                        position + 2 * column_width,
                    )
                } else {
                    let Some(&next) = resolved.get(position + 1) else {
                        continue;
                    };
                    let skeleton = (document.nodes[next].kind == Kind::Fskl)
                        .then_some(node.children[position + 1]);
                    (
                        raw,
                        raw,
                        skeleton,
                        position + 1 + usize::from(skeleton.is_some()),
                    )
                }
            } else if matches!(member_node.kind, Kind::Archive | Kind::Momo | Kind::Mha)
                && member_node.error.is_none()
                && member_node.children.len() == 2
            {
                let model = member_node.children[0];
                let skeleton = member_node.children[1];
                if payload(document, model)
                    .is_none_or(|index| document.nodes[index].kind != Kind::Fmod)
                    || payload(document, skeleton)
                        .is_none_or(|index| document.nodes[index].kind != Kind::Fskl)
                {
                    continue;
                }
                (raw, model, Some(skeleton), position + 1)
            } else {
                continue;
            };
            if let Some(&texture) = node.children.get(texture_position) {
                declare(scope, model, skeleton, texture);
            }
        }
    }
    declarations
}

fn valid_bundle(
    document: &Document,
    model: usize,
    skeleton: Option<usize>,
    texture: usize,
) -> bool {
    if payload(document, model).is_none_or(|index| document.nodes[index].kind != Kind::Fmod)
        || skeleton.is_some_and(|index| {
            payload(document, index).is_none_or(|index| document.nodes[index].kind != Kind::Fskl)
        })
    {
        return false;
    }
    let Some(texture) = payload(document, texture).map(|index| &document.nodes[index]) else {
        return false;
    };
    match texture.kind {
        Kind::Png | Kind::Dds => true,
        Kind::Txb | Kind::Archive => {
            (texture.kind == Kind::Txb || !texture.children.is_empty())
                && texture.children.iter().all(|&index| {
                    payload(document, index).is_some_and(|index| {
                        let image = &document.nodes[index];
                        matches!(image.kind, Kind::Png | Kind::Dds) || image.range.is_empty()
                    })
                })
        }
        _ => false,
    }
}

fn payload(document: &Document, node: usize) -> Option<usize> {
    let target = document.payload(node)?;
    let mut current = node;
    loop {
        let node = document.nodes.get(current)?;
        if node.error.is_some() {
            return None;
        }
        if current == target {
            return Some(target);
        }
        current = *node.children.first()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inspect::Node, metadata::Metadata};
    use std::sync::Arc;

    fn node(kind: Kind, children: &[usize]) -> Node {
        Node {
            name: String::new(),
            kind,
            buffer: 0,
            range: 0..0,
            fields: Vec::new(),
            metadata: Metadata::default(),
            children: children.into(),
            deferred: false,
            error: None,
        }
    }

    fn document(nodes: Vec<Node>) -> Document {
        Document {
            nodes,
            root: 0,
            buffers: vec![Arc::from([])],
        }
    }

    fn install(document: &mut Document) {
        for (scope, value) in model_resources(document) {
            document.nodes[scope].metadata.insert(value);
        }
    }

    #[test]
    fn parallel_model_columns_have_separate_declarations_and_keep_encoded_members() {
        let mut document = document(vec![
            node(Kind::Archive, &[1, 2, 3, 4, 5, 6]),
            node(Kind::Ecd, &[7]),
            node(Kind::Fmod, &[]),
            node(Kind::Jkr, &[8]),
            node(Kind::Fskl, &[]),
            node(Kind::Txb, &[9]),
            node(Kind::Png, &[]),
            node(Kind::Fmod, &[10]),
            node(Kind::Fskl, &[]),
            node(Kind::Png, &[]),
            node(Kind::Object, &[]),
        ]);
        install(&mut document);
        let scopes = document.metadata();
        assert!(scopes.resolve::<ModelResources>(0).is_none());
        let first = scopes.resolve::<ModelResources>(7).unwrap();
        assert_eq!(first.source, 1);
        assert_eq!(
            first.value,
            &ModelResources {
                skeleton: Some(3),
                textures: vec![5]
            }
        );
        assert_eq!(scopes.resolve::<ModelResources>(10).unwrap().source, 1);
        let second = scopes.resolve::<ModelResources>(2).unwrap();
        assert_eq!(second.source, 2);
        assert_eq!(
            second.value,
            &ModelResources {
                skeleton: Some(4),
                textures: vec![6]
            }
        );
    }

    #[test]
    fn model_runs_keep_separate_column_widths_and_noncolumn_tail_pairs() {
        use Kind::{Dds, Fmod, Fskl, Png, Unknown};

        for (kinds, expected) in [
            (vec![Fmod, Fmod, Fskl, Png], vec![(2, Some(3), 4)]),
            (vec![Fmod, Fmod, Png], vec![(2, None, 3)]),
            (
                vec![
                    Fmod, Fmod, Fskl, Fskl, Png, Dds, Unknown, Fmod, Fmod, Fskl, Png, Fmod, Fmod,
                    Fmod, Fskl, Fskl, Fskl, Png, Png, Png,
                ],
                vec![
                    (1, Some(3), 5),
                    (2, Some(4), 6),
                    (9, Some(10), 11),
                    (12, Some(15), 18),
                    (13, Some(16), 19),
                    (14, Some(17), 20),
                ],
            ),
        ] {
            let children = (1..=kinds.len()).collect::<Vec<_>>();
            let mut nodes = vec![node(Kind::Archive, &children)];
            nodes.extend(kinds.into_iter().map(|kind| node(kind, &[])));
            let actual = model_resources(&document(nodes))
                .into_iter()
                .map(|(scope, value)| (scope, value.skeleton, value.textures[0]))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn dedicated_geometry_directory_supplies_external_textures_to_its_children() {
        let mut document = document(vec![
            node(Kind::Archive, &[1, 2]),
            node(Kind::Ecd, &[3]),
            node(Kind::Txb, &[6]),
            node(Kind::Archive, &[4, 5]),
            node(Kind::Fmod, &[]),
            node(Kind::Fskl, &[]),
            node(Kind::Png, &[]),
        ]);
        install(&mut document);
        let scopes = document.metadata();
        let model = scopes.resolve::<ModelResources>(4).unwrap();
        assert_eq!(model.source, 1);
        assert_eq!(
            model.value,
            &ModelResources {
                skeleton: Some(5),
                textures: vec![2]
            }
        );
        assert_eq!(scopes.resolve::<ModelResources>(5).unwrap().source, 1);
        assert!(scopes.resolve::<ModelResources>(2).is_none());
    }

    #[test]
    fn stage_descriptors_keep_reference_sites_instead_of_reparenting_targets() {
        let descriptor = [1, 0, 3, 0, 3, 2, 1];
        let mut bytes = vec![0; 4 + 4 * 8];
        bytes[..4].copy_from_slice(&4_u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&36_u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&(descriptor.len() as u32).to_le_bytes());
        bytes.extend(descriptor);
        let mut document = document(vec![
            node(Kind::Archive, &[1, 5]),
            node(Kind::Archive, &[2, 3, 4]),
            node(Kind::Fmod, &[]),
            node(Kind::Fskl, &[]),
            node(Kind::Png, &[]),
            node(Kind::StageObjectPackage, &[6, 7, 8, 9]),
            node(Kind::Block, &[]),
            node(Kind::StageResourceReference, &[4]),
            node(Kind::StageResourceReference, &[3]),
            node(Kind::StageResourceReference, &[2]),
        ]);
        document.nodes[5].range = 0..bytes.len();
        document.buffers[0] = bytes.into();
        install(&mut document);
        let scopes = document.metadata();
        let original = scopes.resolve::<ModelResources>(2).unwrap();
        assert_eq!(original.source, 2);
        assert_eq!(
            original.value,
            &ModelResources {
                skeleton: Some(3),
                textures: vec![4]
            }
        );
        let referenced = scopes.resolve::<ModelResources>(9).unwrap();
        assert_eq!(referenced.source, 9);
        assert_eq!(
            referenced.value,
            &ModelResources {
                skeleton: Some(8),
                textures: vec![7]
            }
        );
        assert_eq!(scopes.path(2).unwrap(), [0, 1, 2]);
        assert_eq!(scopes.path(9).unwrap(), [0, 5, 9]);
    }

    #[test]
    fn failed_dependency_wrappers_do_not_create_an_automatic_bundle() {
        let mut document = document(vec![
            node(Kind::Archive, &[1, 2, 3]),
            node(Kind::Fmod, &[]),
            node(Kind::Jkr, &[4]),
            node(Kind::Png, &[]),
            node(Kind::Fskl, &[]),
        ]);
        document.nodes[2].error = Some("decode failed".into());
        assert!(model_resources(&document).is_empty());
    }
}
