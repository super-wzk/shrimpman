use super::*;
use crate::{
    field::Field,
    inspect::{Kind, Node, NodeAddress, expand, inspect},
    preview::ResourceRef,
};
use mhf_resource::PathSegment::Index;
use mhf_resource::binary::Reader;
use std::sync::Arc;

pub(crate) fn resolve(
    mut document: Document,
    root: &Path,
    text: &str,
) -> (Document, usize, Vec<usize>, Option<usize>) {
    let path = text.parse().unwrap();
    for _ in 0..100 {
        match document.locate_resource(root, &path) {
            Location::Resolved {
                node,
                context,
                field,
            } => return (document, node, context, field),
            Location::Expand(node) => {
                assert!(document.nodes[node].deferred);
                let expanded = expand(&document, node).unwrap();
                assert!(!expanded.nodes[node].deferred);
                document = expanded;
            }
            Location::Missing => panic!("cannot locate {text}"),
        }
    }
    panic!("navigation did not converge for {text}");
}

#[test]
fn references_keep_loading_coordinates_scope_and_physical_owners_separate() {
    #[derive(Debug, PartialEq, Eq)]
    struct Tag(u8);
    let node = |name: &str, kind, children| Node {
        native_id: None,
        material_slots: Vec::new(),
        name: name.into(),
        address: None,
        kind,
        children,
        buffer: 0,
        range: 0..8,
        fields: Vec::new(),
        metadata: Default::default(),
        action: None,
        deferred: false,
        error: None,
    };
    let mut document = Document {
        attack_directory: None,
        source: Path::new("dat/目录 %#.bin").into(),
        root: 0,
        buffers: vec![Arc::from([37, 0, 0, 0, 0, 0, 0, 0])],
        nodes: vec![
            node("arbitrary label", Kind::Archive, vec![1, 2]),
            node("owner", Kind::Archive, vec![3]),
            node("caller", Kind::StageResourceReference, vec![3]),
            node("model", Kind::Fmod, vec![]),
        ],
    };
    document.nodes[1].address = Some(NodeAddress {
        anchor: 0,
        segments: vec![Index(0)],
    });
    document.nodes[2].address = Some(NodeAddress {
        anchor: 0,
        segments: vec![Index(7)],
    });
    document.nodes[3].address = Some(NodeAddress {
        anchor: 1,
        segments: vec![Index(4)],
    });
    document.nodes[1].metadata.insert(Tag(1));
    document.nodes[2].metadata.insert(Tag(2));
    document.nodes[3].fields.push(Field::from_binary(
        "version",
        0,
        Reader::new(&document.buffers[0]).read::<u32>().unwrap(),
    ));
    let physical = crate::edit::node_key(&document, 3).unwrap();
    let document = Arc::new(document);
    let direct = ResourceRef::new(document.clone(), 3);
    let alias = ResourceRef::at_context(document.clone(), 2, vec![0, 2]);
    assert_eq!(
        direct
            .resource_address(Path::new("dat"), None)
            .unwrap()
            .path
            .to_string(),
        "目录 %25%23.bin#0/4"
    );
    assert_eq!(
        alias
            .resource_address(Path::new("dat"), None)
            .unwrap()
            .path
            .to_string(),
        "目录 %25%23.bin#7"
    );
    assert!(direct.same_source(&alias));
    assert!(!direct.same_instance(&alias));
    assert!(!direct.same_origin(&alias));
    assert_eq!(alias.scope().get::<Tag>().unwrap().value, &Tag(2));
    assert_eq!(crate::edit::locate(&document, &physical), Some(3));
    let path = "目录 %25%23.bin#7/version".parse().unwrap();
    assert_eq!(
        document.locate_resource(Path::new("dat"), &path),
        Location::Resolved {
            node: 3,
            context: vec![0, 2, 3],
            field: Some(0)
        }
    );
    let mut changed = (*document).clone();
    changed.nodes[0].children.reverse();
    changed.nodes[0].name = "另一个显示名称".into();
    let remapped = alias.remap_path(Arc::new(changed)).unwrap();
    assert_eq!(remapped.scope().get::<Tag>().unwrap().value, &Tag(2));
    assert_eq!(
        remapped
            .resource_address(Path::new("dat"), None)
            .unwrap()
            .path
            .to_string(),
        "目录 %25%23.bin#7"
    );
}

#[test]
fn encoding_headers_do_not_shadow_payload_fields_and_helpers_only_show_ancestors() {
    let mut bytes = b"JKR\x1a\x08\x01\0\0".to_vec();
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend([1_u8, 0, 0, 0, 0, 0, 0, 0]);
    let mut document = inspect("dat/encoded.bin", bytes.into());
    let payload = document.payload(document.root).unwrap();
    let binding = Reader::new(&document.buffers[document.nodes[payload].buffer])
        .read::<u32>()
        .unwrap();
    let buffer = document.nodes[payload].buffer;
    document.nodes[payload]
        .fields
        .push(Field::from_binary("version", buffer, binding));
    let path = "encoded.bin#version".parse().unwrap();
    let Location::Resolved {
        node,
        field: Some(field),
        ..
    } = document.locate_resource(Path::new("dat"), &path)
    else {
        panic!("payload version must resolve")
    };
    assert_eq!(node, payload);
    assert_eq!(document.nodes[node].fields[field].binding.buffer, 1);
    let outer = document.nodes[document.root]
        .fields
        .iter()
        .position(|field| field.key.as_deref() == Some("version"))
        .unwrap();
    let address = document
        .resource_address(Path::new("dat"), &[document.root], Some(outer))
        .unwrap();
    assert!(!address.exact);
    assert_eq!(address.path.to_string(), "encoded.bin");
    document.nodes[payload].address = None;
    let helper = document.nodes.len();
    document.nodes.push(Node {
        native_id: None,
        material_slots: Vec::new(),
        name: "仅显示用途".into(),
        address: None,
        kind: Kind::Block,
        buffer: 1,
        range: 0..0,
        fields: Vec::new(),
        metadata: Default::default(),
        children: Vec::new(),
        action: None,
        deferred: false,
        error: None,
    });
    document.nodes[payload].children.push(helper);
    let address = document
        .resource_address(Path::new("dat"), &[document.root, payload, helper], None)
        .unwrap();
    assert!(!address.exact);
    assert_eq!(address.path.to_string(), "encoded.bin");
    assert_eq!(
        document.locate_resource(Path::new("dat"), &"encoded.bin#made_up/0".parse().unwrap()),
        Location::Missing
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_sources_remain_real_file_identities_without_lossy_addresses() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let source = std::path::PathBuf::from(OsString::from_vec(b"dat/invalid-\xff.bin".to_vec()));
    let document = Arc::new(inspect(&source, Arc::from([])));
    assert_eq!(document.source, source);
    let reference = ResourceRef::new(document.clone(), document.root);
    assert!(reference.belongs_to(&source));
    assert!(reference.resource_address(Path::new("dat"), None).is_none());
}
