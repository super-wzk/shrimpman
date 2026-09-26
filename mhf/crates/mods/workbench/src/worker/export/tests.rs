use super::*;
use crate::inspect::Node;
fn export_document() -> Document {
    let node = |name: &str, kind, buffer, range| Node {
        name: name.into(),
        kind,
        buffer,
        range,
        fields: Vec::new(),
        metadata: Default::default(),
        children: Vec::new(),
        action: None,
        deferred: false,
        error: None,
    };
    Document {
        root: 0,
        buffers: vec![Arc::from([1, 2, 3, 4]), Arc::from([5, 6, 7, 8, 9, 10])],
        nodes: vec![
            node("Z:\\游戏\\dat\\em001.hd.pac", Kind::Ecd, 0, 0..4),
            node("解码结果.old", Kind::Fmod, 1, 0..6),
            node("图像.old", Kind::Png, 1, 1..5),
            node("image.png", Kind::Block, 1, 2..4),
            node("motion.mot", Kind::Track, 1, 1..3),
            node("data.dds", Kind::Unknown, 1, 0..6),
        ],
    }
}

#[test]
fn export_names_distinguish_original_files_resources_and_fragments() {
    let document = export_document();
    assert_eq!(
        Export::from_node(&document, 0).unwrap().name,
        "em001.hd.pac"
    );
    assert_eq!(
        Export::from_node(&document, 1).unwrap().name,
        "解码结果.fmod"
    );
    assert_eq!(Export::from_node(&document, 2).unwrap().name, "图像.png");
    for (index, range) in [(3, 2..4), (4, 1..3), (5, 0..6)] {
        let export = Export::from_node(&document, index).unwrap();
        assert!(
            export
                .name
                .ends_with(&format!("-b1-0x{:08X}.bin", range.start))
        );
        assert_eq!(&export.bytes[export.range], &document.buffers[1][range]);
    }
    let mut invalid = document;
    invalid.nodes[2].error = Some("truncated resource".into());
    assert!(
        Export::from_node(&invalid, 2)
            .unwrap()
            .name
            .ends_with(".bin")
    );
    invalid.nodes[2].range = 0..99;
    assert!(Export::from_node(&invalid, 2).is_err());
}

#[test]
fn export_filenames_keep_unicode_and_extensions_with_safe_collision_names() {
    assert_eq!(
        safe_filename("Z:\\游戏\\怪物 001.hd.dds"),
        "怪物 001.hd.dds"
    );
    assert_eq!(safe_filename("../../raw:node?.bin"), "raw_node_.bin");
    for name in ["CON", "NUL.dds", "com1.png", "LPT².bin", "CONOUT$"] {
        assert!(safe_filename(name).starts_with('_'));
    }
    assert_eq!(safe_filename(".."), "resource");
    assert_eq!(numbered_filename("怪物 001.hd.dds", 0), "怪物 001.hd.dds");
    assert_eq!(
        numbered_filename("怪物 001.hd.dds", 1),
        "怪物 001.hd-001.dds"
    );
    let long = format!("{}.dds", "😀".repeat(150));
    for index in [999, 1000, usize::MAX] {
        let bounded = numbered_filename(&long, index);
        assert!(bounded.encode_utf16().count() <= 255);
        assert!(bounded.ends_with(&format!("-{index:03}.dds")));
    }
}

#[test]
fn exports_preserve_selected_bytes_and_never_overwrite_existing_files() {
    let directory =
        std::env::temp_dir().join(format!("mhf-workbench-export-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let export = Export {
        name: "../raw:node.bin".into(),
        bytes: Arc::from([0, 0xff, 0x80, 4]),
        range: 1..3,
    };
    let first = export_bytes(&directory, &export).unwrap();
    let second = export_bytes(&directory, &export).unwrap();
    assert_ne!(first, second);
    assert_eq!(first.file_name().unwrap(), "raw_node.bin");
    assert_eq!(second.file_name().unwrap(), "raw_node-001.bin");
    assert_eq!(first.parent(), Some(directory.as_path()));
    assert_eq!(fs::read(&first).unwrap(), [0xff, 0x80]);
    assert_eq!(fs::read(&second).unwrap(), [0xff, 0x80]);
    let unicode = export_bytes(
        &directory,
        &Export {
            name: "怪物 001.dds".into(),
            bytes: export.bytes.clone(),
            range: export.range.clone(),
        },
    )
    .unwrap();
    assert_eq!(unicode.file_name().unwrap(), "怪物 001.dds");
    assert_eq!(fs::read(unicode).unwrap(), [0xff, 0x80]);
    assert!(
        export_bytes(
            &directory,
            &Export {
                range: 0..5,
                ..export
            }
        )
        .is_err()
    );
    fs::remove_dir_all(directory).unwrap();
}
