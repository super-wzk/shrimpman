use super::*;

fn encoded_camera() -> Vec<u8> {
    let mut bytes = vec![0; EventCamera::HEADER_SIZE];
    bytes[8..12].copy_from_slice(&(16.0_f32 / 9.0).to_bits().to_le_bytes());
    bytes[12..16].copy_from_slice(&3_u32.to_le_bytes());
    for (array, stride) in EventCamera::STRIDES.into_iter().enumerate() {
        bytes.resize(bytes.len().next_multiple_of(16), 0xa5);
        let offset = bytes.len() as u32;
        bytes[16 + 4 * array..20 + 4 * array].copy_from_slice(&offset.to_le_bytes());
        for frame in 0..3 {
            for component in 0..stride / 4 {
                let bits = match (array, frame, component) {
                    (0, 0, 0) => 0x7fc0_1234_u32,
                    (1, 0, 1) => 0x8000_0000_u32,
                    _ => ((100 * array + 10 * frame + component) as f32).to_bits(),
                };
                bytes.extend(bits.to_le_bytes());
            }
        }
    }
    bytes.resize(bytes.len().next_multiple_of(16), 0xa5);
    bytes
}

fn assert_camera_expansion(document: &Document, node_index: usize) {
    let node = &document.nodes[node_index];
    let raw = document.bytes(node_index).unwrap();
    let camera = EventCamera::probe(raw).unwrap();
    assert!(node.deferred);
    let expanded = expand(document, node_index).unwrap();
    assert!(!expanded.nodes[node_index].deferred);
    assert!(document.nodes[node_index].deferred);
    assert_eq!(expanded.bytes(node_index).unwrap(), raw);
    assert_eq!(expanded.buffers.len(), document.buffers.len());
    for (before, after) in document.buffers.iter().zip(&expanded.buffers) {
        assert!(Arc::ptr_eq(before, after));
    }
    let arrays = &expanded.nodes[node_index].children;
    assert_eq!(arrays.len(), 4);
    for (array, &child_index) in arrays.iter().enumerate() {
        let child = &expanded.nodes[child_index];
        let offset = node.range.start + camera.array_offsets[array] as usize;
        let stride = EventCamera::STRIDES[array];
        assert_eq!(child.buffer, node.buffer);
        assert_eq!(child.range, offset..offset + camera.arrays[array].len());
        assert_eq!(expanded.bytes(child_index).unwrap(), camera.arrays[array]);
        assert_eq!(child.fields.len(), camera.frame_count as usize);
        assert!(child.error.is_none());
        for (frame, field) in child.fields.iter().enumerate() {
            assert_eq!(
                (field.offset, field.size),
                (offset + frame * stride, stride)
            );
            let encoded = &document.buffers[node.buffer][field.offset..field.offset + field.size];
            assert_eq!(
                encoded,
                &camera.arrays[array][frame * stride..(frame + 1) * stride]
            );
            for word in encoded.as_chunks::<4>().0 {
                let bits = u32::from_le_bytes(*word);
                assert!(
                    field
                        .value
                        .to_ascii_lowercase()
                        .contains(&format!("{bits:08x}"))
                );
            }
        }
    }
}

#[test]
fn camera_expansion_keeps_source_identity_float_bits_and_absolute_array_offsets() {
    let camera = encoded_camera();
    let mut directory = Vec::new();
    for word in [1_u32, 12, camera.len() as u32] {
        directory.extend(word.to_le_bytes());
    }
    directory.extend_from_slice(&camera);
    let mut encoded = b"JKR\x1a\x08\x01\0\0".to_vec();
    encoded.extend(16_u32.to_le_bytes());
    encoded.extend((directory.len() as u32).to_le_bytes());
    encoded.extend_from_slice(&directory);
    let source: Arc<[u8]> = encoded.into();
    let document = inspect("renamed.bin", source.clone());
    assert!(Arc::ptr_eq(&document.buffers[0], &source));
    assert!(document.nodes.iter().all(|node| node.error.is_none()));
    let camera_node = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::EventCamera)
        .unwrap();
    assert_eq!(document.nodes[camera_node].range, 12..12 + camera.len());
    assert_ne!(document.nodes[camera_node].buffer, 0);
    assert_eq!(document.bytes(camera_node).unwrap(), camera);
    assert_camera_expansion(&document, camera_node);
}

