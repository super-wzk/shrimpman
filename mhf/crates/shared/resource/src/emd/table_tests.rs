use super::*;

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 2048];
    put32(&mut bytes, 0, 96);
    bytes[100] = 1;
    put32(&mut bytes, 12, 144);
    bytes
}

#[test]
fn species_parameter_directory_has_fixed_extent_and_opaque_links() {
    let mut bytes = fixture();
    assert!(
        Emd::parse(&bytes)
            .unwrap()
            .directory_table(3, 0)
            .unwrap()
            .is_none()
    );
    put32(&mut bytes, 144 + 184, 400);
    // Target contents and the second word are not interpreted as a counted table.
    put32(&mut bytes, 400, u32::MAX);
    put32(&mut bytes, 404, u32::MAX);
    let file = Emd::parse(&bytes).unwrap();
    let table = file.directory_table(3, 0).unwrap().unwrap();
    assert_eq!(table.range, 400..2000);
    assert_eq!((table.count, table.stride), (200, 8));
    assert_eq!(table.kind, RecordKind::ParameterLink);
    assert_eq!(table.record(0).unwrap().1, &[255; 8]);
    assert!(table.record(200).is_err());
    assert!(file.directory_table(3, 1).is_err());
    bytes.truncate(1999);
    assert!(Emd::parse(&bytes).unwrap().directory_table(3, 0).is_err());
    put32(&mut bytes, 144 + 184, 96);
    assert!(Emd::parse(&bytes).unwrap().directory_table(3, 0).is_err());
}

#[test]
fn fixed_and_header_counted_tables_have_exact_extents() {
    for (slot, header, count, stride) in [
        (1, None, 12, 4),
        (2, None, 1, 52),
        (4, None, 12, 4),
        (5, None, 1, 6),
        (6, None, 3, 4),
        (7, Some(12), 2, 12),
        (9, Some(16), 2, 4),
        (11, None, 1, 18),
        (13, Some(18), 2, 28),
        (14, Some(20), 2, 12),
        (15, Some(22), 2, 2),
        (16, Some(22), 2, 4),
        (17, Some(24), 2, 8),
        (18, Some(26), 2, 18),
        (19, Some(28), 2, 32),
        (21, None, 1, 2),
        (22, Some(34), 2, 28),
    ] {
        let mut bytes = fixture();
        put32(&mut bytes, slot * 4, 400);
        if let Some(offset) = header {
            bytes[96 + offset] = count as u8;
        }
        // The consumer of table 18 reads a byte, not the neighboring byte.
        if slot == 18 {
            bytes[96 + 27] = 0x7f;
        }
        let file = Emd::parse(&bytes).unwrap();
        let table = file.root_table(slot).unwrap().unwrap();
        assert_eq!(table.range, 400..400 + count * stride, "slot {slot}");
        assert_eq!(table.count, count);
        assert_eq!(table.stride, stride);
        assert!(table.record(count).is_err());
        let mut covered = vec![false; stride];
        for field in table.kind.fields() {
            let end = field.offset + field.scalar.size();
            assert!(end <= stride);
            assert!(covered[field.offset..end].iter().all(|byte| !byte));
            covered[field.offset..end].fill(true);
        }
    }
}

#[test]
fn profile_directories_preserve_aliases_and_validate_targets() {
    let mut bytes = fixture();
    put32(&mut bytes, 4, 400);
    put32(&mut bytes, 400, 800);
    put32(&mut bytes, 404, 800);
    let file = Emd::parse(&bytes).unwrap();
    let first = file.directory_table(1, 0).unwrap().unwrap();
    assert_eq!(first.range, 800..834);
    assert_eq!(first.kind, RecordKind::PartParameters);
    assert_eq!(
        file.directory_table(1, 1).unwrap().unwrap().range,
        first.range
    );
    assert!(file.directory_table(1, 2).is_err()); // Unconditional zero relocation is not null.
    assert!(file.directory_table(1, 12).is_err());
    put32(&mut bytes, 400, 2047);
    assert!(Emd::parse(&bytes).unwrap().directory_table(1, 0).is_err());
}

