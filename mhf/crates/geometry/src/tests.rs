use crate::{
    fmod::{self, Geometry, Strip},
    mesh::{self, MATERIAL, VARIANT, Vertices},
};

fn positions(count: u32) -> Vec<u8> {
    (0..count)
        .flat_map(|index| [index as f32, (index % 7) as f32, 0.0])
        .flat_map(f32::to_le_bytes)
        .collect()
}

fn block(kind: u32, count: u32, body: Vec<u8>) -> Vec<u8> {
    [kind, count, 12 + body.len() as u32]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .chain(body)
        .collect()
}

fn resource(vertex_count: u32, strips: &[(bool, Vec<u32>)]) -> Vec<u8> {
    let mut words = Vec::new();
    for (reversed, indices) in strips {
        words.push(indices.len() as u32 | if *reversed { 0x8000_0000 } else { 0 });
        words.extend(indices);
    }
    let faces = block(
        5,
        1,
        block(
            0x30000,
            strips.len() as u32,
            words.into_iter().flat_map(u32::to_le_bytes).collect(),
        ),
    );
    let vertices = block(0x70000, vertex_count, positions(vertex_count));
    let object = block(4, 2, [faces, vertices].concat());
    block(1, 1, block(2, 1, object))
}

fn compile(geometry: &Geometry, flags: u32) -> mesh::Mesh {
    let bytes = positions(geometry.vertex_count);
    mesh::compile(
        &geometry.encode(flags).unwrap(),
        geometry.strips.len() as u32,
        flags,
        &Vertices {
            bytes: &bytes,
            count: geometry.vertex_count,
            format: 1,
        },
    )
    .unwrap()
}

/// Independent topology oracle: compare oriented, nondegenerate triangles,
/// rather than duplicating the native strip-stitching algorithm in a test.
fn triangles(indices: &[u32], reversed: bool) -> Vec<[u32; 3]> {
    indices
        .windows(3)
        .enumerate()
        .filter_map(|(index, triangle)| {
            let mut triangle: [u32; 3] = triangle.try_into().unwrap();
            if triangle[0] == triangle[1]
                || triangle[1] == triangle[2]
                || triangle[0] == triangle[2]
            {
                return None;
            }
            if (index % 2 == 1) ^ reversed {
                triangle.swap(0, 1);
            }
            let smallest = triangle
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| *v)
                .unwrap()
                .0;
            triangle.rotate_left(smallest);
            Some(triangle)
        })
        .collect()
}

#[test]
fn original_assets_keep_topology_and_winding() {
    for left_count in 3..7 {
        for right_count in 3..7 {
            for left_reversed in [false, true] {
                for right_reversed in [false, true] {
                    let left = (0..left_count).collect::<Vec<_>>();
                    let right = (left_count..left_count + right_count).collect::<Vec<_>>();
                    let file = resource(
                        left_count + right_count,
                        &[
                            (left_reversed, left.clone()),
                            (right_reversed, right.clone()),
                        ],
                    );
                    let geometry = fmod::read(&file, 0).unwrap();
                    let mesh = compile(&geometry, 0);
                    assert_eq!(
                        triangles(&mesh.indices, false),
                        [
                            triangles(&left, left_reversed),
                            triangles(&right, right_reversed)
                        ]
                        .concat()
                    );
                    assert_eq!(mesh.descriptors, [mesh.indices.len() as u32]);
                    assert_eq!(mesh.bounds.len(), 1);
                }
            }
        }
    }
}

#[test]
fn indices_cross_the_word_boundary_without_aliasing() {
    let file = resource(70_001, &[(false, vec![65_535, 65_536, 70_000])]);
    let geometry = fmod::read(&file, 0).unwrap();
    let mesh = compile(&geometry, 0);
    assert_eq!(mesh.indices, [65_535, 65_536, 70_000]);
    assert_eq!(mesh.bounds[0].center[0], (65_535.0 + 70_000.0) * 0.5);
}

#[test]
fn a_single_strip_exceeds_both_packed_and_word_lengths() {
    let indices = (0..70_000).collect::<Vec<_>>();
    let geometry = fmod::read(&resource(70_000, &[(true, indices.clone())]), 0).unwrap();
    let mesh = compile(&geometry, 0);
    assert_eq!(mesh.descriptors, [70_001]);
    assert_eq!(triangles(&mesh.indices, false), triangles(&indices, true));
}

