//! Optional validation of three original stage object packages.

use mhf_resource::{
    container::open_layers,
    stage::{ObjectPackage, ObjectWordTable},
    txb::Txb,
};

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads three original stage object packages"]
fn object_word_tables_retain_original_bytes_and_match_texture_counts() {
    let root = std::path::PathBuf::from(
        std::env::var_os("MHF_RESOURCE_GAME_ROOT").expect("set MHF_RESOURCE_GAME_ROOT"),
    );
    for name in ["nso0042.pac", "nso0043.pac", "nso0309.pac"] {
        let path = root.join("dat/stage").join(name);
        let source =
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let decoded = open_layers(&source, 64 * 1024 * 1024, 8)
            .unwrap_or_else(|error| panic!("{name} package envelope: {error}"));
        let package = ObjectPackage::probe(decoded.payload(), 256)
            .unwrap_or_else(|error| panic!("{name} object package: {error}"));

        let member = package
            .member(14)
            .unwrap_or_else(|| panic!("{name} has no kind-14 member"));
        let decoded_words = open_layers(member.bytes, 64 * 1024 * 1024, 8)
            .unwrap_or_else(|error| panic!("{name} kind-14 envelope: {error}"));
        let bytes = decoded_words.payload();
        let table = ObjectWordTable::parse(bytes)
            .unwrap_or_else(|error| panic!("{name} object word table: {error}"));
        assert_eq!(table.unknown_00, 0, "{name}");
        assert_eq!(bytes.len(), 8 + 4 * table.count as usize, "{name}");
        assert!(table.trailing.is_empty(), "{name}");
        assert_eq!(table.as_bytes(), bytes, "{name}");
        assert_eq!(table.as_bytes().as_ptr(), bytes.as_ptr(), "{name}");
        assert_eq!(table.values, &bytes[8..], "{name}");
        assert_eq!(table.values.as_ptr(), bytes[8..].as_ptr(), "{name}");
        let values: Vec<_> = table.values().collect();
        assert_eq!(values.len(), table.count as usize, "{name}");
        let encoded: Vec<_> = values.into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(encoded.as_slice(), table.values, "{name}");

        let textures = package
            .member(3)
            .unwrap_or_else(|| panic!("{name} has no kind-3 texture member"));
        let decoded_textures = open_layers(textures.bytes, 64 * 1024 * 1024, 8)
            .unwrap_or_else(|error| panic!("{name} texture envelope: {error}"));
        let bank = Txb::parse(decoded_textures.payload(), 4096)
            .unwrap_or_else(|error| panic!("{name} texture directory: {error}"));
        assert_eq!(table.count, bank.archive.count, "{name}");
        assert_eq!(table.values().count(), bank.textures.len(), "{name}");
    }
}
