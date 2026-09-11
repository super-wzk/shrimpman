//! Optional validation against local, untracked decoded client resources.

use mhf_resource::motion::ObservedMotionDirectory;

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original mytra resource"]
fn mytra_standalone_clips_keep_empty_tracks_and_every_encoded_key() {
    use mhf_resource::{container::SimpleArchive, motion::Motion};
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mytra.bin")).unwrap();
    let archive = SimpleArchive::parse(&source, 100).unwrap();
    let mut clips = 0;
    let mut keys = 0;
    for entry in archive.entries.iter().skip(6) {
        let bytes = entry.payload(&source).unwrap();
        let motion = Motion::probe(bytes).unwrap();
        assert_eq!(motion.header.kind, 0x8000_0002);
        assert_eq!(motion.tracks.len(), if entry.index == 11 { 3 } else { 4 });
        assert!(motion.tracks[0].channels.is_empty());
        assert_eq!(motion.as_bytes(), bytes);
        for track in &motion.tracks {
            assert_eq!(
                track.as_bytes(),
                &bytes[track.offset..track.offset + track.as_bytes().len()]
            );
            for channel in &track.channels {
                let stride = channel.encoding().stride().unwrap();
                for index in 0..channel.native_key_count() as usize {
                    let key = channel.key(index).unwrap();
                    assert_eq!(
                        key.to_bytes(),
                        &channel.payload()[index * stride..(index + 1) * stride]
                    );
                    keys += 1;
                }
            }
        }
        clips += 1;
    }
    assert_eq!(clips, 6);
    assert!(keys > 0);
    eprintln!("audited {clips} mytra standalone clips and {keys} encoded keys");
}

#[test]
#[ignore = "requires MHF_MOTION_SAMPLE_DIR with decoded npc41 and em019 resources"]
fn decoded_client_motion_samples_keep_every_track_and_encoded_key() {
    let root = std::path::PathBuf::from(
        std::env::var_os("MHF_MOTION_SAMPLE_DIR").expect("set MHF_MOTION_SAMPLE_DIR"),
    );
    for (
        name,
        expected_size,
        expected_motions,
        expected_tracks,
        expected_channels,
        expected_keys,
    ) in [
        ("npc41-decoded.mot", 21_044, 1, 18, 153, 2_271),
        ("em019-chunk-2.bin", 122_108, 21, 504, 1_586, 11_973),
    ] {
        let bytes = std::fs::read(root.join(name)).unwrap();
        assert_eq!(bytes.len(), expected_size);
        let observed = ObservedMotionDirectory::probe_with_budget(&bytes, bytes.len() / 4).unwrap();
        assert_eq!(observed.record_count(), 3);
        let archive = observed.directory;
        let (mut motions, mut tracks, mut channels, mut keys) = (0, 0, 0, 0);
        for (group_index, group) in archive.groups.iter().enumerate() {
            for slot in 0..group.motion_offsets.len() {
                let Some(motion) = archive.motion(group_index, slot).unwrap() else {
                    continue;
                };
                motions += 1;
                tracks += motion.tracks.len();
                for (track_index, track) in motion.tracks.iter().enumerate() {
                    channels += track.channels.len();
                    for (channel_index, channel) in track.channels.iter().enumerate() {
                        let stride = channel
                            .encoding()
                            .stride()
                            .expect("verified native encoding");
                        for key_index in 0..usize::from(channel.native_key_count()) {
                            let key = channel.key(key_index).unwrap();
                            assert_eq!(
                                key.to_bytes(),
                                channel.payload()[key_index * stride..key_index * stride + stride]
                            );
                            keys += 1;
                        }
                        if channel.native_key_count() > 0 {
                            assert_eq!(
                                motion
                                    .with_key(
                                        track_index,
                                        channel_index,
                                        0,
                                        channel.key(0).unwrap()
                                    )
                                    .unwrap(),
                                motion.as_bytes()
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(
            (motions, tracks, channels, keys),
            (
                expected_motions,
                expected_tracks,
                expected_channels,
                expected_keys
            )
        );
        assert_eq!(archive.as_bytes(), bytes);
    }
}

#[test]
#[ignore = "requires MHF_MOTION_SAMPLE_DIR with decoded ordinary and HD em171 motions"]
fn ordinary_and_hd_em171_have_complete_but_empty_motion_tables() {
    let root = std::path::PathBuf::from(
        std::env::var_os("MHF_MOTION_SAMPLE_DIR").expect("set MHF_MOTION_SAMPLE_DIR"),
    );
    let ordinary = std::fs::read(root.join("em171-decoded.mot")).unwrap();
    let hd = std::fs::read(root.join("em171-hd-decoded.mot")).unwrap();
    assert_eq!(ordinary, hd);
    for bytes in [&ordinary, &hd] {
        assert_eq!(bytes.len(), 2456);
        let observed = ObservedMotionDirectory::probe_with_budget(bytes, 600).unwrap();
        assert_eq!(observed.record_count(), 7);
        for (index, group) in observed.directory.groups.iter().enumerate() {
            assert_eq!(group.offsets_offset as usize, 56 + 400 * index);
            assert_eq!(group.motion_offsets.len(), if index < 6 { 100 } else { 0 });
            assert!(group.motion_offsets.iter().all(Option::is_none));
        }
        assert_eq!(observed.directory.as_bytes(), bytes);
    }
}
