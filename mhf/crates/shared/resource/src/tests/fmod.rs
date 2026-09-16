use super::*;

fn block(kind: u32, count: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [kind, count, (payload.len() + HEADER_SIZE) as u32] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(payload);
    bytes
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn geometry_file() -> Vec<u8> {
    let positions = block(
        POSITIONS,
        3,
        &words(&[
            0x8000_0000,
            0x7fc0_1234,
            1.0f32.to_bits(),
            2.0f32.to_bits(),
            3.0f32.to_bits(),
            4.0f32.to_bits(),
            5.0f32.to_bits(),
            6.0f32.to_bits(),
            7.0f32.to_bits(),
        ]),
    );
    let normals = block(NORMALS, 3, &words(&[2.0f32.to_bits(); 9]));
    let uvs = block(UVS, 3, &words(&[3.0f32.to_bits(); 6]));
    let colors = block(COLORS, 3, &words(&[255.0f32.to_bits(); 12]));
    let weights = block(
        WEIGHTS,
        3,
        &words(&[
            2,
            31,
            70.0f32.to_bits(),
            2,
            30.0f32.to_bits(),
            1,
            70001,
            100.0f32.to_bits(),
            0,
        ]),
    );
    let strip = block(STRIPS_B, 1, &words(&[0x8000_0003, 2, 1, 0]));
    let face = block(FACE, 1, &strip);
    let unknown = block(0xfeed_0101, 0xffff_ffff, &[0x99, 0xaa, 0xbb]);
    let mut object_payload = [positions, normals, uvs, colors, weights, face, unknown].concat();
    object_payload.extend_from_slice(&[0xab, 0xcd]);
    let object = block(OBJECT, 7, &object_payload);
    let main = block(MAIN, 1, &object);
    let mut root = block(FILE, 1, &main);
    root.extend_from_slice(&[0xfe, 0xdc, 0xba]);
    root
}

#[test]
fn decodes_file_values_without_render_conversion() {
    let source = geometry_file();
    let file = Fmod::parse(&source).unwrap();
    assert_eq!(file.as_bytes(), source);
    assert_eq!(file.trailing, [0xfe, 0xdc, 0xba]);
    let object = file.objects().next().unwrap();
    assert_eq!(object.trailing, [0xab, 0xcd]);
    let vertices = object.positions().unwrap();
    assert_eq!(vertices.values[0][0].to_bits(), 0x8000_0000);
    assert_eq!(vertices.values[0][1].to_bits(), 0x7fc0_1234);
    assert_eq!(object.normals().unwrap().values[0], [2.0; 3]);
    assert_eq!(object.uvs().unwrap().values[0], [3.0; 2]);
    assert_eq!(object.colors().unwrap().values[0], [255.0; 4]);
    let weights = object.weights().unwrap();
    assert_eq!(weights.vertices[0].influences[0].bone_index, 31);
    assert_eq!(weights.vertices[0].influences[0].weight, 70.0);
    assert_eq!(weights.vertices[0].influences[1].bone_index, 2);
    assert_eq!(weights.vertices[1].influences[0].bone_index, 70001);
    assert!(weights.vertices[2].influences.is_empty());
    let (group, strip) = object.faces().unwrap().strips().next().unwrap();
    assert_eq!(group.block.header.kind, STRIPS_B);
    assert!(strip.reversed());
    assert_eq!(strip.indices, [2, 1, 0]);
    let Component::Unknown(unknown) = object.components.last().unwrap() else {
        panic!()
    };
    assert_eq!(unknown.header.count, u32::MAX);
    assert_eq!(unknown.payload(), [0x99, 0xaa, 0xbb]);
    object.validate_geometry().unwrap();
}

#[test]
fn vertex_edit_preserves_every_other_byte() {
    let source = geometry_file();
    let parsed = Fmod::parse(&source).unwrap();
    let positions = parsed.objects().next().unwrap().positions().unwrap();
    let offset = positions.block.offset() + HEADER_SIZE + 12;
    let edit = [f32::from_bits(0x7f80_0123), -0.0, -9.25];
    let changed = parsed.with_vertex_position(0, 1, edit).unwrap();
    assert_eq!(&source[..offset], &changed[..offset]);
    assert_eq!(&source[offset + 12..], &changed[offset + 12..]);
    let reparsed = Fmod::parse(&changed).unwrap();
    let actual = reparsed
        .objects()
        .next()
        .unwrap()
        .positions()
        .unwrap()
        .values[1];
    assert_eq!(actual.map(f32::to_bits), edit.map(f32::to_bits));
    assert!(parsed.with_vertex_position(0, 3, edit).is_err());
    assert!(parsed.with_vertex_position(1, 0, edit).is_err());
}

#[test]
fn materials_and_textures_have_individual_record_headers() {
    let mut material = vec![0x5a; 256];
    material[..16].copy_from_slice(&words(&[1.0f32.to_bits(); 4]));
    material[0x2c..0x30].copy_from_slice(&0.25f32.to_le_bytes());
    material[0x34..0x38].copy_from_slice(&2u32.to_le_bytes());
    material.extend_from_slice(&words(&[1, 0]));
    material.extend_from_slice(&[0xd1, 0xd2]);
    let material_record = block(1, 1, &material);
    let unknown_record = block(0x72, 3, &[0x9a]);
    let materials = block(MATERIALS, 2, &[material_record, unknown_record].concat());
    let mut texture = vec![0xa5; 256];
    texture[..12].copy_from_slice(&words(&[8, 256, 128]));
    let texture_record = block(0, 1, &texture);
    let textures = block(TEXTURES, 1, &texture_record);
    let source = block(FILE, 2, &[materials, textures].concat());
    let file = Fmod::parse(&source).unwrap();
    let material = file.materials().next().unwrap();
    assert_eq!(material.color_00, [1.0; 4]);
    assert_eq!(material.color_20[3], 0.25);
    assert_eq!(material.texture_indices, [1, 0]);
    assert_eq!(material.unknown_38, [0x5a; 200]);
    assert_eq!(material.trailing, [0xd1, 0xd2]);
    let texture = file.textures().next().unwrap();
    assert_eq!(
        (texture.image_id, texture.width, texture.height),
        (8, 256, 128)
    );
    assert_eq!(texture.unknown_0c, [0xa5; 244]);
    assert_eq!(file.as_bytes(), source);
}

#[test]
fn all_truncations_of_declared_file_fail() {
    let source = geometry_file();
    let size = Block::parse(&source).unwrap().header.size as usize;
    for length in 0..size {
        assert!(Fmod::parse(&source[..length]).is_err(), "length {length}");
    }
    assert!(Fmod::parse(&source[..size]).is_ok());
}

#[test]
fn corrupt_counts_sizes_and_indices_report_file_offsets() {
    assert!(Block::parse(&words(&[1, 0, 0])).is_err());
    assert!(Block::parse(&words(&[1, 0, u32::MAX])).is_err());
    assert!(Block::parse_at(&[], usize::MAX).is_err());
    let bad_children = block(FILE, u32::MAX, &[]);
    assert_eq!(Fmod::parse(&bad_children).unwrap_err().offset, 4);

    for (kind, stride) in [
        (POSITIONS, 12),
        (NORMALS, 12),
        (UVS, 8),
        (COLORS, 16),
        (ATTRIBUTE_12, 16),
    ] {
        let component = block(kind, 2, &vec![0; stride]);
        let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &component)));
        assert_eq!(Fmod::parse(&source).unwrap_err().offset, 40);
    }
    for component in [
        block(WEIGHTS, 1, &words(&[u32::MAX])),
        block(
            FACE,
            1,
            &block(STRIPS_A, 1, &words(&[0x1000_0003, 0, 1, 2])),
        ),
    ] {
        let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &component)));
        assert!(Fmod::parse(&source).is_err());
    }

    let mut source = geometry_file();
    let parsed = Fmod::parse(&source).unwrap();
    let strip_offset = parsed
        .objects()
        .next()
        .unwrap()
        .faces()
        .unwrap()
        .strips()
        .next()
        .unwrap()
        .1
        .offset;
    source[strip_offset + 4..strip_offset + 8].copy_from_slice(&70001u32.to_le_bytes());
    let parsed = Fmod::parse(&source).unwrap();
    let error = parsed
        .objects()
        .next()
        .unwrap()
        .validate_geometry()
        .unwrap_err();
    assert_eq!(error.offset, strip_offset + 4);
}