#[test]
fn sentinel_directory_is_bounded_and_terminator_is_preserved() {
    let mut bytes = fixture();
    put32(&mut bytes, 40, 400);
    put32(&mut bytes, 400, 800);
    put32(&mut bytes, 404, 900);
    let file = Emd::parse(&bytes).unwrap();
    let table = file.root_table(10).unwrap().unwrap();
    assert_eq!(table.count, 2);
    assert_eq!(table.terminator, Some(408..412));
    assert_eq!(
        file.directory_table(10, 1).unwrap().unwrap().range,
        900..990
    );
    bytes[400..].fill(1);
    assert!(Emd::parse(&bytes).unwrap().root_table(10).is_err());
    put32(&mut bytes, 400, 0);
    let table = Emd::parse(&bytes).unwrap().root_table(10).unwrap().unwrap();
    assert_eq!(table.count, 0);
    assert_eq!(table.terminator, Some(400..404));
}

#[test]
fn parallel_group_counts_control_the_target_extent() {
    let mut bytes = fixture();
    bytes[96 + 22] = 2;
    put32(&mut bytes, 60, 400);
    put32(&mut bytes, 64, 500);
    bytes[400] = 3;
    bytes[402] = 1;
    put32(&mut bytes, 500, 800);
    put32(&mut bytes, 504, 900);
    let file = Emd::parse(&bytes).unwrap();
    assert_eq!(
        file.directory_table(16, 0).unwrap().unwrap().range,
        800..896
    );
    assert_eq!(
        file.directory_table(16, 1).unwrap().unwrap().range,
        900..932
    );
}

#[test]
fn association_action_rules_use_counted_four_byte_records() {
    let mut bytes = fixture();
    bytes[96 + 28] = 1;
    put32(&mut bytes, 19 * 4, 400);
    bytes[426..428].copy_from_slice(&256u16.to_le_bytes());
    put32(&mut bytes, 428, 800);
    bytes[800..804].copy_from_slice(&[7, 2, 44, 1]);
    let file = Emd::parse(&bytes).unwrap();
    let table = file.directory_table(19, 0).unwrap().unwrap();
    assert_eq!(table.kind, RecordKind::ActionRule);
    assert_eq!(table.range, 800..1824);
    assert_eq!(table.count, 256);
    assert_eq!(table.record(0).unwrap().1, &[7, 2, 44, 1]);
    assert!(table.record(256).is_err());
    assert!(file.directory_table(19, 1).is_err());
    put32(&mut bytes, 428, 2047);
    assert!(Emd::parse(&bytes).unwrap().directory_table(19, 0).is_err());
}

#[test]
fn association_links_are_inactive_when_either_gate_is_zero() {
    for (count, offset) in [(0u16, u32::MAX), (5, 0), (0, 0)] {
        let mut bytes = fixture();
        bytes[96 + 28] = 1;
        put32(&mut bytes, 19 * 4, 400);
        bytes[426..428].copy_from_slice(&count.to_le_bytes());
        put32(&mut bytes, 428, offset);
        assert!(
            Emd::parse(&bytes)
                .unwrap()
                .directory_table(19, 0)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn unknown_layouts_are_not_sized_from_neighboring_roots() {
    let mut bytes = fixture();
    for slot in [8, 12, 20, 23] {
        put32(&mut bytes, slot * 4, 500);
        let file = Emd::parse(&bytes).unwrap();
        assert_eq!(file.root_offset(slot).unwrap(), 500);
        assert!(file.root_table(slot).unwrap().is_none());
    }
    assert!(Emd::parse(&bytes).unwrap().root_table(24).is_err());
}

#[test]
fn malformed_tables_do_not_prevent_reading_unrelated_records() {
    let mut bytes = fixture();
    put32(&mut bytes, 4, 100); // Overlaps header.
    let file = Emd::parse(&bytes).unwrap();
    assert!(file.root_table(1).is_err());
    assert!(file.root_table(3).is_ok());
    assert_eq!(file.species().next().unwrap().bytes.len(), SPECIES_STRIDE);
    let mut short = vec![0; 130];
    put32(&mut short, 0, 96);
    assert!(Emd::parse(&short).is_err());
}
