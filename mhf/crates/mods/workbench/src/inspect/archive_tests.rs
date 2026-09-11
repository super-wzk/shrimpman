use super::*;

fn named_aliases(count: usize, payload: &[u8]) -> Vec<u8> {
    let names: Vec<_> = (0..count)
        .map(|index| format!("member-{index}.bin\0"))
        .collect();
    let names_size: usize = names.iter().map(String::len).sum();
    let names_offset = 24 + 20 * count;
    let payload_offset = names_offset + names_size;
    let mut bytes = b"mha\x01".to_vec();
    for word in [24, count, names_offset, names_size] {
        bytes.extend_from_slice(&(word as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 4]);
    let mut name_offset = 0;
    for (index, name) in names.iter().enumerate() {
        for word in [
            name_offset,
            payload_offset,
            payload.len(),
            payload.len(),
            index,
        ] {
            bytes.extend_from_slice(&(word as u32).to_le_bytes());
        }
        name_offset += name.len();
    }
    for name in names {
        bytes.extend_from_slice(name.as_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn named_directories_inspect_every_entry_without_fixed_count_or_node_limits() {
    let source: Arc<[u8]> = named_aliases(16_385, b"raw").into();
    let document = inspect("many.abn", source.clone());
    assert_eq!(document.nodes.len(), 16_386);
    assert_eq!(document.nodes[0].children.len(), 16_385);
    assert_eq!(document.buffers.len(), 1);
    assert!(Arc::ptr_eq(&document.buffers[0], &source));
    assert!(
        document
            .nodes
            .iter()
            .all(|node| node.error.is_none() && !node.deferred)
    );
    for &member in &document.nodes[0].children {
        assert_eq!(document.bytes(member), Some(&b"raw"[..]));
    }
    let repeated = expand(&document, 0).unwrap();
    assert_eq!(repeated.nodes.len(), document.nodes.len());
    assert_eq!(repeated.buffers.len(), document.buffers.len());
}

#[test]
fn nested_named_members_are_inspected_and_bad_payloads_keep_source_errors() {
    let nested = named_aliases(2, b"JKR\x1a\0");
    let document = inspect("nested.abn", named_aliases(1, &nested).into());
    let selected = document.nodes[0].children[0];
    assert_eq!(document.nodes[selected].kind, Kind::Mha);
    assert_eq!(document.nodes[selected].children.len(), 2);
    assert_eq!(document.buffers.len(), 1);
    for &bad in &document.nodes[selected].children {
        assert!(!document.nodes[bad].deferred);
        assert_eq!(document.nodes[bad].kind, Kind::Jkr);
        assert!(document.nodes[bad].error.is_some());
        assert_eq!(document.bytes(bad), Some(&b"JKR\x1a\0"[..]));
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; fully inspects three original named directories"]
fn original_large_named_directories_keep_every_resource_and_complete_bundle() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    for (name, count, models, texture_kind) in [
        ("wf500.abn", 400, 400, Kind::Txb),
        ("f01_wst.abn", 798, 798, Kind::Png),
        ("f00_body.abn", 738, 736, Kind::Png),
    ] {
        let source: Arc<[u8]> = std::fs::read(root.join("dat/extend").join(name))
            .unwrap()
            .into();
        let archive = MhaArchive::parse(&source, source.len()).unwrap();
        let document = Arc::new(inspect(name, source.clone()));
        assert_eq!(document.nodes[0].children.len(), count, "{name}");
        assert!(Arc::ptr_eq(&document.buffers[0], &source));
        let errors: Vec<_> = document
            .nodes
            .iter()
            .filter_map(|node| node.error.as_ref().map(|error| (&node.name, error)))
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        for (entry, &node) in archive.entries.iter().zip(&document.nodes[0].children) {
            assert_eq!(
                document.bytes(node).unwrap(),
                entry.entry.payload(&source).unwrap()
            );
            assert_ne!(
                document.nodes[node].kind,
                Kind::Unknown,
                "{name}: {}",
                document.nodes[node].name
            );
        }
        let bundles = crate::preview::AssetBundle::find_with_nodes(document.clone()).0;
        let kinds = |kind| {
            document
                .nodes
                .iter()
                .filter(|node| node.kind == kind)
                .count()
        };
        eprintln!(
            "{name}: members={count}, nodes={}, FMOD={}, FSKL={}, TXB={}, PNG={}, bundles={}",
            document.nodes.len(),
            kinds(Kind::Fmod),
            kinds(Kind::Fskl),
            kinds(Kind::Txb),
            kinds(Kind::Png),
            bundles.len()
        );
        // This checks structural pairing. Equipment-specific shared texture
        // slots are validated by the native preflight, not invented here.
        assert_eq!(bundles.len(), models, "{name}");
        assert_eq!(kinds(Kind::Fmod), models, "{name}");
        assert_eq!(kinds(Kind::Fskl), models, "{name}");
        assert_eq!(kinds(texture_kind), models, "{name}");
        assert!(
            bundles
                .iter()
                .all(|bundle| bundle.textures.len() == 1
                    && bundle.textures[0].kind() == texture_kind)
        );
        let repeated = expand(&document, 0).unwrap();
        assert_eq!(repeated.nodes.len(), document.nodes.len());
        assert_eq!(repeated.buffers.len(), document.buffers.len());
    }
}
