use super::*;

fn file(width: u32, height: u32, mips: u32, four_cc: &[u8; 4], payload: usize) -> Vec<u8> {
    let mut words = [0u32; 31];
    words[0] = 124;
    words[2] = height;
    words[3] = width;
    words[6] = mips;
    words[7..18].fill(0xfeed_1234);
    words[18] = 32;
    words[19] = DDPF_FOURCC;
    words[20] = u32::from_le_bytes(*four_cc);
    words[28] = 0xbeef_abcd;
    words[29] = 0xaaaa_5555;
    words[30] = 0x7654_3210;
    let mut bytes = MAGIC.to_vec();
    bytes.extend(words.into_iter().flat_map(u32::to_le_bytes));
    bytes.resize(bytes.len() + payload, 0x5a);
    bytes
}

fn set_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn dxt_mips_have_block_minimum_and_original_headers_are_preserved() {
    let mut bytes = file(7, 5, 3, b"DXT1", 32 + 8 + 8);
    bytes.extend_from_slice(&[0xf1, 0xf2]);
    let dds = Dds::parse(&bytes).unwrap();
    assert_eq!(dds.encoding(), Encoding::Dxt1);
    assert_eq!(dds.header.reserved_1, [0xfeed_1234; 11]);
    assert_eq!(dds.header.caps_3, 0xbeef_abcd);
    assert_eq!(dds.header.caps_4, 0xaaaa_5555);
    assert_eq!(dds.header.reserved_2, 0x7654_3210);
    let surfaces = dds.surfaces(3).unwrap();
    assert_eq!(
        surfaces
            .iter()
            .map(|s| (s.width, s.height, s.bytes.len()))
            .collect::<Vec<_>>(),
        [(7, 5, 32), (3, 2, 8), (1, 1, 8)]
    );
    assert_eq!(surfaces[0].offset, 128);
    assert_eq!(surfaces[2].offset, 168);
    assert_eq!(dds.as_bytes(), bytes);
    assert_eq!(&dds.pixel_data()[48..], [0xf1, 0xf2]);
    assert!(dds.surfaces(2).is_err());
}

#[test]
fn masked_pixels_preserve_pitch_and_channel_masks() {
    let mut bytes = file(3, 2, 1, b"\0\0\0\0", 32);
    set_word(&mut bytes, 8, DDSD_PITCH);
    set_word(&mut bytes, 20, 16);
    set_word(&mut bytes, 80, 0x41);
    set_word(&mut bytes, 88, 32);
    for (offset, value) in [
        (92, 0x00ff_0000),
        (96, 0x0000_ff00),
        (100, 0x0000_00ff),
        (104, 0xff00_0000),
    ] {
        set_word(&mut bytes, offset, value);
    }
    let dds = Dds::parse(&bytes).unwrap();
    assert_eq!(dds.header.pixel_format.r_bit_mask, 0x00ff_0000);
    let surfaces = dds.surfaces(1).unwrap();
    assert_eq!(surfaces[0].row_pitch, 16);
    assert_eq!(surfaces[0].slice_pitch, 32);
    assert_eq!(surfaces[0].bytes.len(), 32);
    set_word(&mut bytes, 20, 8);
    assert!(Dds::parse(&bytes).unwrap().surfaces(1).is_err());
}

#[test]
fn legacy_cube_faces_and_volume_depth_use_stored_order() {
    let mut cube = file(4, 4, 1, b"DXT5", 32);
    set_word(&mut cube, 112, DDSCAPS2_CUBEMAP | 0x400 | 0x8000);
    let dds = Dds::parse(&cube).unwrap();
    let surfaces = dds.surfaces(2).unwrap();
    assert_eq!(surfaces[0].cube_face, Some(CubeFace::PositiveX));
    assert_eq!(surfaces[1].cube_face, Some(CubeFace::NegativeZ));
    assert_eq!(surfaces[1].offset, 144);

    let mut volume = file(4, 4, 2, b"DXT1", 24);
    set_word(&mut volume, 24, 2);
    set_word(&mut volume, 112, DDSCAPS2_VOLUME);
    let dds = Dds::parse(&volume).unwrap();
    let surfaces = dds.surfaces(2).unwrap();
    assert_eq!(surfaces[0].depth, 2);
    assert_eq!(surfaces[0].bytes.len(), 16);
    assert_eq!(surfaces[1].depth, 1);
    assert_eq!(surfaces[1].bytes.len(), 8);
}

#[test]
fn dx10_cube_arrays_and_unknown_dxgi_values_are_not_rewritten() {
    let mut bytes = file(4, 4, 1, b"DX10", 0);
    for value in [98u32, 3, 4, 2, 0x1234_0003] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.resize(bytes.len() + 12 * 16, 0x33);
    let dds = Dds::parse(&bytes).unwrap();
    assert_eq!(dds.data_offset, 148);
    assert_eq!(dds.dx10.unwrap().misc_flags_2, 0x1234_0003);
    let surfaces = dds.surfaces(12).unwrap();
    assert_eq!(surfaces.len(), 12);
    assert_eq!(surfaces[6].array_index, 1);
    assert_eq!(surfaces[6].cube_face, Some(CubeFace::PositiveX));
    assert!(dds.surfaces(11).is_err());
    set_word(&mut bytes, 128, 0xffff_1234);
    let dds = Dds::parse(&bytes).unwrap();
    assert_eq!(dds.encoding(), Encoding::Dxgi(0xffff_1234));
    assert!(dds.surfaces(12).is_err());
    assert_eq!(dds.as_bytes(), bytes);
}

#[test]
fn truncated_headers_and_surfaces_and_extreme_counts_fail() {
    let bytes = file(4, 4, 1, b"DXT1", 8);
    for length in 0..128 {
        assert!(Dds::parse(&bytes[..length]).is_err());
    }
    for length in 128..136 {
        assert!(Dds::parse(&bytes[..length]).unwrap().surfaces(1).is_err());
    }
    let mut dx10 = file(1, 1, 1, b"DX10", 0);
    for length in 128..148 {
        dx10.resize(length, 0);
        assert!(Dds::parse(&dx10).is_err());
    }
    let mut huge = file(u32::MAX, u32::MAX, 1, b"DXT5", 0);
    assert!(Dds::parse(&huge).unwrap().surfaces(1).is_err());
    set_word(&mut huge, 28, u32::MAX);
    assert!(Dds::parse(&huge).unwrap().surfaces(1).is_err());
    set_word(&mut huge, 4, 125);
    assert_eq!(Dds::parse(&huge).unwrap_err().offset, 4);
}

#[test]
fn external_dds_sample() {
    let Some(path) = std::env::var_os("MHF_RESOURCE_DDS_SAMPLE") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let dds = Dds::parse(&bytes).unwrap();
    assert_eq!(dds.as_bytes(), bytes);
    assert!(!dds.surfaces(65_536).unwrap().is_empty());
}
