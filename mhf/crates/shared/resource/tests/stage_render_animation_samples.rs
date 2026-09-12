use mhf_resource::{
    container::{SimpleArchive, open_layers},
    stage::{
        RenderTables,
        render_tables::{AnimationChannel, AnimationCommand, KeyframeHeader, PointLightSelection},
    },
};

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original HD render animation tables"]
fn original_hd_commands_resolve_to_borrowed_contiguous_animation_records() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut commands = 0;
    let mut references = 0;
    for entry in std::fs::read_dir(root.join("dat/stage-hd")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "pac") {
            continue;
        }
        let source = std::fs::read(&path).unwrap();
        let decoded = open_layers(&source, 512 * 1024 * 1024, 16).unwrap();
        let archive = SimpleArchive::parse(decoded.payload(), 16).unwrap();
        let Some(member) = archive.entries.get(2).filter(|entry| entry.size != 0) else {
            continue;
        };
        let bytes = member.payload(decoded.payload()).unwrap();
        if bytes.starts_with(&1u16.to_le_bytes()) {
            continue;
        }
        let file = RenderTables::parse(bytes).unwrap();
        for table in &file.tables {
            match table.animation_channel() {
                Some(AnimationChannel::PointLight) => {
                    for record in table.records() {
                        PointLightSelection::parse(record).unwrap();
                    }
                }
                Some(_) => {
                    for record in table.records() {
                        KeyframeHeader::parse(record).unwrap();
                    }
                }
                None if table.count_offset == 26 => {
                    for record in table.records() {
                        let command = AnimationCommand::parse(record).unwrap();
                        commands += 1;
                        let Some(channel) = AnimationChannel::from_id(command.channel) else {
                            continue;
                        };
                        if let Some(target) = file
                            .animation_records(channel, command.animation_id)
                            .unwrap()
                        {
                            assert_eq!(target.records.as_ptr(), bytes[target.offset..].as_ptr());
                            assert!(target.records.chunks_exact(target.record_size).all(
                                |record| {
                                    u32::from(u16::from_le_bytes(record[..2].try_into().unwrap()))
                                        == command.animation_id
                                }
                            ));
                            references += 1;
                        }
                    }
                }
                None => {}
            }
        }
    }
    assert!(commands > 0 && references > 0);
    eprintln!(
        "Render animation commands: {commands}; resolved original record ranges: {references}"
    );
}