#[test]
fn joined_short_strips_keep_a_wide_batch_length() {
    let count = 16_000;
    let strips = (0..count)
        .map(|i| Strip {
            reversed: false,
            material: 0,
            variant: 0,
            indices: vec![3 * i, 3 * i + 1, 3 * i + 2],
        })
        .collect();
    let mesh = compile(
        &Geometry {
            vertex_count: count * 3,
            strips,
        },
        0,
    );
    assert!(mesh.descriptors[0] > u16::MAX as u32);
    assert_eq!(mesh.descriptors[0] as usize, mesh.indices.len());
    assert_eq!(triangles(&mesh.indices, false).len(), count as usize);
}

#[test]
fn material_and_skinning_variants_survive_many_batches() {
    let count = 5_000;
    let strips = (0..count)
        .map(|i| Strip {
            reversed: i % 2 != 0,
            material: i % 3,
            variant: i % 2,
            indices: vec![0, 1, 2],
        })
        .collect();
    let mesh = compile(
        &Geometry {
            vertex_count: 3,
            strips,
        },
        MATERIAL | VARIANT,
    );
    assert_eq!(mesh.bounds.len(), count as usize);
    assert_eq!(mesh.descriptors.len(), count as usize * 3);
    for (i, batch) in mesh.descriptors.as_chunks::<3>().0.iter().enumerate() {
        assert_eq!(
            *batch,
            [if i % 2 == 0 { 3 } else { 4 }, i as u32 % 3, i as u32 % 2]
        );
    }
}

#[test]
fn skinned_bounds_use_high_vertex_indices() {
    let count = 65_539;
    let mut bytes = Vec::new();
    for i in 0..count {
        bytes.extend([i as f32, 0.0, 0.0].into_iter().flat_map(f32::to_le_bytes));
        bytes.extend([0, 0, 0, 17, 0, 0, 0, 255]);
    }
    let flags = 0x100;
    let mesh = mesh::compile(
        &[3, 65_536, 65_537, 65_538],
        1,
        flags,
        &Vertices {
            bytes: &bytes,
            count,
            format: 0x101,
        },
    )
    .unwrap();
    assert_eq!(mesh.bounds[0].bone, 17);
    assert_eq!(mesh.bounds[0].center, [65_537.0, 0.0, 0.0]);
    assert_eq!(mesh.bounds[0].radius, 3.0);
}

#[test]
fn malformed_geometry_is_rejected_before_native_conversion() {
    assert!(fmod::read(&resource(3, &[(false, vec![0, 1, 3])]), 0).is_err());
    let mut file = resource(3, &[(false, vec![0, 1, 2])]);
    file.pop();
    assert!(fmod::read(&file, 0).is_err());
    let mut file = resource(3, &[(false, vec![0, 1, 2])]);
    file[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(fmod::read(&file, 0).is_err());
    assert!(mesh::byte_size(i32::MAX as usize, 4).is_err());
    let bytes = positions(3);
    let vertices = Vertices {
        bytes: &bytes,
        count: 3,
        format: 1,
    };
    assert!(mesh::compile(&[3, 0, 1, 3], 1, 0, &vertices).is_err());
    assert!(mesh::compile(&[70_000, 0, 1, 2], 1, 0, &vertices).is_err());
}

#[test]
#[ignore = "set MHF_GEOMETRY_TEST_CLIENT to the verified ZZ HD DLL"]
fn instruction_edits_match_the_actual_client() {
    let path = std::env::var("MHF_GEOMETRY_TEST_CLIENT").expect("MHF_GEOMETRY_TEST_CLIENT");
    let bytes = std::fs::read(path).unwrap();
    let word = |offset| fmod::word(&bytes, offset).unwrap() as usize;
    let half = |offset| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
    let pe = word(0x3c);
    let section_start = pe + 24 + half(pe + 20);
    let sections: Vec<_> = (0..half(pe + 6))
        .map(|i| {
            let offset = section_start + i * 40;
            (word(offset + 12), word(offset + 16), word(offset + 20))
        })
        .collect();
    let mut previous_end = 0;
    for patch in crate::patches::PATCHES {
        assert!(patch.rva >= previous_end, "overlapping patches");
        assert_eq!(patch.original.len(), patch.replacement.len());
        let &(rva, _, offset) = sections
            .iter()
            .find(|&&(rva, size, _)| (rva..rva + size).contains(&patch.rva))
            .unwrap();
        let offset = offset + patch.rva - rva;
        assert_eq!(
            &bytes[offset..offset + patch.original.len()],
            patch.original,
            "RVA {:#x}",
            patch.rva
        );
        previous_end = patch.rva + patch.original.len();
    }
}
