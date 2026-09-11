use mhf_resource::event_camera::EventCamera;

fn camera(count: usize) -> Vec<u8> {
    let mut bytes = vec![0; EventCamera::HEADER_SIZE];
    bytes[8..12].copy_from_slice(&1.333_f32.to_bits().to_le_bytes());
    bytes[12..16].copy_from_slice(&(count as u32).to_le_bytes());
    for (array, stride) in EventCamera::STRIDES.into_iter().enumerate() {
        bytes.resize(bytes.len().next_multiple_of(16), 0xa5);
        let offset = bytes.len() as u32;
        bytes[16 + array * 4..20 + array * 4].copy_from_slice(&offset.to_le_bytes());
        for frame in 0..count {
            for component in 0..stride / 4 {
                let bits = match (array, frame, component) {
                    (0, 0, 0) => 0x7fc0_1234,
                    (1, 0, 0) => 0x8000_0000,
                    _ => (array as f32 * 100.0 + frame as f32 * 10.0 + component as f32).to_bits(),
                };
                bytes.extend(bits.to_le_bytes());
            }
        }
    }
    bytes.resize(bytes.len().next_multiple_of(16), 0xa5);
    bytes
}

#[test]
fn frame_arrays_preserve_float_bits_padding_offsets_and_order() {
    let bytes = camera(3);
    let parsed = EventCamera::probe(&bytes).unwrap();
    assert_eq!(parsed.frame_count, 3);
    assert_eq!(parsed.array_offsets, [32, 48, 96, 112]);
    assert_eq!(parsed.arrays.map(<[u8]>::len), [12, 36, 12, 36]);
    assert_eq!(parsed.as_bytes(), bytes);
    for (index, array) in parsed.arrays.iter().enumerate() {
        let offset = parsed.array_offsets[index] as usize;
        assert_eq!(*array, &bytes[offset..offset + array.len()]);
        assert_eq!(array.as_ptr(), bytes[offset..].as_ptr());
    }
    let first = parsed.frame(0).unwrap();
    assert_eq!(first.field_of_view_bits, 0x7fc0_1234);
    assert_eq!(first.position_bits[0], 0x8000_0000);
    assert_eq!(
        parsed.frame(2).unwrap().target_bits.map(f32::from_bits),
        [320.0, 321.0, 322.0]
    );
    assert!(parsed.frame(3).is_err());
    assert!(parsed.frame(usize::MAX).is_err());
    assert_eq!(&parsed.as_bytes()[44..48], &[0xa5; 4]);
    assert_eq!(&parsed.as_bytes()[148..], &[0xa5; 12]);
}

#[test]
fn native_offsets_remain_independent_while_probe_requires_complete_observed_layout() {
    let mut bytes = camera(3);
    bytes[..4].copy_from_slice(&0xdead_beef_u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
    // Both vector arrays may refer to the same source, with scalar arrays in
    // reverse physical order. Parse the actual native offsets without copying.
    for (index, offset) in [96_u32, 48, 32, 48].into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&offset.to_le_bytes());
    }
    bytes.extend_from_slice(b"uninterpreted trailing bytes");
    let parsed = EventCamera::parse(&bytes).unwrap();
    assert_eq!(parsed.unknown_00, 0xdead_beef);
    assert_eq!(parsed.unknown_04, 0x1234_5678);
    assert_eq!(parsed.arrays[1].as_ptr(), parsed.arrays[3].as_ptr());
    assert_eq!(
        parsed.frame(1).unwrap().position_bits,
        parsed.frame(1).unwrap().target_bits
    );
    assert_eq!(parsed.as_bytes(), bytes);
    assert!(EventCamera::probe(&bytes).is_err());
}

#[test]
fn truncation_overflow_and_header_overlap_are_rejected_before_reading_frames() {
    let bytes = camera(3);
    // The final twelve bytes are padding, not an additional record.
    for length in 0..148 {
        assert!(
            EventCamera::parse(&bytes[..length]).is_err(),
            "length {length}"
        );
    }
    assert!(EventCamera::parse(&bytes[..148]).is_ok());
    for length in 148..bytes.len() {
        assert!(
            EventCamera::probe(&bytes[..length]).is_err(),
            "length {length}"
        );
    }
    for (offset, value) in [(12, u32::MAX), (16, 0), (20, u32::MAX), (24, 31), (28, 148)] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(EventCamera::parse(&invalid).is_err(), "header {offset}");
    }
    let empty = camera(0);
    assert!(EventCamera::parse(&empty).is_ok());
    assert!(EventCamera::probe(&empty).is_err());
    for value in [0.0_f32, -1.0, f32::NAN, f32::INFINITY] {
        let mut invalid = bytes.clone();
        invalid[8..12].copy_from_slice(&value.to_bits().to_le_bytes());
        assert!(EventCamera::parse(&invalid).is_ok());
        assert!(EventCamera::probe(&invalid).is_err());
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original event-camera packages"]
fn all_original_event_cameras_decode_every_frame_without_changing_source_bits() {
    use mhf_resource::container::{SimpleArchive, open_layers};
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut packages = 0;
    let mut cameras = 0;
    let mut frames = 0;
    for entry in std::fs::read_dir(root.join("dat/motion")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("bin") {
            continue;
        }
        let source = std::fs::read(&path).unwrap();
        let decoded = open_layers(&source, 128 * 1024 * 1024, 8).unwrap();
        let archive = SimpleArchive::parse(decoded.payload(), 1000).unwrap();
        packages += 1;
        for entry in &archive.entries {
            let bytes = entry.payload(decoded.payload()).unwrap();
            let camera = EventCamera::probe(bytes).unwrap_or_else(|error| {
                panic!("{} entry {}: {error}", path.display(), entry.index)
            });
            assert_eq!(camera.as_bytes(), bytes);
            for index in 0..camera.frame_count as usize {
                let frame = camera.frame(index).unwrap();
                let arrays = [
                    frame.field_of_view_bits.to_le_bytes().to_vec(),
                    frame
                        .position_bits
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect(),
                    frame.roll_bits.to_le_bytes().to_vec(),
                    frame
                        .target_bits
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect(),
                ];
                for (array, encoded) in arrays.iter().enumerate() {
                    let offset =
                        camera.array_offsets[array] as usize + index * EventCamera::STRIDES[array];
                    assert_eq!(encoded, &bytes[offset..offset + encoded.len()]);
                }
                frames += 1;
            }
            cameras += 1;
        }
    }
    assert_eq!((packages, cameras, frames), (32, 77, 28_771));
    eprintln!("audited {packages} event-camera packages, {cameras} cameras, {frames} frames");
}
