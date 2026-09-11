use super::*;

fn block(kind: u32, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [kind, count, (payload.len() + HEADER_SIZE) as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

fn bone(id: i32, parent: i32, child: i32, sibling: i32) -> Vec<u8> {
    let mut data = vec![0x6d; BONE_RECORD_SIZE];
    for (i, value) in [id, parent, child, sibling].into_iter().enumerate() {
        data[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (i, value) in [
        2.0f32, 3.0, 4.0, -0.0, 0.0, 0.0, 0.0, 2.0, 10.0, 20.0, 30.0, 7.0,
    ]
    .into_iter()
    .enumerate()
    {
        data[16 + i * 4..20 + i * 4].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    data[64..68].copy_from_slice(&0x4321_ffffu32.to_le_bytes());
    data[68..72].copy_from_slice(&0x7654_1234u32.to_le_bytes());
    block(BONE, 1, &data)
}

fn skeleton() -> Vec<u8> {
    let roots = block(ROOT_INDICES, 1, &0u32.to_le_bytes());
    let unknown_metadata = block(0x2200, u32::MAX, &[0xf1, 0xf2]);
    let mut root = block(
        SKELETON,
        5,
        &[
            roots,
            bone(77, -1, 1, -1),
            unknown_metadata,
            bone(12, 0, -1, 2),
            bone(99, 0, -1, -1),
        ]
        .concat(),
    );
    root.extend_from_slice(&[0xe1, 0xe2]);
    root
}

#[test]
fn hierarchy_keeps_order_ids_unknown_bytes_and_fourth_components() {
    let source = skeleton();
    let parsed = Fskl::parse(&source).unwrap();
    assert_eq!(parsed.root_indices(), [0]);
    assert_eq!(
        parsed.bones().map(|b| b.node_id).collect::<Vec<_>>(),
        [77, 12, 99]
    );
    assert_eq!(parsed.nodes.len(), 3);
    assert_eq!(parsed.blocks.len(), 5);
    assert_eq!(parsed.blocks[2].payload(), [0xf1, 0xf2]);
    let first = parsed.bones().next().unwrap();
    assert_eq!(first.first_child_index, 1);
    assert_eq!(first.transform.scale[3].to_bits(), 0x8000_0000);
    assert_eq!(first.transform.rotation, [0.0, 0.0, 0.0, 2.0]);
    assert_eq!(first.transform.translation, [10.0, 20.0, 30.0, 7.0]);
    assert_eq!(first.unknown_40, 0x4321_ffff);
    assert_eq!(first.motion_tag, 0x7654_1234);
    assert_eq!(first.unknown_48, [0x6d; 184]);
    assert_eq!(parsed.as_bytes(), source);
    parsed.validate_hierarchy().unwrap();
}

#[test]
fn translation_edit_changes_only_the_selected_node_field() {
    let source = skeleton();
    let parsed = Fskl::parse(&source).unwrap();
    let second = parsed.bones().nth(1).unwrap();
    let offset = second.block.offset() + HEADER_SIZE + 48;
    let values = [-0.0, f32::from_bits(0x7fc0_3141), 3.5, 9.0];
    let changed = parsed.with_translation(1, values).unwrap();
    assert_eq!(&source[..offset], &changed[..offset]);
    assert_eq!(&source[offset + 16..], &changed[offset + 16..]);
    let reparsed = Fskl::parse(&changed).unwrap();
    assert_eq!(
        reparsed
            .bones()
            .nth(1)
            .unwrap()
            .transform
            .translation
            .map(f32::to_bits),
        values.map(f32::to_bits)
    );
    assert!(parsed.with_translation(3, values).is_err());
}

#[test]
fn third_node_variant_keeps_its_root_ordinal_and_common_transform() {
    let mut third = bone(212, -1, -1, -1);
    third[..4].copy_from_slice(&BONE_3.to_le_bytes());
    third.extend_from_slice(&[0xde, 0xad]);
    let size = third.len() as u32;
    third[8..12].copy_from_slice(&size.to_le_bytes());
    let roots = block(
        ROOT_INDICES,
        2,
        &[0u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
    );
    let source = block(
        SKELETON,
        3,
        &[roots, bone(0, -1, -1, -1), third.clone()].concat(),
    );
    let parsed = Fskl::parse(&source).unwrap();
    assert_eq!(parsed.root_indices(), [0, 1]);
    assert_eq!(parsed.nodes.len(), 2);
    let NodeEntry::Bone(node) = &parsed.nodes[1] else {
        panic!("type 3 node should expose its common transform");
    };
    assert_eq!(node.block.header.kind, BONE_3);
    assert_eq!(node.block.as_bytes(), third);
    assert_eq!(node.node_id, 212);
    assert_eq!(node.parent_index, -1);
    assert_eq!(node.transform.scale[3].to_bits(), 0x8000_0000);
    assert_eq!(node.transform.translation, [10.0, 20.0, 30.0, 7.0]);
    assert_eq!(node.unknown_48, [0x6d; 184]);
    assert_eq!(node.trailing, [0xde, 0xad]);
    assert_eq!(parsed.as_bytes(), source);
    parsed.validate_hierarchy().unwrap();
}

#[test]
fn truncated_bones_and_bad_roots_are_detected() {
    let source = skeleton();
    let size = Block::parse(&source).unwrap().header.size as usize;
    for length in 0..size {
        assert!(Fskl::parse(&source[..length]).is_err());
    }
    for kind in [BONE, BONE_HD, BONE_3] {
        let short = block(SKELETON, 1, &block(kind, 1, &[0; 255]));
        assert_eq!(Fskl::parse(&short).unwrap_err().offset, 24);
    }
    let bad_root = block(
        SKELETON,
        2,
        &[
            block(ROOT_INDICES, 1, &2u32.to_le_bytes()),
            bone(10, -1, -1, -1),
        ]
        .concat(),
    );
    let parsed = Fskl::parse(&bad_root).unwrap();
    assert_eq!(parsed.validate_hierarchy().unwrap_err().offset, 24);
}

#[test]
fn invalid_links_remain_inspectable_and_cycles_do_not_recurse_forever() {
    for bad in [
        bone(0, -1, 4, -1),
        bone(0, -2, -1, -1),
        bone(0, -1, 0, -1),
        bone(0, -1, -1, 0),
    ] {
        let source = block(SKELETON, 1, &bad);
        let parsed = Fskl::parse(&source).unwrap();
        assert!(parsed.validate_hierarchy().is_err());
        assert_eq!(parsed.as_bytes(), source);
    }
    let source = block(
        SKELETON,
        2,
        &[bone(1, -1, 1, -1), bone(2, 0, -1, 0)].concat(),
    );
    assert!(Fskl::parse(&source).unwrap().validate_hierarchy().is_err());
}

#[test]
fn external_fskl_sample() {
    let Some(path) = std::env::var_os("MHF_RESOURCE_FSKL_SAMPLE") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let parsed = Fskl::parse(&bytes).unwrap();
    assert_eq!(parsed.as_bytes(), bytes);
    assert!(parsed.bones().next().is_some());
    parsed.validate_hierarchy().unwrap();
}
