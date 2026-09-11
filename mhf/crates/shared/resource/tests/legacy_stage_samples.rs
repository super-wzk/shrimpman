use mhf_resource::{
    container::{SimpleArchive, StageArchive, open_layers},
    stage::{LegacyLighting, LegacyRenderTables},
};

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads two original legacy stage files"]
fn actual_legacy_scene_members_preserve_the_native_consumed_extents() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    for (name, color, far_value) in [
        ("st001.pac", 0xff2f_2320, 60000f32),
        ("st017.pac", 0xff1f_1d18, 55000f32),
    ] {
        let source = std::fs::read(root.join("dat/stage").join(name)).unwrap();
        let opened = open_layers(&source, 512 * 1024 * 1024, 16).unwrap();
        let outer = SimpleArchive::parse(opened.payload(), 65_536).unwrap();
        let model_opened = open_layers(outer.payload(0).unwrap(), 512 * 1024 * 1024, 16).unwrap();
        let model_package = SimpleArchive::parse(model_opened.payload(), 65_536).unwrap();
        let bytes = model_package.payload(2).unwrap();
        let file = LegacyLighting::parse(bytes).unwrap();
        assert_eq!(file.version, 2);
        assert_eq!(file.unknown_01, 0);
        assert_eq!(file.color_02, color);
        assert_eq!(file.value_06_bits, 0f32.to_bits());
        assert_eq!(file.value_0a_bits, far_value.to_bits());
        assert!(file.extension.is_some());
        assert!(file.tables.is_none());
        assert!(file.trailing.is_empty());
        assert_eq!(file.as_bytes().len(), 170);
        assert_eq!(file.as_bytes().as_ptr(), bytes.as_ptr());
        if name == "st001.pac" {
            let stage_opened =
                open_layers(outer.payload(31).unwrap(), 512 * 1024 * 1024, 16).unwrap();
            let stage = StageArchive::probe(stage_opened.payload(), 65_536).unwrap();
            let bytes = stage.entries[2]
                .entry
                .payload(stage_opened.payload())
                .unwrap();
            let tables = LegacyRenderTables::parse(bytes).unwrap();
            assert_eq!((tables.version, tables.control), (1, 8));
            assert_eq!(
                tables
                    .tables
                    .iter()
                    .map(|table| table.count)
                    .collect::<Vec<_>>(),
                [3, 3, 8, 1, 3, 0]
            );
            assert_eq!(tables.as_bytes().len(), 452);
            assert_eq!(tables.as_bytes().as_ptr(), bytes.as_ptr());
            assert!(tables.trailing.is_empty());
            for table in &tables.tables {
                assert_eq!(table.records.as_ptr(), bytes[table.offset..].as_ptr());
            }
        }
    }
}
