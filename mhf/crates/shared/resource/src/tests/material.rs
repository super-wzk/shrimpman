use super::*;

fn header(count: u8) -> Vec<u8> {
    let mut bytes = vec![0xcc; HEADER_SIZE];
    bytes[0] = count;
    bytes
}

fn record(stride: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for i in 0..stride / 4 {
        let value = match i {
            0 => 0x7fc0_1234u32,
            1 => 0x8000_0000,
            _ => 0x0102_0000 + i as u32,
        };
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn sample(marker: Option<u8>) -> Vec<u8> {
    let mut source = marker.into_iter().collect::<Vec<_>>();
    source.extend_from_slice(&header(2));
    // Empty groups still occupy a group ordinal.
    source.extend_from_slice(&header(0));
    source.extend_from_slice(&header(1));
    source.extend_from_slice(&record(if marker.is_some() {
        EXTENDED_RECORD_SIZE
    } else {
        LEGACY_RECORD_SIZE
    }));
    source
}

#[test]
fn both_record_layouts_preserve_original_bits_headers_and_tails() {
    for marker in [None, Some(0x20), Some(0xfe)] {
        let mut source = sample(marker);
        source.extend_from_slice(&[0xfa, 0xfb, 0xfc]);
        let file = GroupedMaterials::parse(&source).unwrap();
        assert_eq!(file.version_marker, marker);
        assert_eq!(file.header.offset, usize::from(marker.is_some()));
        assert_eq!(file.header.count, 2);
        assert_eq!(file.header.unknown, [0xcc; 15]);
        assert_eq!(file.groups.len(), 2);
        assert!(file.groups[0].records.is_empty());
        let group = &file.groups[1];
        assert_eq!(group.header.offset, file.header.offset + 32);
        assert_eq!(group.header.unknown, [0xcc; 15]);
        let value = &group.records[0];
        assert_eq!(value.offset, file.header.offset + 48);
        assert_eq!(value.color_00[0], 0x7fc0_1234);
        assert_eq!(value.color_00[1], 0x8000_0000);
        assert_eq!(value.color_10[0], 0x0102_0004);
        assert_eq!(value.color_20[3], 0x0102_000b);
        assert_eq!(
            value.parameter_words.len(),
            6 + usize::from(marker.is_some())
        );
        assert_eq!(value.parameter_words[3], 0x0102_000f);
        assert_eq!(value.unknown_tail.len(), 24);
        assert_eq!(value.as_bytes(), record(value.as_bytes().len()));
        assert_eq!(file.trailing, [0xfa, 0xfb, 0xfc]);
        assert_eq!(file.as_bytes(), source);
    }
}

#[test]
fn every_declared_header_and_record_boundary_is_checked() {
    for marker in [None, Some(0x20)] {
        let source = sample(marker);
        for length in 0..source.len() {
            assert!(
                GroupedMaterials::parse(&source[..length]).is_err(),
                "marker {marker:?}, length {length}"
            );
        }
        assert!(GroupedMaterials::parse(&source).is_ok());
    }
}

#[test]
fn signed_counts_and_counts_exceeding_the_file_report_header_offsets() {
    let source = [vec![0x20], header(0x80)].concat();
    assert_eq!(GroupedMaterials::parse(&source).unwrap_err().offset, 1);
    let source = [vec![0x20], header(1), header(0xff)].concat();
    assert_eq!(GroupedMaterials::parse(&source).unwrap_err().offset, 17);
    let source = [vec![0x20], header(127)].concat();
    assert_eq!(GroupedMaterials::parse(&source).unwrap_err().offset, 1);
    let source = [vec![0x20], header(1), header(2), record(100)].concat();
    assert_eq!(GroupedMaterials::parse(&source).unwrap_err().offset, 17);
}

#[test]
fn external_material_sample() {
    let Some(path) = std::env::var_os("MHF_RESOURCE_MATERIAL_SAMPLE") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let parsed = GroupedMaterials::parse(&bytes).unwrap();
    assert_eq!(parsed.as_bytes(), bytes);
    assert!(!parsed.groups.is_empty());
    assert!(parsed.groups.iter().any(|group| !group.records.is_empty()));
    assert!(parsed.trailing.is_empty());
}
