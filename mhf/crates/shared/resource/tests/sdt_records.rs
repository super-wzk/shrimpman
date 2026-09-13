use std::collections::BTreeSet;

use mhf_resource::{
    binary::{Reader, ScalarType},
    container::open_layers,
    sdt::{HITBOX_SLOTS, Sdt, TableKind},
};

fn word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn dword(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn sample() -> Vec<u8> {
    let mut bytes = vec![0; 0x200];
    // Physical order is intentionally different from native key order.
    word(&mut bytes, 2, 140);
    dword(&mut bytes, 20, 0x1a0);
    dword(&mut bytes, 24, 1);
    let entry = 28;
    word(&mut bytes, entry + 2, 100);
    word(&mut bytes, entry + 4, 2);
    word(&mut bytes, entry + 6, 1);
    dword(&mut bytes, entry + 8, 0x80);
    dword(&mut bytes, entry + 12, 0xd0);
    dword(&mut bytes, entry + 16, 0xf0);
    dword(&mut bytes, entry + 20, 0x180);
    dword(&mut bytes, entry + 24, 1);
    word(&mut bytes, 58, 0xffff);
    for offset in [0x80, 0xa8] {
        word(&mut bytes, offset, 81); // Unhalved file counter.
        word(&mut bytes, offset + 2, 29);
        word(&mut bytes, offset + 4, 58);
        word(&mut bytes, offset + 16, 0);
        dword(&mut bytes, offset + 36, 0xded0_1234); // Preserve unused tail.
    }
    // Signed auxiliary data must not be normalized to a boolean/enum.
    dword(&mut bytes, 0xd0, (-3_i32) as u32);
    for slot in 0..HITBOX_SLOTS {
        dword(
            &mut bytes,
            0xf0 + slot * 4,
            if slot < 2 { 0x110 } else { 0x160 },
        );
    }
    word(&mut bytes, 0x110, 3);
    word(&mut bytes, 0x112, 1); // Capsule attached to a node.
    dword(&mut bytes, 0x11c, (-0.0_f32).to_bits());
    dword(&mut bytes, 0x134, 0x7fc1_2345); // NaN endpoint payload.
    word(&mut bytes, 0x138, 0xffff);
    word(&mut bytes, 0x160, 0xffff); // An actual empty list, not a null slot.
    bytes[0x1f0..].fill(0xa5); // Unclaimed data remains accessible.
    bytes
}

#[test]
fn directory_tables_aliases_and_unclaimed_bytes_keep_physical_identity() {
    let bytes = sample();
    let file = Sdt::probe(&bytes).unwrap();
    assert_eq!(
        file.entries()
            .iter()
            .map(|entry| entry.kind)
            .collect::<Vec<_>>(),
        [140, 100]
    );
    assert_eq!(file.terminator(), 56..60);
    assert!(
        file.table(file.entry(0).unwrap(), TableKind::Attack)
            .unwrap()
            .is_none()
    );
    let entry = file.entry(1).unwrap();
    let attacks = file.table(entry, TableKind::Attack).unwrap().unwrap();
    assert_eq!(attacks.range, 0x80..0xd0);
    assert_eq!(attacks.count, 2);
    let attack = attacks.record(1).unwrap();
    assert_eq!(attack.offset, 0xa8);
    assert_eq!(
        Reader::new(attack.as_bytes())
            .read_at::<u16>(0)
            .unwrap()
            .value,
        81
    );
    assert_eq!(&attack.as_bytes()[36..], &0xded0_1234_u32.to_le_bytes());

    let group = file.hitbox_group(entry, 0).unwrap();
    let first = file.hitboxes(&group, 0).unwrap();
    let alias = file.hitboxes(&group, 1).unwrap();
    assert_eq!(first.range, alias.range);
    assert_eq!(first.range, 0x110..0x138);
    assert_eq!(first.terminator, 0x138..0x13a);
    assert_eq!(file.hitboxes(&group, 2).unwrap().count, 0);
    assert!(attacks.record(2).is_err());
    assert!(file.hitbox_group(entry, 1).is_err());
    assert!(file.hitboxes(&group, 8).is_err());
    assert!(first.record(1).is_err());

    let gaps = file.unclaimed_ranges();
    assert!(gaps.iter().any(|range| range.contains(&0x1f0)));
    assert!(!gaps.iter().any(|range| range.contains(&0x110)));
    assert!(!gaps.iter().any(|range| range.contains(&0x139)));
    assert_eq!(file.as_bytes(), bytes);
}

#[test]
fn broken_child_references_are_local_and_strict_recognition_rejects_them() {
    for (field, value) in [
        (28 + 8, 0),
        (28 + 12, u32::MAX),
        (28 + 16, 12),
        (28 + 20, 0x1ff),
        (28 + 24, u32::MAX),
    ] {
        let mut bytes = sample();
        dword(&mut bytes, field, value);
        let file = Sdt::parse(&bytes).unwrap();
        assert!(Sdt::probe(&bytes).is_err(), "field {field:#x}");
        let unaffected = file
            .table(file.entry(0).unwrap(), TableKind::Extra)
            .unwrap()
            .unwrap();
        assert_eq!(unaffected.count, 1);
        assert_eq!(unaffected.record(0).unwrap().offset, 0x1a0);
    }

    let mut bytes = sample();
    dword(&mut bytes, 28 + 12, 0); // Auxiliary references are optional.
    let file = Sdt::probe(&bytes).unwrap();
    assert!(
        file.table(file.entry(1).unwrap(), TableKind::Auxiliary)
            .unwrap()
            .is_none()
    );

    // The declared auxiliary window may alias an attack table. Native uses the
    // same index bound; the parser must not invent a physical extent by taking
    // the next sorted root pointer, or remove the second reference.
    dword(&mut bytes, 28 + 12, 0x80);
    let file = Sdt::probe(&bytes).unwrap();
    let aux = file
        .table(file.entry(1).unwrap(), TableKind::Auxiliary)
        .unwrap()
        .unwrap();
    assert_eq!(aux.range, 0x80..0xa0);
}

#[test]
fn sentinel_scans_are_bounded_and_null_hitbox_slots_are_not_empty_lists() {
    let mut bytes = sample();
    dword(&mut bytes, 0xf0, 0);
    let file = Sdt::parse(&bytes).unwrap();
    let group = file.hitbox_group(file.entry(1).unwrap(), 0).unwrap();
    assert!(file.hitboxes(&group, 0).is_err());
    assert_eq!(file.hitboxes(&group, 1).unwrap().count, 1);

    let mut bytes = sample();
    // A single WORD terminator at EOF is sufficient; unused row padding is
    // not required just to read an empty list.
    dword(&mut bytes, 0xf0, 0x1fe);
    word(&mut bytes, 0x1fe, 0xffff);
    let file = Sdt::parse(&bytes).unwrap();
    let group = file.hitbox_group(file.entry(1).unwrap(), 0).unwrap();
    assert_eq!(file.hitboxes(&group, 0).unwrap().terminator, 0x1fe..0x200);
    word(&mut bytes, 0x1fe, 5);
    let file = Sdt::parse(&bytes).unwrap();
    assert!(file.hitboxes(&group, 0).is_err());

    let mut bytes = sample();
    word(&mut bytes, 58, 0);
    assert!(Sdt::probe(&bytes).is_err());
    assert!(Sdt::parse(&bytes[..59]).is_err());
    assert!(Sdt::parse(&bytes[..3]).is_err());
    assert!(Sdt::probe(&[0; 4096]).is_err());
    assert!(Sdt::probe(&[0, 0, 0xff, 0xff]).is_err());
    let mut duplicate = sample();
    word(&mut duplicate, 30, 140);
    assert!(Sdt::probe(&duplicate).is_err());
}

#[test]
fn typed_fields_preserve_float_payloads_and_only_write_the_addressed_bytes() {
    let bytes = sample();
    let file = Sdt::parse(&bytes).unwrap();
    let group = file.hitbox_group(file.entry(1).unwrap(), 0).unwrap();
    let list = file.hitboxes(&group, 0).unwrap();
    let record = list.record(0).unwrap();
    let reader = Reader::with_base(record.as_bytes(), record.offset);
    let radius = reader.read_at::<f32>(12).unwrap();
    let endpoint = reader.read_at::<f32>(36).unwrap();
    assert_eq!(radius.value.to_bits(), (-0.0_f32).to_bits());
    assert_eq!(endpoint.value.to_bits(), 0x7fc1_2345);
    assert!(
        record
            .fields()
            .iter()
            .any(|field| field.offset == 36 && field.scalar == ScalarType::F32)
    );
    let mut output = bytes.clone();
    endpoint.write(&mut output, endpoint.value).unwrap();
    assert_eq!(output, bytes);
    radius.write(&mut output, 12.5).unwrap();
    assert_eq!(&output[..0x11c], &bytes[..0x11c]);
    assert_eq!(&output[0x120..], &bytes[0x120..]);
    assert_eq!(&output[0x11c..0x120], &12.5_f32.to_le_bytes());
}

#[test]
fn extra_schemas_follow_current_bank_ranges_and_keep_category_specific_types() {
    let mut bytes = vec![0; 0x40 + 160 * 32];
    word(&mut bytes, 2, 100);
    dword(&mut bytes, 20, 0x40);
    dword(&mut bytes, 24, 160);
    word(&mut bytes, 30, 0xffff);
    let layouts = |bytes: &[u8], index| {
        let file = Sdt::parse(bytes).unwrap();
        file.table(file.entry(0).unwrap(), TableKind::Extra)
            .unwrap()
            .unwrap()
            .record(index)
            .unwrap()
            .fields()
    };
    assert!(layouts(&bytes, 90).is_empty());
    for selector in [125, 129, 136] {
        let mut selected = bytes.clone();
        dword(&mut selected, 0x40 + selector * 32 + 16, 90);
        dword(&mut selected, 0x40 + selector * 32 + 20, 91);
        let layout = layouts(&selected, 90);
        assert_eq!(layout.len(), 8);
        assert_eq!(layout[0].scalar, ScalarType::F32);
        assert_eq!(layout[4].scalar, ScalarType::I32);
        assert_eq!(layouts(&selected, 91).len(), 8);
        assert!(layouts(&selected, 92).is_empty());
        // Moving a range changes its interpretation; original sample row
        // numbers are not a permanent type declaration for the whole format.
        dword(&mut selected, 0x40 + selector * 32 + 16, 92);
        dword(&mut selected, 0x40 + selector * 32 + 20, 92);
        assert!(layouts(&selected, 90).is_empty());
        assert_eq!(layouts(&selected, 92).len(), 8);
        for (start, end) in [(u32::MAX, 92), (93, 92), (90, 160)] {
            dword(&mut selected, 0x40 + selector * 32 + 16, start);
            dword(&mut selected, 0x40 + selector * 32 + 20, end);
            assert!(layouts(&selected, 92).is_empty());
        }
        // A fixed native index still retains its independently proven layout.
        dword(&mut selected, 0x40 + selector * 32 + 16, 103);
        dword(&mut selected, 0x40 + selector * 32 + 20, 103);
        assert!(layouts(&selected, 103)[0].name.contains("威力"));
    }
    // The same offset can be a different type/meaning in another category.
    word(&mut bytes, 2, 999);
    assert!(layouts(&bytes, 103).is_empty());
    assert!(
        layouts(&bytes, 12)
            .iter()
            .any(|field| field.name.contains("抗性"))
    );

    word(&mut bytes, 2, 140);
    let state = 0x40 + 100 * 32 + 20;
    assert!(!layouts(&bytes, 100).iter().any(|field| field.offset == 0));
    assert_eq!(
        layouts(&bytes, 100)
            .iter()
            .find(|field| field.offset == 24)
            .unwrap()
            .scalar,
        ScalarType::U16
    );
    dword(&mut bytes, state, 2);
    assert_eq!(layouts(&bytes, 100)[0].scalar, ScalarType::F32);
    assert_eq!(
        layouts(&bytes, 100)
            .iter()
            .find(|field| field.offset == 24)
            .unwrap()
            .scalar,
        ScalarType::I32
    );
    dword(&mut bytes, state, 5);
    assert_eq!(layouts(&bytes, 100).len(), 2); // Only the two keys are established.
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original mhfsdt.bin without modifying it"]
fn original_sdt_resolves_all_declared_tables_and_unique_hitbox_lists() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfsdt.bin")).unwrap();
    let decoded = open_layers(&source, 128 * 1024 * 1024, 8).unwrap();
    let file = Sdt::probe(decoded.payload()).unwrap();
    let mut totals = [0; 3];
    let mut list_offsets = BTreeSet::new();
    let mut hitbox_offsets = BTreeSet::new();
    let mut hitbox_count = 0;
    let mut group_count = 0;
    let mut typed_fields = 0;
    let mut covered = vec![false; file.as_bytes().len()];
    covered[..file.terminator().end].fill(true);
    let mut check_record = |record: mhf_resource::sdt::Record<'_>| {
        let mut seen = vec![false; record.as_bytes().len()];
        for field in record.fields() {
            let range = usize::from(field.offset)..usize::from(field.offset) + field.scalar.size();
            assert!(range.end <= seen.len());
            assert!(seen[range.clone()].iter().all(|seen| !seen));
            seen[range].fill(true);
            typed_fields += 1;
        }
    };
    for entry in file.entries() {
        for (index, kind) in [TableKind::Attack, TableKind::Auxiliary, TableKind::Extra]
            .into_iter()
            .enumerate()
        {
            if let Some(table) = file.table(entry, kind).unwrap() {
                covered[table.range.clone()].fill(true);
                totals[index] += table.count;
                for record in 0..table.count {
                    check_record(table.record(record).unwrap());
                }
            }
        }
        if let Some(range) = file.hitbox_groups(entry).unwrap() {
            covered[range].fill(true);
            for index in 0..usize::from(entry.hitbox_group_count) {
                group_count += 1;
                let group = file.hitbox_group(entry, index).unwrap();
                for slot in 0..HITBOX_SLOTS {
                    if list_offsets.insert(group.slots[slot]) {
                        let list = file.hitboxes(&group, slot).unwrap();
                        covered[list.range.clone()].fill(true);
                        covered[list.terminator.clone()].fill(true);
                        hitbox_count += list.count;
                        for record in 0..list.count {
                            let record = list.record(record).unwrap();
                            hitbox_offsets.insert(record.offset);
                            check_record(record);
                        }
                    }
                }
            }
        }
    }
    let gaps = file.unclaimed_ranges();
    let unclaimed: usize = gaps.iter().map(|range| range.len()).sum();
    for range in gaps {
        assert!(covered[range.clone()].iter().all(|covered| !covered));
        covered[range].fill(true);
    }
    assert!(covered.iter().all(|covered| *covered));
    println!(
        "SDT: {} categories, {totals:?} attack/auxiliary-window/extra records, {group_count} groups, {} unique lists, {hitbox_count} hitbox references / {} unique record offsets, {typed_fields} typed fields, {unclaimed} unclaimed bytes",
        file.entries().len(),
        list_offsets.len(),
        hitbox_offsets.len(),
    );
}
