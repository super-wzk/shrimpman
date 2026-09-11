use super::*;

fn dds() -> Vec<u8> {
    let mut words = [0u32; 31];
    words[0] = 124;
    words[2] = 4;
    words[3] = 4;
    words[18] = 32;
    words[19] = 4;
    words[20] = u32::from_le_bytes(*b"DXT1");
    let mut bytes = b"DDS ".to_vec();
    bytes.extend(words.into_iter().flat_map(u32::to_le_bytes));
    bytes.extend_from_slice(&[0xcc; 8]);
    bytes
}

#[test]
fn empty_aliases_unknowns_and_verbatim_image_extraction_keep_slots() {
    let image = dds();
    let mut bytes = Vec::new();
    // Two directory slots deliberately reference the same original DDS.
    for value in [4u32, 36, 136, u32::MAX, 0, 36, 136, 172, 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&image);
    bytes.extend_from_slice(&[0xfa, 0xfb, 0xfc]);
    let txb = Txb::parse(&bytes, 4).unwrap();
    assert_eq!(txb.textures[0].image.dimensions(), Some((4, 4)));
    assert!(matches!(txb.textures[1].image, Image::Empty));
    assert!(matches!(txb.textures[3].image, Image::Unknown(_)));
    assert_eq!(txb.extract(0).unwrap(), image);
    assert_eq!(txb.extract(2).unwrap(), image);
    assert_eq!(
        txb.extract(0).unwrap().as_ptr(),
        txb.extract(2).unwrap().as_ptr()
    );
    assert_eq!(txb.as_bytes(), bytes);
    assert!(Txb::parse(&bytes, 3).is_err());
    assert!(txb.extract(4).is_err());
}

#[test]
fn malformed_image_error_has_bundle_offset() {
    let mut bytes = Vec::new();
    for value in [1u32, 12, 4] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"DDS ");
    assert_eq!(Txb::parse(&bytes, 1).unwrap_err().offset, 16);
}

#[test]
fn external_txb_sample() {
    let Some(path) = std::env::var_os("MHF_RESOURCE_TXB_SAMPLE") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let txb = Txb::parse(&bytes, 65_536).unwrap();
    assert_eq!(txb.as_bytes(), bytes);
    assert!(!txb.textures.is_empty());
    for texture in &txb.textures {
        match &texture.image {
            Image::Png(png) => png.validate().unwrap(),
            Image::Dds(dds) => {
                dds.surfaces(65_536).unwrap();
            }
            Image::Empty | Image::Unknown(_) => {}
        }
        assert_eq!(
            txb.extract(texture.entry.index).unwrap(),
            texture.image.as_bytes()
        );
    }
}