#[test]
fn utf16_line_break_placeholders_keep_bom_and_content_in_the_original_buffer() {
    for source in [
        [0xff, 0xfe, 0x0d, 0x00, 0x0a, 0x00],
        [0xfe, 0xff, 0x00, 0x0d, 0x00, 0x0a],
    ] {
        let source: Arc<[u8]> = Arc::from(source);
        let document = inspect("renamed.mot", source.clone());
        let node = &document.nodes[document.root];
        assert_eq!(node.kind, Kind::Text);
        assert!(node.error.is_none());
        assert!(node.children.is_empty());
        assert!(Arc::ptr_eq(&document.buffers[node.buffer], &source));
        assert_eq!(document.bytes(document.root).unwrap(), source.as_ref());
        assert_eq!(node.fields.len(), 2);
        assert_eq!((node.fields[0].offset, node.fields[0].size), (0, 2));
        assert_eq!((node.fields[1].offset, node.fields[1].size), (2, 4));
        assert_eq!(node.fields[1].value, format!("{:?}", "\r\n"));
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; audits original motion directory and mytra.bin"]
fn original_motion_directory_and_mytra_resolve_and_expand_every_animation() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut paths: Vec<_> = std::fs::read_dir(root.join("dat/motion"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    let mut files = 0;
    let mut cameras = 0;
    let mut text_files = 0;
    for path in paths {
        let source: Arc<[u8]> = std::fs::read(&path).unwrap().into();
        let document = inspect(&path.to_string_lossy(), source.clone());
        assert!(Arc::ptr_eq(&document.buffers[0], &source));
        for (index, node) in document.nodes.iter().enumerate() {
            assert!(
                node.error.is_none() && node.kind != Kind::Unknown,
                "{} {} {:?}: {:?}",
                path.display(),
                node.name,
                node.kind,
                node.error
            );
            if node.kind == Kind::EventCamera {
                assert_camera_expansion(&document, index);
                cameras += 1;
            }
            if node.kind == Kind::Text {
                assert_eq!(
                    document.bytes(index).unwrap(),
                    [0xff, 0xfe, 0x0d, 0, 0x0a, 0]
                );
                text_files += 1;
            }
        }
        files += 1;
    }
    assert_eq!((files, cameras, text_files), (117, 77, 2));

    let source: Arc<[u8]> = std::fs::read(root.join("dat/mytra.bin")).unwrap().into();
    let directory = SimpleArchive::parse(&source, 100).unwrap();
    let document = inspect("renamed.bin", source.clone());
    assert!(
        document
            .nodes
            .iter()
            .all(|node| node.error.is_none() && node.kind != Kind::Unknown)
    );
    assert!(Arc::ptr_eq(&document.buffers[0], &source));
    let children = &document.nodes[document.root].children;
    assert_eq!(children.len(), directory.entries.len());
    let mut motions = 0;
    for (entry, &index) in directory.entries.iter().zip(children).skip(6) {
        let node = &document.nodes[index];
        assert_eq!(node.kind, Kind::Motion);
        assert_eq!(node.range.start, entry.offset as usize);
        let bytes = entry.payload(&source).unwrap();
        assert_eq!(document.bytes(index).unwrap(), bytes);
        let motion = Motion::probe(bytes).unwrap();
        let expanded = expand(&document, index).unwrap();
        assert!(Arc::ptr_eq(&expanded.buffers[node.buffer], &source));
        assert_eq!(expanded.bytes(index).unwrap(), bytes);
        assert!(!expanded.nodes[index].deferred);
        assert!(expanded.nodes.iter().all(|node| node.error.is_none()));
        let tracks = &expanded.nodes[index].children;
        assert_eq!(tracks.len(), motion.tracks.len());
        for (&track_index, track) in tracks.iter().zip(&motion.tracks) {
            let track_node = &expanded.nodes[track_index];
            assert_eq!(track_node.kind, Kind::Track);
            assert_eq!(track_node.buffer, node.buffer);
            assert_eq!(track_node.range.start, node.range.start + track.offset);
            assert_eq!(expanded.bytes(track_index).unwrap(), track.as_bytes());
            assert_eq!(track_node.children.len(), track.channels.len());
            for (&channel_index, channel) in track_node.children.iter().zip(&track.channels) {
                let channel_node = &expanded.nodes[channel_index];
                assert_eq!(channel_node.kind, Kind::Channel);
                assert_eq!(channel_node.buffer, node.buffer);
                assert_eq!(channel_node.range.start, node.range.start + channel.offset);
                assert_eq!(expanded.bytes(channel_index).unwrap(), channel.as_bytes());
            }
        }
        motions += 1;
    }
    assert_eq!(motions, 6);
    eprintln!(
        "audited {files} motion files, {cameras} expanded cameras, {text_files} UTF-16 files and {motions} complete mytra clips"
    );
}
