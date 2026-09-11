use super::*;

fn chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(&kind);
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(&crate::crypto::crc32(&bytes[4..]).to_be_bytes());
    bytes
}

fn png() -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&128u32.to_be_bytes());
    ihdr.extend_from_slice(&64u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
    bytes.extend(chunk(*b"IHDR", &ihdr));
    bytes.extend(chunk(*b"PLTE", &[1, 2, 3, 4, 5, 6]));
    bytes.extend(chunk(*b"vpAg", &[0xaa, 0xbb, 0xcc]));
    // Intentionally opaque: validating chunks does not validate zlib pixels.
    bytes.extend(chunk(*b"IDAT", &[0x78, 0x9c, 0x31, 0x32]));
    bytes.extend(chunk(*b"IEND", &[]));
    bytes
}

#[test]
fn ihdr_endianness_palette_unknown_chunks_and_bytes_are_preserved() {
    let mut source = png();
    source.extend_from_slice(&[0xf1, 0xf2]);
    let parsed = Png::parse(&source).unwrap();
    assert_eq!((parsed.header.width, parsed.header.height), (128, 64));
    assert_eq!((parsed.header.bit_depth, parsed.header.color_type), (8, 3));
    assert_eq!(parsed.chunks[2].kind, *b"vpAg");
    assert_eq!(parsed.chunks[2].data, [0xaa, 0xbb, 0xcc]);
    assert_eq!(parsed.trailing, [0xf1, 0xf2]);
    assert_eq!(parsed.as_bytes(), source);
    parsed.validate().unwrap();
}

#[test]
fn truncated_chunk_headers_payloads_and_checksums_fail() {
    let source = png();
    for length in 0..source.len() {
        assert!(Png::parse(&source[..length]).is_err(), "{length}");
    }
    let mut bad = source.clone();
    bad[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Png::parse(&bad).is_err());
    bad = source;
    let parsed = Png::parse(&bad).unwrap();
    let checksum = parsed.chunks[2].offset + parsed.chunks[2].source.len() - 1;
    bad[checksum] ^= 1;
    assert!(Png::parse(&bad).unwrap().validate().is_err());
}

#[test]
fn invalid_palette_and_chunk_order_are_reported_without_rewriting() {
    let source = png();
    let parsed = Png::parse(&source).unwrap();
    let mut reordered = MAGIC.to_vec();
    for i in [0, 3, 1, 4] {
        reordered.extend_from_slice(parsed.chunks[i].source);
    }
    let parsed = Png::parse(&reordered).unwrap();
    assert!(parsed.validate().is_err());
    assert_eq!(parsed.as_bytes(), reordered);
}
