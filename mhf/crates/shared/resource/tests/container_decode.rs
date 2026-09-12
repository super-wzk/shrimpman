use mhf_resource::{
    container::{MhaArchive, SimpleArchive, StageArchive, open_layers},
    crypto::{Ecd, Exf, crc32},
    jkr::{HuffmanTable, Jkr},
};

fn jkr(encoding: u16, size: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"JKR\x1a\x08\x01".to_vec();
    bytes.extend_from_slice(&encoding.to_le_bytes());
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn jkr_encoding_numbers_and_each_lz_instruction() {
    for encoding in [0, 1] {
        let encoded = jkr(encoding, 3, b"abcunused");
        let resource = Jkr::parse(&encoded).unwrap();
        let decoded = resource.decode(3).unwrap();
        assert_eq!(&**decoded, b"abc");
        assert_eq!(decoded.encoding.source, encoded);
        assert_eq!(decoded.encoding.source.as_ptr(), encoded.as_ptr());
    }
    for (payload, size) in [
        (vec![0x40, b'A', 0], 4),        // Short overlapping copy.
        (vec![0x60, b'A', 0x20, 0], 4),  // Three-bit length.
        (vec![0x60, b'A', 0, 0], 11),    // Four additional length bits.
        (vec![0x70, b'A', 0, 0, 0], 27), // Byte length + 26.
    ] {
        let encoded = jkr(3, size, &payload);
        assert_eq!(
            &**Jkr::parse(&encoded).unwrap().decode(100).unwrap(),
            vec![b'A'; size as usize].as_slice()
        );
    }
    let mut run = vec![0xe0, 0, 0, 0xff];
    run.extend(0..27);
    let encoded = jkr(3, 27, &run);
    assert_eq!(
        &**Jkr::parse(&encoded).unwrap().decode(100).unwrap(),
        (0..27).collect::<Vec<u8>>().as_slice()
    );
}

#[test]
fn huffman_only_and_huffman_over_lz_use_distinct_bit_streams() {
    // Root 256 -> A/B, MSB-first bit sequence 010 -> ABA.
    let encoded = jkr(2, 3, &[0, 1, b'A', 0, b'B', 0, 0x40]);
    assert_eq!(&**Jkr::parse(&encoded).unwrap().decode(3).unwrap(), b"ABA");
    // Root 257 -> 0x40 / node256; node256 -> A / 0.
    // Huffman bits 0 10 11 produce LZ bytes [0x40, A, 0].
    let encoded = jkr(4, 4, &[1, 1, b'A', 0, 0, 0, 0x40, 0, 0, 1, 0x58]);
    assert_eq!(&**Jkr::parse(&encoded).unwrap().decode(4).unwrap(), b"AAAA");
}

#[test]
fn decoder_rejects_truncation_cycles_output_overflow_and_unknown_encoding() {
    for payload in [&[0x80, 0][..], &[0, b'A'], &[0x40, b'A', 0]] {
        let encoded = jkr(3, 3, payload);
        assert!(Jkr::parse(&encoded).unwrap().decode(3).is_err());
    }
    assert!(HuffmanTable::parse(&[0, 1, 0, 1, b'A', 0], 0).is_err());
    assert!(HuffmanTable::parse(&[0, 1, 1, 1, b'A', 0], 0).is_err());
    let encoded = jkr(0, u32::MAX, &[]);
    assert!(Jkr::parse(&encoded).unwrap().decode(100).is_err());
    let encoded = jkr(99, 0, &[]);
    let file = Jkr::parse(&encoded).unwrap();
    assert_eq!(file.header.encoding, 99);
    assert!(file.decode(0).is_err());
}

#[test]
fn layer_history_preserves_original_and_enforces_cumulative_limits() {
    let inner = jkr(0, 3, b"abc");
    let outer = jkr(0, inner.len() as u32, &inner);
    let opened = open_layers(&outer, inner.len() + 3, 2).unwrap();
    assert_eq!(opened.payload(), b"abc");
    assert_eq!(opened.source, outer);
    assert_eq!(opened.layer_source(0).unwrap(), outer);
    assert_eq!(opened.layer_source(1).unwrap(), inner);
    assert!(opened.layer_source(2).is_none());
    assert!(open_layers(&outer, inner.len() + 2, 2).is_err());
    assert!(open_layers(&outer, 100, 1).is_err());
}

#[test]
fn transparent_codec_values_can_be_borrowed_as_typed_resources_without_self_references() {
    let model = words(&[1, 0, 12]);
    let encoded = jkr(0, model.len() as u32, &model);
    let decoded = Jkr::parse(&encoded).unwrap().decode(model.len()).unwrap();
    let parsed = mhf_resource::fmod::Fmod::parse(&decoded).unwrap();
    assert_eq!(parsed.as_bytes().as_ptr(), decoded.as_ptr());
    assert_eq!(decoded.encoding.source.as_ptr(), encoded.as_ptr());
    assert!(std::ptr::eq(decoded.as_ref(), &*decoded));

    let decoded = decoded.map_inner(std::sync::Arc::<[u8]>::from);
    let parsed = mhf_resource::fmod::Fmod::parse(&decoded).unwrap();
    assert_eq!(parsed.as_bytes().as_ptr(), decoded.as_ptr());
    assert_eq!(decoded.encoding.header.decoded_size as usize, model.len());

    let outer = jkr(0, encoded.len() as u32, &encoded);
    let opened = open_layers(&outer, encoded.len() + model.len(), 2).unwrap();
    let parsed = mhf_resource::fmod::Fmod::parse(&opened).unwrap();
    assert_eq!(parsed.as_bytes().as_ptr(), opened.layers[1].as_ptr());
    assert_eq!(opened.layer_source(0).unwrap().as_ptr(), outer.as_ptr());
    assert_eq!(
        opened.layer_source(1).unwrap().as_ptr(),
        opened.layers[0].as_ptr()
    );
    assert!(matches!(
        opened.layers[0].encoding,
        mhf_resource::container::LayerHeader::Jkr(_)
    ));
}

#[test]
fn exf_transparent_value_keeps_its_exact_source_and_unknown_header() {
    let source = b"exf\x1a\0\0\xaa\xbb\x11\x22\x33\x44\x55\x66\x77\x88";
    let decoded = Exf::parse(source).unwrap().decode(0).unwrap();
    assert!(decoded.is_empty());
    assert_eq!(decoded.encoding.source.as_ptr(), source.as_ptr());
    assert_eq!(decoded.encoding.header.unknown_06, [0xaa, 0xbb]);
    assert_eq!(decoded.encoding.header.unknown_08, [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(decoded.encoding.header.seed, 0x8877_6655);
}

#[test]
fn directories_preserve_empty_slots_aliases_gaps_and_trailing_bytes() {
    let mut source = words(&[3, 32, 3, u32::MAX, 0, 32, 3]);
    source.extend_from_slice(b"gap!abctrailing");
    let archive = SimpleArchive::parse(&source, 3).unwrap();
    assert_eq!(archive.payload(0).unwrap(), b"abc");
    assert_eq!(archive.payload(1).unwrap(), b"");
    assert_eq!(archive.payload(2).unwrap(), b"abc");
    assert_eq!(archive.entries[1].offset, u32::MAX);
    assert_eq!(archive.source, source);
    assert!(SimpleArchive::parse(&source, 2).is_err());
    source[4..8].copy_from_slice(&4u32.to_le_bytes());
    assert!(SimpleArchive::parse(&source, 3).is_err());

    let mut momo = b"MOMO".to_vec();
    momo.extend(words(&[1, 16, 3]));
    momo.extend_from_slice(b"abc");
    assert_eq!(
        SimpleArchive::parse(&momo, 1).unwrap().payload(0).unwrap(),
        b"abc"
    );
}

#[test]
fn stage_and_named_archive_have_their_own_layouts() {
    let mut stage = words(&[40, 3, 0, 0, 0, 0, 1, 0xfedcba98, 43, 2]);
    stage.extend_from_slice(b"abcde");
    let parsed = StageArchive::parse(&stage, 4).unwrap();
    assert_eq!(parsed.entries[0].resource_id, None);
    assert_eq!(parsed.entries[3].resource_id, Some(0xfedcba98));
    assert_eq!(parsed.entries[3].entry.payload(&stage).unwrap(), b"de");
    assert!(SimpleArchive::parse(&stage, 4).is_err());
    assert!(StageArchive::probe(&stage, 4).is_err());

    let mut mha = b"mha\x01".to_vec();
    mha.extend(words(&[24, 1, 44, 3]));
    mha.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    mha.extend(words(&[0, 47, 3, 4, 0xaabbccdd]));
    mha.extend_from_slice(b"\xffx\0abc\xee");
    let parsed = MhaArchive::parse(&mha, 1).unwrap();
    assert_eq!(parsed.header.unknown_14, 0x2211);
    assert_eq!(parsed.entries[0].name, b"\xffx");
    assert_eq!(parsed.entries[0].entry.payload(&mha).unwrap(), b"abc");
    assert_eq!(parsed.entries[0].padded_size, 4);
    assert_eq!(parsed.entries[0].file_id, 0xaabbccdd);
    assert!(MhaArchive::parse(&mha[..mha.len() - 1], 1).is_err());
}

#[test]
fn stage_probe_requires_a_complete_native_placement_table() {
    let mut bytes = words(&[40, 76, 0, 0, 0, 0, 1, 77, 116, 4]);
    bytes.extend(words(&[2, 1, u32::MAX, 8]));
    let mut placement = [0; 60];
    placement[54..56].copy_from_slice(&77u16.to_le_bytes());
    bytes.extend_from_slice(&placement);
    bytes.extend_from_slice(b"data");
    let archive = StageArchive::probe(&bytes, 4).unwrap();
    assert_eq!(archive.entries[3].resource_id, Some(77));
    assert_eq!(archive.entries[3].entry.payload(&bytes).unwrap(), b"data");
    bytes[48..52].fill(0);
    assert!(StageArchive::parse(&bytes, 4).is_ok());
    assert!(StageArchive::probe(&bytes, 4).is_err());
}

#[test]
fn crc32_known_answer_and_ecd_header_bounds() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    assert_eq!(crc32(b""), 0);
    let mut encoded = b"ecd\x1a\x04\0\xaa\xbb".to_vec();
    encoded.extend(words(&[0, 0]));
    encoded.extend_from_slice(b"trailer");
    let file = Ecd::parse(&encoded).unwrap();
    let decoded = file.decode(0).unwrap();
    assert_eq!(&**decoded, b"");
    assert_eq!(decoded.encoding.header.unknown_06, [0xaa, 0xbb]);
    assert_eq!(decoded.encoding.trailing_bytes().unwrap(), b"trailer");
    assert_eq!(decoded.encoding.source.as_ptr(), encoded.as_ptr());
    encoded[8..12].copy_from_slice(&100u32.to_le_bytes());
    assert!(Ecd::parse(&encoded).is_err());
}

#[test]
fn ecd_known_answers_cover_all_key_sets_and_detect_corruption() {
    // Produced by the independent ReFrontier EncodeEcd algorithm for
    // "123456789", unknown_06=aabb. Decoding must also validate its CRC.
    let encrypted = [
        "6563641a0000aabb090000002639f4cbf577b418203c37b628",
        "6563641a0100aabb090000002639f4cbdeb25b2b8fdaea8924",
        "6563641a0200aabb090000002639f4cbdeb25b2b8fdaea8924",
        "6563641a0300aabb090000002639f4cbdeb25b2b8fdaea8924",
        "6563641a0400aabb090000002639f4cbd66a904d4dd823f293",
        "6563641a0500aabb090000002639f4cba2c4fa3dd82ff546ef",
    ];
    for hex in encrypted {
        let mut bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
            .collect();
        assert_eq!(
            &**Ecd::parse(&bytes).unwrap().decode(9).unwrap(),
            b"123456789"
        );
        bytes[20] ^= 1;
        assert!(Ecd::parse(&bytes).unwrap().decode(9).is_err());
    }
}

#[test]
fn changed_public_header_offsets_return_errors_without_indexing_panics() {
    let encoded = jkr(0, 3, b"abc");
    let mut file = Jkr::parse(&encoded).unwrap();
    file.header.data_offset = u32::MAX;
    assert!(file.header_extension().is_err());
    assert!(file.decode(3).is_err());
    let encoded = b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0";
    let mut file = Ecd::parse(encoded).unwrap();
    file.header.payload_size = u32::MAX;
    assert!(file.trailing_bytes().is_err());
    assert!(file.decode(10).is_err());
    let mut table = HuffmanTable::parse(&[0, 1, b'A', 0, b'B', 0], 0).unwrap();
    table.offset = usize::MAX;
    assert!(table.children(256).is_err());
}

#[test]
fn mha_duplicate_and_suffix_name_offsets_keep_the_original_views() {
    let mut bytes = b"mha\x01".to_vec();
    bytes.extend(words(&[24, 3, 84, 4, 0]));
    for name in [0, 1, 1] {
        bytes.extend(words(&[name, 88, 1, 1, 7]));
    }
    bytes.extend_from_slice(b"abc\0x");
    let archive = MhaArchive::parse(&bytes, 3).unwrap();
    assert_eq!(
        archive
            .entries
            .iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>(),
        [b"abc".as_slice(), b"bc", b"bc"]
    );
    assert_eq!(
        archive.entries[1].name.as_ptr(),
        archive.entries[2].name.as_ptr()
    );
    assert_eq!(archive.source, bytes);
}

#[test]
#[ignore = "requires the caller's game installation; set MHF_RESOURCE_GAME_ROOT"]
fn installed_game_containers_and_jkr_0_3_4() {
    let root = std::path::PathBuf::from(
        std::env::var_os("MHF_RESOURCE_GAME_ROOT").expect("set MHF_RESOURCE_GAME_ROOT"),
    );
    for (name, encoding, checksum) in [
        ("dat/my_gallery/s264_w006.bin", 0, 0x992ef74c),
        ("dat/ryoudan/rquest.bin", 3, 0x93606541),
        ("dat/motion/npc41.mot", 4, 0x15e676e5),
    ] {
        let source = std::fs::read(root.join(name)).unwrap();
        let file = Jkr::parse(&source).unwrap();
        assert_eq!(file.header.encoding, encoding);
        let output = file.decode(64 * 1024 * 1024).unwrap();
        assert_eq!(output.len(), output.encoding.header.decoded_size as usize);
        assert_eq!(crc32(&output), checksum);
        eprintln!(
            "{name}: JKR{encoding} -> {} bytes, CRC32 {:08x}",
            output.len(),
            crc32(&output)
        );
    }
    let source = std::fs::read(root.join("dat/parts/m00/m_editpl.bin")).unwrap();
    let archive = SimpleArchive::parse(&source, 1000).unwrap();
    assert_eq!(archive.count, 3);
    for index in 0..3 {
        let opened = open_layers(archive.payload(index).unwrap(), 64 * 1024 * 1024, 8).unwrap();
        assert_eq!(
            crc32(opened.payload()),
            [0xdf14a893, 0xf514356b, 0xf09b509c][index]
        );
        if index == 2 {
            assert_eq!(
                SimpleArchive::parse(opened.payload(), 1000).unwrap().count,
                141
            );
        }
        eprintln!(
            "m_editpl.bin/{index}: {} bytes, CRC32 {:08x}",
            opened.payload().len(),
            crc32(opened.payload())
        );
    }
    let source = std::fs::read(root.join("dat/stage-hd/st063-hd.pac")).unwrap();
    let opened = open_layers(&source, 64 * 1024 * 1024, 8).unwrap();
    assert!(matches!(
        opened.layers[0].encoding,
        mhf_resource::container::LayerHeader::Ecd(_)
    ));
    eprintln!(
        "st063-hd.pac: {} layers, {} bytes, CRC32 {:08x}",
        opened.layers.len(),
        opened.payload().len(),
        crc32(opened.payload())
    );
    let source = std::fs::read(root.join("dat/wd000snd.abn")).unwrap();
    let archive = MhaArchive::parse(&source, 10000).unwrap();
    assert_eq!(archive.header.count, 14);
    let source = std::fs::read(root.join("dat/sound/grdn_fes.snd")).unwrap();
    assert_eq!(SimpleArchive::parse(&source, 10000).unwrap().count, 3);
    let source = std::fs::read(root.join("dat/sound/mus/s_m68_02.mus")).unwrap();
    let opened = open_layers(&source, 64 * 1024 * 1024, 8).unwrap();
    eprintln!(
        "s_m68_02.mus: {} bytes, CRC32 {:08x}, prefix {:?}",
        opened.payload().len(),
        crc32(opened.payload()),
        &opened.payload()[..16]
    );
    assert_eq!(crc32(opened.payload()), 0x94afcefc);
}