#[test]
fn word_groups_preserve_order_empty_groups_raw_words_and_trailing_bytes() {
    let mut payload = words(&[2, u32::MAX, 0x8000_0000, 0, 1, 0x7fc0_1234]);
    payload.extend_from_slice(&[0xde, 0xad]);
    let groups = block(WORD_GROUPS, 3, &payload);
    let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &groups)));
    let parsed = Fmod::parse(&source).unwrap();
    let object = parsed.objects().next().unwrap();
    let Component::WordGroups(value) = &object.components[0] else {
        panic!("0x0E word groups should be decoded");
    };
    assert_eq!(value.groups.len(), 3);
    assert_eq!(value.groups[0].offset, 48);
    assert_eq!(value.groups[0].words, [u32::MAX, 0x8000_0000]);
    assert_eq!(value.groups[1].offset, 60);
    assert!(value.groups[1].words.is_empty());
    assert_eq!(value.groups[2].offset, 64);
    assert_eq!(value.groups[2].words, [0x7fc0_1234]);
    assert_eq!(value.trailing, [0xde, 0xad]);
    assert_eq!(value.block.as_bytes(), groups);
    assert_eq!(parsed.as_bytes(), source);
}

#[test]
fn word_group_counts_and_payload_boundaries_are_checked() {
    for (count, payload, offset) in [
        (u32::MAX, vec![], 40),
        (1, words(&[u32::MAX]), 48),
        (2, words(&[1, 17]), 56),
        (1, [words(&[1]), vec![0xaa, 0xbb, 0xcc]].concat(), 48),
    ] {
        let group = block(WORD_GROUPS, count, &payload);
        let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &group)));
        assert_eq!(Fmod::parse(&source).unwrap_err().offset, offset);
    }
}

