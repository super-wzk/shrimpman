//! Optional audit of original client packages; no game resources are committed.

use mhf_resource::{
    container::{SimpleArchive, open_layers},
    effect_archive::{EffectArchive, EffectResource},
};

#[test]
#[ignore = "requires MHF_CLIENT_DATA_DIR with emmodel and emmodel-hd packages"]
fn client_effect_members_parse_and_every_known_record_reencodes_exactly() {
    let root = std::path::PathBuf::from(
        std::env::var_os("MHF_CLIENT_DATA_DIR").expect("set MHF_CLIENT_DATA_DIR"),
    );
    let mut packages = 0;
    let mut banks = 0;
    let mut event_tables = 0;
    for directory in ["emmodel", "emmodel-hd"] {
        for entry in std::fs::read_dir(root.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) != Some("pac") {
                continue;
            }
            let source = std::fs::read(&path).unwrap();
            let decoded = open_layers(&source, 128 * 1024 * 1024, 8).unwrap();
            let outer = SimpleArchive::parse(decoded.payload(), 100).unwrap();
            let Some(entry) = outer.entries.get(5).filter(|entry| entry.size != 0) else {
                continue;
            };
            let effect_bytes = open_layers(
                entry.payload(decoded.payload()).unwrap(),
                128 * 1024 * 1024,
                8,
            )
            .unwrap();
            let archive = EffectArchive::parse(effect_bytes.payload(), 1024)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(archive.as_bytes(), effect_bytes.payload());
            packages += 1;
            for member in &archive.members {
                match member.resource().unwrap_or_else(|error| {
                    panic!("{} effect {}: {error}", path.display(), member.index)
                }) {
                    EffectResource::Bank(bank) => {
                        let tables: [Vec<u8>; 6] = [
                            bank.emitters
                                .iter()
                                .flat_map(|value| value.to_bytes())
                                .collect(),
                            bank.vector_keys
                                .iter()
                                .flat_map(|value| value.to_bytes())
                                .collect(),
                            bank.color_keys
                                .iter()
                                .flat_map(|value| value.to_bytes())
                                .collect(),
                            bank.integer_keys
                                .iter()
                                .flat_map(|value| value.to_bytes())
                                .collect(),
                            bank.definitions_56
                                .iter()
                                .flat_map(|value| value.to_bytes())
                                .collect(),
                            bank.definitions_140
                                .iter()
                                .flat_map(|value| value.to_bytes())
                                .collect(),
                        ];
                        for (table, (bytes, stride)) in
                            tables.iter().zip([112, 24, 16, 16, 56, 140]).enumerate()
                        {
                            assert_eq!(bytes.len(), usize::from(bank.counts[table]) * stride);
                            assert_eq!(
                                bytes,
                                &member.as_bytes()
                                    [bank.table_offsets[table]..bank.table_offsets[table + 1]]
                            );
                        }
                        assert_eq!(bank.as_bytes(), member.as_bytes());
                        // This local corpus has six-table v4 banks. Future
                        // layouts remain raw instead of silently losing tails.
                        assert_eq!(bank.version, 4);
                        assert_eq!(&bank.counts[6..], &[0, 0, 0]);
                        assert!(bank.trailing_bytes.is_empty());
                        banks += 1;
                    }
                    EffectResource::MotionEvents(table) => {
                        assert_eq!(table.as_bytes(), member.as_bytes());
                        for event in &table.events {
                            assert_eq!(
                                event.to_bytes(),
                                member.as_bytes()[event.offset..event.offset + 32]
                            );
                        }
                        assert!(table.trailing_bytes.is_empty());
                        event_tables += 1;
                    }
                    EffectResource::Unknown(_) => panic!(
                        "{} has unknown effect kind {}",
                        path.display(),
                        member.reference.kind
                    ),
                }
            }
        }
    }
    assert!(packages > 0 && banks > 0 && event_tables > 0);
    eprintln!(
        "audited {packages} packages, {banks} effect banks, {event_tables} motion-event tables"
    );
}
