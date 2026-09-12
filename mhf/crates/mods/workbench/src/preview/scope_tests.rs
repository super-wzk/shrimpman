use super::*;
use crate::{inspect::Node, metadata::ModelResources};

#[derive(Debug, PartialEq, Eq)]
struct Tag(u8);

fn fixture(extra_nodes: usize) -> Arc<Document> {
    let node = |kind, children| Node {
        name: "resource".into(),
        kind,
        buffer: 0,
        range: 0..16,
        fields: Vec::new(),
        metadata: Default::default(),
        children,
        deferred: false,
        error: None,
    };
    let mut document = Document {
        root: 0,
        buffers: vec![Arc::from([0_u8; 16])],
        nodes: vec![
            node(Kind::Archive, vec![1, 2]),
            node(Kind::Archive, vec![3]),
            node(Kind::Archive, vec![4]),
            node(Kind::Archive, vec![5, 6, 7, 9, 10]),
            node(Kind::StageResourceReference, vec![3]),
            node(Kind::Ecd, vec![11]),
            node(Kind::Fskl, vec![]),
            node(Kind::Txb, vec![8]),
            node(Kind::Dds, vec![]),
            node(Kind::Motion, vec![]),
            node(
                Kind::DatRecord(mhf_resource::dat::DATA_TABLES.len() + 3),
                vec![],
            ),
            node(Kind::Fmod, vec![]),
        ],
    };
    document.nodes[0].name = "scopes.bin".into();
    document.nodes[1].metadata.insert(Tag(7));
    document.nodes[2].metadata.insert(Tag(9));
    document.nodes[11].metadata.insert(ModelResources {
        skeleton: Some(6 + extra_nodes),
        textures: vec![7 + extra_nodes],
    });
    if extra_nodes != 0 {
        for value in &mut document.nodes {
            for child in &mut value.children {
                *child += extra_nodes;
            }
        }
        document
            .nodes
            .splice(1..1, (0..extra_nodes).map(|_| node(Kind::Empty, vec![])));
    }
    Arc::new(document)
}

#[test]
fn every_resource_kind_keeps_its_loading_scope_through_reference_containers() {
    let document = fixture(0);
    let first = ResourceRef::new(document.clone(), 1).loadable_resources();
    let second = ResourceRef::new(document.clone(), 2).loadable_resources();
    assert_eq!(first.len(), 5);
    assert_eq!(second.len(), 5);
    for (a, b) in first.iter().zip(&second) {
        assert!(a.same_source(b));
        assert!(!a.same_instance(b));
        assert!(!a.compatible_scope(b));
        assert_eq!(a.scope().get::<Tag>().unwrap().value, &Tag(7));
        assert_eq!(b.scope().get::<Tag>().unwrap().value, &Tag(9));
    }
    assert!(first[3].compatible_scope(&first[1]));
    assert!(second[3].compatible_scope(&second[1]));
    assert!(!first[3].compatible_scope(&second[1]));
    assert_eq!(ResourceRef::new(document, 0).loadable_resources().len(), 10);
}

#[test]
fn dependency_queries_and_activation_use_the_same_scope_as_the_model() {
    let document = fixture(0);
    let first = ResourceRef::new(document.clone(), 1).loadable_resources();
    let second = ResourceRef::new(document, 2).loadable_resources();
    let named = AssetBundle::find_with_nodes(second[0].document.clone()).0;
    let bundle = AssetBundle::from_source(second[0].clone(), &named);
    let skeleton = bundle.skeleton.as_ref().unwrap();
    assert!(skeleton.same_instance(&second[1]));
    assert!(!skeleton.same_instance(&first[1]));
    let image = bundle.textures[0].texture_images().remove(0);
    assert!(image.same_instance(&second[2]));
    let loaded = |sources: &[ResourceRef]| {
        sources
            .iter()
            .enumerate()
            .map(|(index, source)| LoadedResource {
                id: index as u64,
                source: source.clone(),
                enabled: true,
            })
            .collect::<Vec<_>>()
    };
    let foreign = bundle.loaded_from(&loaded(&first));
    assert!(foreign.skeleton.is_none());
    assert!(foreign.textures[0].same_source(&ResourceRef::white_texture()));
    let local = bundle.loaded_from(&loaded(&second));
    assert!(local.skeleton.is_some());
    assert!(local.textures[0].same_instance(&image));
}

#[test]
fn refreshing_paths_relocates_indices_without_losing_the_reference_scope() {
    let old = fixture(0);
    let old_sources = ResourceRef::new(old, 2).loadable_resources();
    let new = fixture(3);
    for source in old_sources {
        let replacement = source.remap(new.clone()).unwrap();
        assert_eq!(replacement.node, source.node + 3);
        assert!(replacement.same_origin(&source));
        assert_eq!(replacement.scope().get::<Tag>().unwrap().value, &Tag(9));
        assert_eq!(replacement.bytes().unwrap(), source.bytes().unwrap());
    }
}

#[test]
fn refreshing_rebuilds_transparent_layers_instead_of_following_stale_payload_edges() {
    let old = fixture(0);
    let encoded = ResourceRef::new(old.clone(), 2)
        .loadable_resources()
        .remove(0);
    let payload = encoded.scope_source(11);
    let mut bare = (*old).clone();
    bare.nodes[5] = bare.nodes[11].clone();
    // A detail child must not be mistaken for the removed payload link.
    let detail = bare.nodes.len();
    let mut node = bare.nodes[11].clone();
    node.kind = Kind::Block;
    node.metadata.insert(Tag(99));
    bare.nodes.push(node);
    bare.nodes[5].children = vec![detail];
    let bare = Arc::new(bare);
    for source in [encoded, payload] {
        let unwrapped = source.remap(bare.clone()).unwrap();
        assert_eq!(unwrapped.node, 5);
        assert_eq!(unwrapped.context.last(), Some(&5));
        assert_eq!(unwrapped.scope().get::<Tag>().unwrap().value, &Tag(9));
        let rewrapped = unwrapped.remap(old.clone()).unwrap();
        assert_eq!(rewrapped.node, 11);
        assert_eq!(rewrapped.context.last(), Some(&11));
        assert!(rewrapped.same_origin(&source));
    }
}