#[test]
fn rendering_words_preserve_unknown_bits_and_trailing_bytes() {
    // The 18-word layout occurs in both em077_b.pac and em077_b-hd.pac.
    // Values outside the observed range must also survive without conversion.
    let values = [
        0x0001_0000,
        1,
        0,
        0,
        0,
        2,
        0,
        1,
        0,
        1,
        0,
        2,
        3,
        u32::MAX,
        0x8000_0000,
        0x7fc0_1234,
        0x0102_0304,
        0,
    ];
    let mut payload = words(&values);
    payload.extend_from_slice(&[0xab, 0xcd, 0xef]);
    let rendering = block(RENDERING, 1, &payload);
    let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &rendering)));
    let parsed = Fmod::parse(&source).unwrap();
    let object = parsed.objects().next().unwrap();
    let Component::Rendering(value) = &object.components[0] else {
        panic!("rendering configuration should be decoded");
    };
    assert_eq!(value.words, values);
    assert_eq!(value.trailing, [0xab, 0xcd, 0xef]);
    assert_eq!(value.block.offset(), 36);
    assert_eq!(value.block.as_bytes(), rendering);
    assert_eq!(parsed.as_bytes(), source);
}

#[test]
fn rendering_block_initialization_appends_one_fixed_child() {
    let object = block(OBJECT, 0, &[0xab, 0xcd]);
    let source = block(FILE, 1, &block(MAIN, 1, &object));
    let parsed = Fmod::parse(&source).unwrap();
    let mut words = [0; RENDERING_WORDS];
    words[0] = RENDERING_VERSION;
    let changed = parsed.with_rendering_block(0, words).unwrap();
    assert_eq!(changed.len(), source.len() + RENDERING_SIZE);
    let reparsed = Fmod::parse(&changed).unwrap();
    let object = reparsed.objects().next().unwrap();
    assert_eq!(object.block.header.count, 1);
    assert_eq!(
        object.block.header.size as usize,
        HEADER_SIZE + RENDERING_SIZE + 2
    );
    assert_eq!(object.trailing, [0xab, 0xcd]);
    let rendering = reparsed.rendering_block(0).unwrap().unwrap();
    assert_eq!(rendering.block.header.kind, RENDERING);
    assert_eq!(rendering.block.header.count, 1);
    assert_eq!(rendering.block.header.size, RENDERING_SIZE as u32);
    assert_eq!(rendering.words, words);
    assert_eq!(rendering.words[UV_TRANSFORM_WORD], 0);

    words[UV_TRANSFORM_WORD] = 1;
    let enabled = Fmod::parse(&source)
        .unwrap()
        .with_rendering_block(0, words)
        .unwrap();
    let enabled = Fmod::parse(&enabled).unwrap();
    let enabled = enabled.rendering_block(0).unwrap().unwrap();
    assert_eq!(enabled.block.header.kind, RENDERING);
    assert_eq!(enabled.words[UV_TRANSFORM_WORD], 1);
    assert!(reparsed.with_rendering_block(0, words).is_err());
    words[0] = 0;
    assert!(parsed.with_rendering_block(0, words).is_err());
}

#[test]
fn rendering_truncation_is_checked_and_other_record_counts_stay_unknown() {
    let rendering = block(RENDERING, 1, &[0; 71]);
    let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &rendering)));
    assert_eq!(Fmod::parse(&source).unwrap_err().offset, 48);

    let rendering = block(RENDERING, 2, &[0xab]);
    let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 1, &rendering)));
    let parsed = Fmod::parse(&source).unwrap();
    assert!(matches!(
        parsed.objects().next().unwrap().components[0],
        Component::Unknown(_)
    ));
    assert_eq!(parsed.as_bytes(), source);
}

#[test]
fn external_fmod_sample() {
    let Some(path) = std::env::var_os("MHF_RESOURCE_FMOD_SAMPLE") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let parsed = Fmod::parse(&bytes).unwrap();
    assert_eq!(parsed.as_bytes(), bytes);
    assert!(parsed.objects().next().is_some());
    for object in parsed.objects() {
        object.validate_geometry().unwrap();
    }
    let mut total_vertices = 0;
    for object in parsed.objects() {
        total_vertices += object.positions().map_or(0, |p| p.values.len());
    }
    assert!(total_vertices > 0);
}
