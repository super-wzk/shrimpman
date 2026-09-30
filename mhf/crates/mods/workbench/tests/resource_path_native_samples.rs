//! Read-only comparison of canonical navigation with the shared format parsers.
use mhf_resource::{
    ResourcePath,
    action_definition::{AttackDirectory, AttackReference, Definition},
    container::open_layers,
    emd::Emd,
    motion::ObservedMotionDirectory,
    sdt::{Sdt, TableKind},
};
use mhf_workbench::inspect::{self, Document, resource_path::Location};
use std::{ops::Range, path::Path, sync::Arc};

fn check(
    mut document: Document,
    root: &Path,
    text: &str,
    range: Range<usize>,
    expected: &[u8],
) -> Document {
    let path: ResourcePath = text.parse().unwrap();
    for _ in 0..100 {
        match document.locate_resource(root, &path) {
            Location::Expand(node) => document = inspect::expand(&document, node).unwrap(),
            Location::Resolved {
                node,
                context,
                field,
            } => {
                assert!(field.is_none());
                assert_eq!(document.nodes[node].range, range, "{text}");
                assert_eq!(document.bytes(node).unwrap(), expected, "{text}");
                let address = document.resource_address(root, &context, None).unwrap();
                assert!(address.exact);
                assert_eq!(address.path, path);
                return document;
            }
            Location::Missing => panic!("cannot locate {text}"),
        }
    }
    panic!("navigation did not converge for {text}");
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original DAT/SDT/EMD/MOT only"]
fn original_resource_paths_match_shared_parser_spans() {
    let root =
        std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap()).join("dat");
    let read = |name: &str| -> Arc<[u8]> { std::fs::read(root.join(name)).unwrap().into() };

    let source = read("mhfdat.bin");
    let decoded = open_layers(&source, usize::MAX, 16).unwrap();
    let definition = Definition::parse(decoded.payload(), 0, 4, 0).unwrap();
    assert!(!definition.events.is_empty());
    let index = definition.events.len() - 1;
    let range = definition.event_span(index).unwrap();
    check(
        inspect::inspect(root.join("mhfdat.bin"), source.clone()),
        &root,
        &definition
            .event_path("mhfdat.bin", index)
            .unwrap()
            .to_string(),
        range.clone(),
        &decoded.payload()[range],
    );

    let source = read("mhfsdt.bin");
    let decoded = open_layers(&source, usize::MAX, 16).unwrap();
    let file = Sdt::probe(decoded.payload()).unwrap();
    let attack = AttackReference {
        category: 0,
        subtype: None,
        record: 23,
    };
    let path = AttackDirectory::from_sdt("mhfsdt.bin", &file)
        .unwrap()
        .resolve(attack)
        .unwrap()
        .unwrap();
    assert_eq!(path.to_string(), "mhfsdt.bin#0/attacks/23");
    let entry = file
        .entries()
        .iter()
        .find(|entry| entry.record_count != 0)
        .unwrap();
    let table = file.table(entry, TableKind::Attack).unwrap().unwrap();
    let record = table.record(table.count - 1).unwrap();
    check(
        inspect::inspect(root.join("mhfsdt.bin"), source.clone()),
        &root,
        &format!("mhfsdt.bin#{}/attacks/{}", entry.index, table.count - 1),
        record.offset..record.offset + record.as_bytes().len(),
        record.as_bytes(),
    );

    let source = read("mhfemd.bin");
    let decoded = open_layers(&source, usize::MAX, 16).unwrap();
    let table = Emd::parse(decoded.payload())
        .unwrap()
        .root_table(17)
        .unwrap()
        .unwrap();
    let index = 42.min(table.count - 1);
    let (offset, record) = table.record(index).unwrap();
    check(
        inspect::inspect(root.join("mhfemd.bin"), source.clone()),
        &root,
        &format!("mhfemd.bin#17/{index}"),
        offset..offset + record.len(),
        record,
    );

    let source = read("motion/w04.mot");
    let decoded = open_layers(&source, usize::MAX, 16).unwrap();
    let file = ObservedMotionDirectory::probe_with_budget(decoded.payload(), usize::MAX).unwrap();
    let (group, slot) = file
        .directory
        .groups
        .iter()
        .enumerate()
        .find_map(|(group, record)| {
            record
                .motion_offsets
                .iter()
                .position(Option::is_some)
                .map(|slot| (group, slot))
        })
        .unwrap();
    let motion = file.directory.motion(group, slot).unwrap().unwrap();
    check(
        inspect::inspect(root.join("motion/w04.mot"), source.clone()),
        &root,
        &format!("motion/w04.mot#{group}/{slot}"),
        motion.offset..motion.offset + motion.as_bytes().len(),
        motion.as_bytes(),
    );
}
