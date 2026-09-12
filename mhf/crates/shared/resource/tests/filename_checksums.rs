use mhf_resource::crypto::{Ecd, Exf, filename_checksum};

#[test]
fn native_filename_checksum_vectors_use_the_seed_and_uppercase_basename() {
    for (seed, name, expected) in [
        (0xd645_46e9, "wi521.bin", 0xac10),
        (0x178f_a5cc, "wi521.bin", 0x6dc1),
        (0x178f_a5cc, "wi522.bin", 0x2d34),
        (0xcbf4_3926, "test.bin", 0xbd48),
        (0x94af_cefc, "s_m68_02.mus", 0x2241),
    ] {
        assert_eq!(filename_checksum(seed, name.as_bytes()).unwrap(), expected);
        let path = format!("Z:\\game\\dat\\weapon\\{}", name.to_ascii_uppercase());
        assert_eq!(filename_checksum(seed, path.as_bytes()).unwrap(), expected);
        assert_eq!(
            filename_checksum(seed, format!("dat/weapon/{name}").as_bytes()).unwrap(),
            expected
        );
    }
    for name in [&b""[..], b"folder/", b"file\0.bin", &[0xff]] {
        assert!(filename_checksum(0, name).is_err());
    }
}

#[test]
fn bound_encoding_requires_a_name_and_rebinds_without_changing_the_ciphertext() {
    let template = b"ecd\x1a\x04\0\x10\xac\0\0\0\0\0\0\0\0tail";
    let file = Ecd::parse(template).unwrap();
    assert!(file.encode(b"123456789", None).is_err());
    let original = file.encode(b"123456789", Some(b"test.bin")).unwrap();
    let parsed = Ecd::parse(&original).unwrap();
    assert_eq!(parsed.header.filename_checksum, 0xbd48);
    parsed.validate_filename(b"TEST.BIN").unwrap();
    assert!(parsed.validate_filename(b"other.bin").is_err());
    assert_eq!(parsed.trailing_bytes().unwrap(), b"tail");
    assert_eq!(&**parsed.decode(9).unwrap(), b"123456789");

    let renamed = file.encode(b"123456789", Some(b"other.bin")).unwrap();
    assert_eq!(&original[..6], &renamed[..6]);
    assert_ne!(&original[6..8], &renamed[6..8]);
    assert_eq!(&original[8..], &renamed[8..]);

    let mut lower_key = file;
    lower_key.header.key_index = 3;
    let encoded = lower_key.encode(b"123456789", None).unwrap();
    let parsed = Ecd::parse(&encoded).unwrap();
    assert_eq!(parsed.header.filename_checksum, 0xac10);
    parsed.validate_filename(b"").unwrap();
    assert_eq!(&**parsed.decode(9).unwrap(), b"123456789");
}

#[test]
fn exf_rebinding_preserves_seed_unknown_word_and_stream_bytes() {
    let template = b"exf\x1a\x04\0\x41\x22\xaa\xbb\xcc\xdd\xfc\xce\xaf\x94";
    let file = Exf::parse(template).unwrap();
    file.validate_filename(b"sound/mus/s_m68_02.mus").unwrap();
    let original = file.encode(b"stream bytes", None).unwrap();
    let renamed = file.encode(b"stream bytes", Some(b"other.mus")).unwrap();
    assert_eq!(&original[..6], &renamed[..6]);
    assert_ne!(&original[6..8], &renamed[6..8]);
    assert_eq!(&original[8..], &renamed[8..]);
    let parsed = Exf::parse(&renamed).unwrap();
    parsed.validate_filename(b"OTHER.MUS").unwrap();
    assert_eq!(parsed.header.seed, 0x94af_cefc);
    assert_eq!(parsed.header.unknown_08, [0xaa, 0xbb, 0xcc, 0xdd]);
    assert_eq!(&**parsed.decode(100).unwrap(), b"stream bytes");

    let mut lower_key = file;
    lower_key.header.key_index = 3;
    let encoded = lower_key.encode(b"changed", Some(b"other.mus")).unwrap();
    let parsed = Exf::parse(&encoded).unwrap();
    assert_eq!(parsed.header.filename_checksum, 0x2241);
    parsed.validate_filename(b"").unwrap();
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; checks original EXF audio filename bindings"]
fn original_exf_audio_filename_bindings() {
    use std::io::Read;
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut folders = vec![root.join("dat/sound")];
    let mut checked = 0;
    while let Some(folder) = folders.pop() {
        for item in std::fs::read_dir(folder).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let mut header = [0; 16];
            let mut reader = std::fs::File::open(&path).unwrap();
            if reader.read_exact(&mut header).is_err() || !header.starts_with(b"exf\x1a") {
                continue;
            }
            let file = Exf::parse(&header).unwrap();
            if file.header.key_index == 4 {
                file.validate_filename(path.file_name().unwrap().to_str().unwrap().as_bytes())
                    .unwrap();
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
    eprintln!("Validated {checked} original EXF filename bindings");
}
