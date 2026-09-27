use super::*;
use crate::inspect::{Document, expand, inspect};

fn sample() -> Vec<u8> {
    let mut bytes = vec![0; 144 + 178 * SPECIES_STRIDE];
    bytes[..4].copy_from_slice(&96u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&144u32.to_le_bytes());
    bytes[100] = 178;
    bytes
}

fn emd_root(document: &Document) -> usize {
    document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Emd)
        .unwrap()
}

fn species_node(document: &Document, species: u8) -> usize {
    document.nodes[emd_root(document)]
        .children
        .iter()
        .copied()
        .find(|&node| document.nodes[node].kind == Kind::EmdSpecies(species))
        .unwrap()
}

fn global_group(document: &Document) -> usize {
    *document.nodes[emd_root(document)].children.last().unwrap()
}

fn global_table_node(document: &Document, slot: usize) -> usize {
    document.nodes[global_group(document)]
        .children
        .iter()
        .copied()
        .find(|&node| {
            document.nodes[node].kind == Kind::EmdTable(slot, None)
                || document.nodes[node].kind == Kind::EmdGlobalTable(slot)
        })
        .unwrap()
}

fn descendants(document: &Document, parent: usize) -> Vec<usize> {
    let mut pending = document.nodes[parent].children.clone();
    let mut result = Vec::new();
    while let Some(node) = pending.pop() {
        pending.extend_from_slice(&document.nodes[node].children);
        result.push(node);
    }
    result
}

fn record_node(
    document: &Document,
    parent: usize,
    kind: RecordKind,
    range: std::ops::Range<usize>,
) -> usize {
    descendants(document, parent)
        .into_iter()
        .find(|&node| {
            document.nodes[node].kind == Kind::EmdRecord(kind)
                && document.nodes[node].range == range
        })
        .expect("record is reachable through the selected view")
}

fn expand_species(document: &Document, species: u8) -> Document {
    expand(document, species_node(document, species)).unwrap()
}

fn put32(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn navigation_sample() -> (Vec<u8>, usize, usize, usize) {
    let mut bytes = sample();
    let fixed = bytes.len();
    bytes.resize(fixed + 178 * 52, 0);
    put32(&mut bytes, 2 * 4, fixed);
    bytes[fixed + 52 + 32..fixed + 52 + 36].copy_from_slice(&0.25f32.to_le_bytes());

    let directory = bytes.len();
    let profiles = directory + 48;
    bytes.resize(profiles + 178 * 80, 0);
    put32(&mut bytes, 4 * 4, directory);
    put32(&mut bytes, directory, profiles);
    put32(&mut bytes, directory + 4, profiles);
    put32(&mut bytes, directory + 8, u32::MAX as usize);
    bytes[profiles + 80..profiles + 82].copy_from_slice(&100i16.to_le_bytes());

    let modifiers = bytes.len();
    bytes.resize(modifiers + 3 * 28, 0);
    put32(&mut bytes, 13 * 4, modifiers);
    bytes[96 + 18..96 + 20].copy_from_slice(&3u16.to_le_bytes());
    for (index, species, selector) in [(0, 1u16, -1i16), (1, 2, 3), (2, 1, 4)] {
        let at = modifiers + index * 28;
        bytes[at..at + 2].copy_from_slice(&species.to_le_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&selector.to_le_bytes());
    }
    (bytes, fixed, profiles, modifiers)
}

#[test]
fn navigation_lists_species_directly_and_retains_global_data_and_root_offsets() {
    let document = inspect("mhfemd.bin", sample().into());
    let root = emd_root(&document);
    let children = &document.nodes[root].children;
    assert_eq!(children.len(), 179);
    for (id, &node) in children[..178].iter().enumerate() {
        assert_eq!(document.nodes[node].kind, Kind::EmdSpecies(id as u8));
        assert!(document.nodes[node].deferred);
        assert!(document.nodes[node].children.is_empty());
    }
    let global = global_group(&document);
    assert_eq!(document.nodes[global].kind, Kind::EmdGroup);
    for slot in [0, 1, 4, 6, 8, 10, 14, 15, 20, 23] {
        let node = global_table_node(&document, slot);
        assert_eq!(document.nodes[node].kind, Kind::EmdTable(slot, None));
    }
    assert!(document.nodes.iter().all(|node| !matches!(
        node.kind,
        Kind::EmdTable(2 | 3 | 5 | 7 | 9 | 11..=13 | 16..=19 | 21..=22, None)
    )));
    for slot in 0..24 {
        assert!(
            document.nodes[root]
                .fields
                .iter()
                .any(|field| { field.writable && field.binding.range == (slot * 4..slot * 4 + 4) })
        );
    }
}

#[test]
fn species_navigation_selects_direct_profile_and_conditional_records() {
    let (bytes, fixed, profiles, modifiers) = navigation_sample();
    let document = inspect("mhfemd.bin", bytes.into());
    let document = expand_species(&document, 1);
    let species = species_node(&document, 1);
    let direct = record_node(
        &document,
        species,
        RecordKind::FixedParameters,
        fixed + 52..fixed + 104,
    );
    assert!(document.nodes[direct].deferred);
    let children = descendants(&document, species);
    assert!(!children.iter().any(|&node| {
        document.nodes[node].kind == Kind::EmdRecord(RecordKind::FixedParameters)
            && document.nodes[node].range == (fixed + 104..fixed + 156)
    }));
    assert_eq!(
        children
            .iter()
            .filter(|&&node| {
                document.nodes[node].kind == Kind::EmdRecord(RecordKind::Parameters80)
                    && document.nodes[node].range == (profiles + 80..profiles + 160)
            })
            .count(),
        2
    );
    let mut conditional: Vec<_> = children
        .iter()
        .filter_map(|&node| {
            (document.nodes[node].kind == Kind::EmdRecord(RecordKind::Modifiers))
                .then_some(document.nodes[node].range.clone())
        })
        .collect();
    conditional.sort_by_key(|range| range.start);
    assert_eq!(
        conditional,
        [modifiers..modifiers + 28, modifiers + 56..modifiers + 84]
    );

    // Another species stays lazy until selected, then gets its own matching row.
    let other = species_node(&document, 2);
    assert!(document.nodes[other].children.is_empty());
    let document = expand(&document, other).unwrap();
    record_node(
        &document,
        other,
        RecordKind::Modifiers,
        modifiers + 28..modifiers + 56,
    );
}

#[test]
fn shared_default_edits_are_visible_in_species_and_global_views() {
    let mut bytes = sample();
    let offset = bytes.len();
    bytes.resize(offset + 3 * 32, 0);
    put32(&mut bytes, 19 * 4, offset);
    bytes[96 + 28..96 + 30].copy_from_slice(&3u16.to_le_bytes());
    for (index, species) in [0u16, 1, 250].into_iter().enumerate() {
        let at = offset + index * 32;
        bytes[at + 2..at + 4].copy_from_slice(&species.to_le_bytes());
    }
    bytes[offset + 6..offset + 8].copy_from_slice(&5i16.to_le_bytes());
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let document = expand_species(&expand_species(&document, 2), 3);
    let global = global_table_node(&document, 19);
    assert_eq!(document.nodes[global].kind, Kind::EmdGlobalTable(19));
    let document = expand(&document, global).unwrap();
    let global_records = &document.nodes[global].children;
    assert_eq!(global_records.len(), 2);
    assert!(
        global_records
            .iter()
            .all(|&node| document.nodes[node].range != (offset + 32..offset + 64))
    );
    record_node(
        &document,
        global,
        RecordKind::SpeciesAssociation,
        offset + 64..offset + 96,
    );
    let parents = [
        species_node(&document, 2),
        species_node(&document, 3),
        global,
    ];
    let records = parents.map(|parent| {
        record_node(
            &document,
            parent,
            RecordKind::SpeciesAssociation,
            offset..offset + 32,
        )
    });
    assert_ne!(records[0], records[1]);
    assert_ne!(records[0], records[2]);
    for &record in &records {
        assert_eq!(
            document.nodes[record].buffer,
            document.nodes[records[0]].buffer
        );
    }
    let document = expand(&document, records[0]).unwrap();
    let field = document.nodes[records[0]]
        .fields
        .iter()
        .find(|field| field.binding.range == (offset + 6..offset + 8))
        .unwrap();
    let patch = field.write(&document.buffers, "-7").unwrap().unwrap();
    let edited = crate::edit::apply(
        &document,
        patch.binding.buffer,
        patch.binding.range,
        &patch.after,
    )
    .unwrap();
    bytes[offset + 6..offset + 8].copy_from_slice(&(-7i16).to_le_bytes());
    assert_eq!(edited.buffers[0].as_ref(), bytes);
    let edited = expand_species(&expand_species(&edited, 2), 3);
    let global = global_table_node(&edited, 19);
    let edited = expand(&edited, global).unwrap();
    for parent in [species_node(&edited, 2), species_node(&edited, 3), global] {
        let record = record_node(
            &edited,
            parent,
            RecordKind::SpeciesAssociation,
            offset..offset + 32,
        );
        let expanded = expand(&edited, record).unwrap();
        let field = expanded.nodes[record]
            .fields
            .iter()
            .find(|field| field.binding.range == (offset + 6..offset + 8))
            .unwrap();
        assert_eq!(field.read(&expanded.buffers).unwrap(), "-7");
    }
}

#[test]
fn weighted_lists_expand_with_signed_fields_and_preserve_the_terminator() {
    let mut bytes = sample();
    let directory = bytes.len();
    let records = directory + 12;
    bytes.resize(records + 5, 0);
    bytes[24..28].copy_from_slice(&(directory as u32).to_le_bytes());
    for index in 0..3 {
        let at = directory + index * 4;
        bytes[at..at + 4].copy_from_slice(&(records as u32).to_le_bytes());
    }
    bytes[records..].copy_from_slice(&[10, 0xfe, 20, 7, 0xff]);
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let root = global_table_node(&document, 6);
    let document = expand(&document, root).unwrap();
    let entry = document.nodes[root].children[0];
    let target = document.nodes[entry].children[0];
    let marker = document.nodes[target]
        .fields
        .iter()
        .find(|field| field.name == "终止标记")
        .unwrap();
    assert_eq!(marker.binding.range, records + 4..records + 5);
    assert_eq!(marker.read(&document.buffers).unwrap(), "FF");
    let document = expand(&document, target).unwrap();
    assert_eq!(document.nodes[target].children.len(), 2);
    let record = document.nodes[target].children[0];
    let document = expand(&document, record).unwrap();
    let value = document.nodes[record]
        .fields
        .iter()
        .find(|field| field.binding.range == (records + 1..records + 2))
        .unwrap();
    assert_eq!(value.value, "-2");
    let patch = value.write(&document.buffers, "-3").unwrap().unwrap();
    let edited = crate::edit::apply(
        &document,
        patch.binding.buffer,
        patch.binding.range.clone(),
        &patch.after,
    )
    .unwrap();
    bytes[records + 1] = 0xfd;
    assert_eq!(&edited.buffers[0][..], bytes);
}

#[test]
fn species_views_expand_shared_parameters_and_isolate_invalid_links() {
    let mut bytes = sample();
    let parameters = bytes.len();
    let anger = parameters + 16_000;
    bytes.resize(anger + 60, 0);
    for id in [1, 2] {
        let at = 144 + id * SPECIES_STRIDE;
        bytes[at + 176..at + 180].copy_from_slice(&(parameters as u32).to_le_bytes());
        bytes[at + 72..at + 76].copy_from_slice(&(anger as u32).to_le_bytes());
    }
    let bad = 144 + SPECIES_STRIDE + 76;
    bytes[bad..bad + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[anger..anger + 2].copy_from_slice(&(-100i16).to_le_bytes());
    let document = inspect("mhfemd.bin", bytes.clone().into());
    assert!(
        document
            .nodes
            .iter()
            .all(|node| !matches!(node.kind, Kind::EmdSpeciesTable(..)))
    );
    let document = expand_species(&expand_species(&document, 1), 2);
    let find = |kind| {
        document
            .nodes
            .iter()
            .position(|node| node.kind == kind)
            .unwrap()
    };
    let bank = find(Kind::EmdSpeciesTable(1, SpeciesTable::ParameterBank(1)));
    let alias = find(Kind::EmdSpeciesTable(2, SpeciesTable::ParameterBank(1)));
    assert_eq!(document.nodes[bank].range, document.nodes[alias].range);
    assert_eq!(
        document.nodes[bank].range,
        parameters + 8000..parameters + 16_000
    );
    let invalid = find(Kind::EmdSpeciesTable(1, SpeciesTable::AngerProfile(1)));
    assert!(document.nodes[invalid].error.is_some());
    let profile = find(Kind::EmdSpeciesTable(1, SpeciesTable::AngerProfile(0)));
    let document = expand(&document, bank).unwrap();
    assert_eq!(document.nodes[bank].children.len(), 200);
    let document = expand(&document, profile).unwrap();
    let record = document.nodes[profile].children[0];
    let document = expand(&document, record).unwrap();
    let threshold = document.nodes[record]
        .fields
        .iter()
        .find(|field| field.name == "anger_threshold")
        .unwrap();
    assert_eq!(threshold.value, "-100");
    assert_eq!(threshold.binding.range, anger..anger + 2);
    let edited =
        crate::edit::apply(&document, 0, anger..anger + 2, &(-101i16).to_le_bytes()).unwrap();
    bytes[anger..anger + 2].copy_from_slice(&(-101i16).to_le_bytes());
    assert_eq!(&edited.buffers[0][..], bytes);
    let edited = expand_species(&edited, 2);
    let alias = descendants(&edited, species_node(&edited, 2))
        .into_iter()
        .find(|&node| {
            edited.nodes[node].kind == Kind::EmdSpeciesTable(2, SpeciesTable::AngerProfile(0))
        })
        .unwrap();
    let edited = expand(&edited, alias).unwrap();
    let record = edited.nodes[alias].children[0];
    let edited = expand(&edited, record).unwrap();
    assert_eq!(
        edited.nodes[record]
            .fields
            .iter()
            .find(|field| field.name == "anger_threshold")
            .unwrap()
            .read(&edited.buffers)
            .unwrap(),
        "-101"
    );
}

#[test]
fn species_parameter_directories_preserve_aliases_and_allow_scalar_edits() {
    let mut bytes = sample();
    let offset = bytes.len();
    bytes.resize(offset + 1600, 0);
    for id in [1, 2] {
        let link = 144 + id * SPECIES_STRIDE + 184;
        bytes[link..link + 4].copy_from_slice(&(offset as u32).to_le_bytes());
    }
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let document = expand_species(&expand_species(&document, 1), 2);
    let directories: Vec<_> = document
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| matches!(node.kind, Kind::EmdTable(3, Some(_))))
        .map(|(index, node)| (index, node.range.clone()))
        .collect();
    assert_eq!(directories.len(), 2);
    assert_eq!(directories[0].1, directories[1].1);
    let directory = directories[0].0;
    let document = expand(&document, directory).unwrap();
    assert_eq!(document.nodes[directory].children.len(), 200);
    let record = document.nodes[directory].children[0];
    let document = expand(&document, record).unwrap();
    let field = document.nodes[record]
        .fields
        .iter()
        .find(|field| field.name == "value_04")
        .unwrap();
    assert_eq!(field.binding.range, offset + 4..offset + 8);
    let edited = crate::edit::apply(
        &document,
        document.nodes[record].buffer,
        field.binding.range.clone(),
        &u32::MAX.to_le_bytes(),
    )
    .unwrap();
    bytes[offset + 4..offset + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(&edited.buffers[0][..], bytes);
}

#[test]
fn named_resource_uses_count_and_preserves_unknown_species() {
    let document = inspect("mhfemd.bin", sample().into());
    assert_eq!(document.nodes[0].kind, Kind::Emd);
    assert_eq!(
        document.nodes[0]
            .children
            .iter()
            .filter(|&&node| matches!(document.nodes[node].kind, Kind::EmdSpecies(_)))
            .count(),
        178
    );
    let first = species_node(&document, 1);
    assert!(document.nodes[first].name.contains("雌火龙"));
    let last = species_node(&document, 177);
    assert!(document.nodes[last].name.contains("em177"));
    let unnamed = species_node(&document, 18);
    assert!(document.nodes[unnamed].name.contains("em018"));
    let expanded = expand(&document, last).unwrap();
    assert!(
        expanded.nodes[last]
            .fields
            .iter()
            .any(|field| field.binding.range.start == 144 + 177 * SPECIES_STRIDE)
    );
}

#[test]
fn encoded_resource_can_be_edited_without_losing_unknown_bytes() {
    use mhf_resource::crypto::Ecd;
    let bytes = sample();
    let mut jkr = b"JKR\x1a\x08\x01\0\0".to_vec();
    jkr.extend_from_slice(&16u32.to_le_bytes());
    jkr.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    jkr.extend_from_slice(&bytes);
    let encoded = Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
        .unwrap()
        .encode(&jkr, Some(b"mhfemd.bin"))
        .unwrap();
    let document = inspect("mhfemd.bin", encoded.into());
    let child = species_node(&document, 1);
    let document = expand(&document, child).unwrap();
    let field = document.nodes[child]
        .fields
        .iter()
        .find(|field| field.binding.range.len() == 4)
        .unwrap();
    let range = field.binding.range.clone();
    let buffer = document.nodes[child].buffer;
    let edited =
        crate::edit::apply(&document, buffer, range.clone(), &123u32.to_le_bytes()).unwrap();
    let root = edited
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Emd)
        .unwrap();
    let mut expected = bytes;
    expected[range].copy_from_slice(&123u32.to_le_bytes());
    assert_eq!(&edited.buffers[root.buffer][root.range.clone()], expected);
    assert_eq!(
        root.children
            .iter()
            .filter(|&&node| matches!(edited.nodes[node].kind, Kind::EmdSpecies(_)))
            .count(),
        178
    );
}

#[test]
fn nested_species_edit_preserves_nonzero_base_and_both_envelopes() {
    use mhf_resource::crypto::Ecd;

    let (bytes, fixed, _, _) = navigation_sample();
    let base = 64;
    let mut archive = vec![0; base];
    archive[..4].copy_from_slice(b"mha\x01");
    for (at, value) in [
        (4, 24),
        (8, 1),
        (12, 44),
        (16, 11),
        (20, 739 | (1 << 16)),
        (28, base),
        (32, bytes.len()),
        (36, bytes.len() + 3),
        (40, 739),
    ] {
        put32(&mut archive, at, value);
    }
    archive[44..55].copy_from_slice(b"mhfemd.bin\0");
    archive.extend_from_slice(&bytes);
    archive.extend_from_slice(&[0xde, 0xad, 0xef]);
    let mut jkr = b"JKR\x1a\x08\x01\0\0".to_vec();
    jkr.extend_from_slice(&16u32.to_le_bytes());
    jkr.extend_from_slice(&(archive.len() as u32).to_le_bytes());
    jkr.extend_from_slice(&archive);
    let encoded = Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
        .unwrap()
        .encode(&jkr, Some(b"bundle.mha"))
        .unwrap();

    let document = inspect("bundle.mha", encoded.into());
    assert_eq!(document.nodes[emd_root(&document)].range.start, base);
    let document = expand_species(&document, 1);
    let aggregate = record_node(
        &document,
        species_node(&document, 1),
        RecordKind::FixedParameters,
        base + fixed + 52..base + fixed + 104,
    );
    let document = expand(&document, aggregate).unwrap();
    let field_range = base + fixed + 52 + 32..base + fixed + 52 + 36;
    let field = document.nodes[aggregate]
        .fields
        .iter()
        .find(|field| field.binding.range == field_range)
        .unwrap();
    assert_ne!(field.binding.buffer, 0);
    let patch = field.write(&document.buffers, "0.75").unwrap().unwrap();
    let edited = crate::edit::apply(
        &document,
        patch.binding.buffer,
        patch.binding.range,
        &patch.after,
    )
    .unwrap();
    archive[field_range].copy_from_slice(&0.75f32.to_le_bytes());
    let reopened =
        mhf_resource::container::open_layers(&edited.buffers[0], 64 * 1024 * 1024, 8).unwrap();
    assert_eq!(reopened.layers.len(), 2);
    assert_eq!(reopened.payload(), archive);
    assert_eq!(
        edited.nodes[emd_root(&edited)].range,
        base..base + bytes.len()
    );
}

#[test]
fn species_profile_aliases_edit_original_bytes_and_global_index_stays_editable() {
    let mut bytes = sample();
    let directory = bytes.len();
    let records = directory + 48;
    bytes.resize(records + 178 * 34, 0);
    put32(&mut bytes, 4, directory);
    for entry in 0..12 {
        put32(&mut bytes, directory + 4 * entry, records);
    }
    bytes[records + 34..records + 36].copy_from_slice(&321i16.to_le_bytes());
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let species = species_node(&document, 1);
    assert!(document.nodes[species].deferred);
    assert!(document.nodes[species].children.is_empty());
    let document = expand(&document, species).unwrap();
    let aliases: Vec<_> = descendants(&document, species)
        .into_iter()
        .filter(|&node| {
            document.nodes[node].range == (records + 34..records + 68)
                && document.nodes[node].kind == Kind::EmdRecord(RecordKind::PartParameters)
        })
        .collect();
    assert_eq!(aliases.len(), 12);
    assert!(
        aliases
            .iter()
            .all(|&node| document.nodes[node].deferred && document.nodes[node].children.is_empty())
    );
    let table = global_table_node(&document, 1);
    let document = expand(&document, table).unwrap();
    assert_eq!(document.nodes[table].children.len(), 12);
    for &pointer in &document.nodes[table].children {
        assert_eq!(
            document.nodes[pointer].kind,
            Kind::EmdRecord(RecordKind::Pointers)
        );
        assert!(document.nodes[pointer].children.is_empty());
    }
    let pointer = document.nodes[table].children[0];
    let document = expand(&document, pointer).unwrap();
    assert!(
        document.nodes[pointer]
            .fields
            .iter()
            .any(|field| field.writable && field.binding.range == (directory..directory + 4))
    );
    let record = aliases[0];
    let document = expand(&document, record).unwrap();
    let field = document.nodes[record]
        .fields
        .iter()
        .find(|field| field.name == "part_0_initial_value")
        .unwrap();
    assert_eq!(field.binding.range, records + 34..records + 36);
    assert_eq!(field.value, "321");
    let patch = field.write(&document.buffers, "456").unwrap().unwrap();
    let edited = crate::edit::apply(
        &document,
        patch.binding.buffer,
        patch.binding.range,
        &patch.after,
    )
    .unwrap();
    bytes[records + 34..records + 36].copy_from_slice(&456i16.to_le_bytes());
    assert_eq!(edited.buffers[0].as_ref(), bytes);
    let edited = expand_species(&edited, 1);
    let aliases: Vec<_> = descendants(&edited, species_node(&edited, 1))
        .into_iter()
        .filter(|&node| {
            edited.nodes[node].kind == Kind::EmdRecord(RecordKind::PartParameters)
                && edited.nodes[node].range == (records + 34..records + 68)
        })
        .collect();
    assert_eq!(aliases.len(), 12);
    for record in aliases {
        let expanded = expand(&edited, record).unwrap();
        let field = expanded.nodes[record]
            .fields
            .iter()
            .find(|field| field.name == "part_0_initial_value")
            .unwrap();
        assert_eq!(field.read(&expanded.buffers).unwrap(), "456");
    }
}

#[test]
fn association_action_rules_expand_and_edit_without_normalizing_values() {
    let mut bytes = sample();
    let association = bytes.len();
    let rules = association + 32;
    bytes.resize(rules + 4, 0);
    bytes[76..80].copy_from_slice(&(association as u32).to_le_bytes());
    bytes[124..126].copy_from_slice(&1u16.to_le_bytes());
    bytes[association + 26..association + 28].copy_from_slice(&1u16.to_le_bytes());
    bytes[association + 28..association + 32].copy_from_slice(&(rules as u32).to_le_bytes());
    bytes[rules..].copy_from_slice(&[7, 2, 44, 1]);
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let root = global_table_node(&document, 19);
    let document = expand(&document, root).unwrap();
    let association_node = document.nodes[root].children[0];
    let target = document.nodes[association_node].children[0];
    assert_eq!(document.nodes[target].range, rules..rules + 4);
    let document = expand(&document, target).unwrap();
    let record = document.nodes[target].children[0];
    let document = expand(&document, record).unwrap();
    let field = document.nodes[record]
        .fields
        .iter()
        .find(|f| f.name == "action_id")
        .unwrap();
    assert_eq!(field.value, "300");
    let edited = crate::edit::apply(
        &document,
        document.nodes[record].buffer,
        field.binding.range.clone(),
        &301u16.to_le_bytes(),
    )
    .unwrap();
    bytes[rules + 2..rules + 4].copy_from_slice(&301u16.to_le_bytes());
    assert_eq!(edited.buffers[0].as_ref(), bytes);
}

#[test]
fn global_group_index_keeps_empty_links_and_unassigned_group_records() {
    let mut bytes = sample();
    let counts = bytes.len();
    let directory = counts + 4;
    let records = directory + 8;
    bytes.resize(records + 64, 0);
    put32(&mut bytes, 15 * 4, counts);
    put32(&mut bytes, 16 * 4, directory);
    bytes[96 + 22..96 + 24].copy_from_slice(&2u16.to_le_bytes());
    bytes[counts + 2..counts + 4].copy_from_slice(&2u16.to_le_bytes());
    put32(&mut bytes, directory, records);
    put32(&mut bytes, directory + 4, records);
    bytes[records + 16..records + 18].copy_from_slice(&(-1i16).to_le_bytes());
    bytes[records + 48..records + 50].copy_from_slice(&1i16.to_le_bytes());
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let document = expand_species(&document, 1);
    record_node(
        &document,
        species_node(&document, 1),
        RecordKind::GroupRecord,
        records + 32..records + 64,
    );
    let unassigned = global_table_node(&document, 16);
    let document = expand(&document, unassigned).unwrap();
    assert_eq!(document.nodes[unassigned].children.len(), 1);
    record_node(
        &document,
        unassigned,
        RecordKind::GroupRecord,
        records..records + 32,
    );
    let index = global_table_node(&document, 15);
    let document = expand(&document, index).unwrap();
    assert_eq!(document.nodes[index].children.len(), 2);
    let empty = document.nodes[index].children[0];
    let document = expand(&document, empty).unwrap();
    assert!(document.nodes[empty].children.is_empty());
    let count = document.nodes[empty]
        .fields
        .iter()
        .find(|field| field.binding.range == (counts..counts + 2))
        .unwrap();
    assert_eq!(count.read(&document.buffers).unwrap(), "0");
    let pointer = document.nodes[empty]
        .fields
        .iter()
        .find(|field| field.binding.range == (directory..directory + 4))
        .unwrap();
    assert!(pointer.writable);
    let patch = pointer
        .write(&document.buffers, &(records + 32).to_string())
        .unwrap()
        .unwrap();
    let edited = crate::edit::apply(
        &document,
        patch.binding.buffer,
        patch.binding.range,
        &patch.after,
    )
    .unwrap();
    put32(&mut bytes, directory, records + 32);
    assert_eq!(edited.buffers[0].as_ref(), bytes);
}

#[test]
fn keyed_scripts_belong_to_species_and_unknown_keys_stay_global() {
    let mut bytes = sample();
    let records = bytes.len();
    let scripts = records + 24;
    bytes.resize(scripts + 6, 0);
    put32(&mut bytes, 17 * 4, records);
    bytes[96 + 24..96 + 26].copy_from_slice(&3u16.to_le_bytes());
    for (index, species) in [1, 1, 250].into_iter().enumerate() {
        let at = records + index * 8;
        bytes[at] = species;
        bytes[at + 1] = index as u8;
        bytes[at + 2] = 10 + index as u8;
        put32(&mut bytes, at + 4, scripts + index * 2);
        bytes[scripts + index * 2..scripts + index * 2 + 2].copy_from_slice(&[0xff, 2]);
    }
    let document = inspect("mhfemd.bin", bytes.clone().into());
    let document = expand_species(&expand_species(&document, 1), 2);
    let global = global_table_node(&document, 17);
    let document = expand(&document, global).unwrap();
    assert_eq!(document.nodes[global].kind, Kind::EmdGlobalTable(17));
    assert_eq!(document.nodes[global].children.len(), 1);
    let orphan = record_node(
        &document,
        global,
        RecordKind::PointerRecord,
        records + 16..records + 24,
    );
    assert_eq!(
        document.bytes(orphan).unwrap(),
        &bytes[records + 16..records + 24]
    );
    let species = species_node(&document, 1);
    let associated: Vec<_> = descendants(&document, species)
        .into_iter()
        .filter(|&node| document.nodes[node].kind == Kind::EmdRecord(RecordKind::PointerRecord))
        .collect();
    assert_eq!(associated.len(), 2);
    for index in 0..2 {
        let record = record_node(
            &document,
            species,
            RecordKind::PointerRecord,
            records + index * 8..records + (index + 1) * 8,
        );
        let script = document.nodes[record].children[0];
        assert_eq!(document.nodes[script].kind, Kind::EmdAiScript(index));
        let expanded = expand(&document, script).unwrap();
        assert!(expanded.nodes[script].error.is_none());
        assert_eq!(expanded.bytes(script).unwrap(), [0xff, 2]);
        assert_eq!(
            expanded.bytes(record).unwrap(),
            &bytes[records + index * 8..records + (index + 1) * 8]
        );
    }
    assert!(
        !descendants(&document, species_node(&document, 2))
            .into_iter()
            .any(|node| document.nodes[node].kind == Kind::EmdRecord(RecordKind::PointerRecord))
    );
}

fn assert_script_matches_table(document: &Document, script: usize, slot: usize, index: usize) {
    let root = &document.nodes[emd_root(document)];
    let file = Emd::parse(&document.buffers[root.buffer][root.range.clone()]).unwrap();
    let table = file.root_table(slot).unwrap().unwrap();
    let (_, record) = table.record(index).unwrap();
    let pointer = if slot == 9 { 0 } else { 4 };
    let offset = u32::from_le_bytes(record[pointer..pointer + 4].try_into().unwrap()) as usize;
    let node = &document.nodes[script];
    assert_eq!(node.buffer, root.buffer);
    assert_eq!(node.range.start, root.range.start + offset);
    assert!(node.error.is_none());
    let bytes = &file.as_bytes()[offset..offset + node.range.len()];
    assert_eq!(document.bytes(script).unwrap(), bytes);
    let decoded = mhf_monster::ai::bytecode::decode(bytes).unwrap();
    assert!(!decoded.is_empty());
    assert_eq!(node.children.len(), decoded.len());
    for (&child, instruction) in node.children.iter().zip(decoded) {
        assert_eq!(
            document.nodes[child].range.start,
            node.range.start + instruction.offset
        );
        assert_eq!(document.bytes(child).unwrap(), instruction.bytes);
    }
}

#[test]
#[ignore = "requires local game resource via MHF_EMD_PATH"]
fn real_emd_species_ai_links_match_source_tables() {
    let path = std::env::var_os("MHF_EMD_PATH").expect("set MHF_EMD_PATH");
    let document = inspect("mhfemd.bin", std::fs::read(path).unwrap().into());
    let document = expand_species(&document, 146);
    let mut candidates: Vec<_> = descendants(&document, species_node(&document, 146))
        .into_iter()
        .filter(|&node| document.nodes[node].kind == Kind::EmdRecord(RecordKind::Pointers))
        .flat_map(|node| document.nodes[node].children.iter().copied())
        .filter_map(|node| match document.nodes[node].kind {
            Kind::EmdAiScript(index) => Some((index, node)),
            _ => None,
        })
        .collect();
    candidates.sort_by_key(|&(index, _)| index);
    assert_eq!(
        candidates
            .iter()
            .map(|&(index, _)| index)
            .collect::<Vec<_>>(),
        [185, 186, 187, 188, 189, 190, 272, 273]
    );
    let candidate = candidates[0].1;
    let document = expand(&document, candidate).unwrap();
    assert_script_matches_table(&document, candidate, 9, 185);
    let global = global_table_node(&document, 9);
    let document = expand(&document, global).unwrap();
    assert!(
        !descendants(&document, global)
            .into_iter()
            .any(|node| document.nodes[node].kind == Kind::EmdAiScript(185))
    );

    let root = &document.nodes[emd_root(&document)];
    let file = Emd::parse(&document.buffers[root.buffer][root.range.clone()]).unwrap();
    let table = file.root_table(17).unwrap().unwrap();
    let (index, species, range, original) = (0..table.count)
        .find_map(|index| {
            let (at, bytes) = table.record(index).unwrap();
            (bytes[0] < file.species_count()).then(|| {
                (
                    index,
                    bytes[0],
                    root.range.start + at..root.range.start + at + bytes.len(),
                    bytes.to_vec(),
                )
            })
        })
        .expect("root 17 has a record for a declared species");
    let document = expand_species(&document, species);
    let associated = record_node(
        &document,
        species_node(&document, species),
        RecordKind::PointerRecord,
        range.clone(),
    );
    let script = document.nodes[associated]
        .children
        .iter()
        .copied()
        .find(|&node| document.nodes[node].kind == Kind::EmdAiScript(index))
        .unwrap();
    let document = expand(&document, script).unwrap();
    assert_script_matches_table(&document, script, 17, index);
    let document = expand(&document, associated).unwrap();
    assert_eq!(document.bytes(associated).unwrap(), original);
    for (offset, expected) in [(1, original[1]), (2, original[2])] {
        let field = document.nodes[associated]
            .fields
            .iter()
            .find(|field| field.binding.range == (range.start + offset..range.start + offset + 1))
            .unwrap();
        assert_eq!(field.read(&document.buffers).unwrap(), expected.to_string());
    }
    if let Some(global) = document.nodes[global_group(&document)]
        .children
        .iter()
        .copied()
        .find(|&node| document.nodes[node].kind == Kind::EmdGlobalTable(17))
    {
        let document = expand(&document, global).unwrap();
        assert!(!descendants(&document, global).into_iter().any(|node| {
            document.nodes[node].kind == Kind::EmdRecord(RecordKind::PointerRecord)
                && document.nodes[node].range == range
        }));
    }
}

#[test]
#[ignore = "requires local game resource via MHF_EMD_PATH"]
fn real_emd_new_views_preserve_envelopes_and_bytes() {
    let path = std::env::var_os("MHF_EMD_PATH").expect("set MHF_EMD_PATH");
    let mut document = inspect("mhfemd.bin", std::fs::read(path).unwrap().into());
    let mut fields = Vec::new();
    for (slot, index) in [(6, 0), (7, 0), (9, 185), (12, 146), (17, 0)] {
        let entry = if slot == 6 {
            let root = global_table_node(&document, slot);
            document = expand(&document, root).unwrap();
            document.nodes[root].children[index]
        } else {
            let root = &document.nodes[emd_root(&document)];
            let file = Emd::parse(&document.buffers[root.buffer][root.range.clone()]).unwrap();
            let table = file.root_table(slot).unwrap().unwrap();
            let (at, bytes) = table.record(index).unwrap();
            let species = if matches!(slot, 9 | 12) {
                146
            } else {
                bytes[0]
            };
            assert!(species < file.species_count());
            let range = root.range.start + at..root.range.start + at + bytes.len();
            let kind = table.kind;
            document = expand_species(&document, species);
            record_node(&document, species_node(&document, species), kind, range)
        };
        let record = if matches!(slot, 6 | 7) {
            let target = document.nodes[entry].children[0];
            document = expand(&document, target).unwrap();
            document.nodes[target].children[0]
        } else if matches!(slot, 9 | 17) {
            let target = document.nodes[entry].children[0];
            document = expand(&document, target).unwrap();
            assert!(document.nodes[target].error.is_none());
            *document.nodes[target]
                .children
                .iter()
                .find(|&&node| document.bytes(node).unwrap().first() == Some(&0x82))
                .expect("known script calls a secondary subscript")
        } else {
            entry
        };
        document = expand(&document, record).unwrap();
        let field = document.nodes[record]
            .fields
            .iter()
            .find(|field| field.writable && (!matches!(slot, 9 | 17) || field.name == "operand_02"))
            .unwrap();
        fields.push(field.clone());
    }
    for kind in [
        SpeciesTable::ParameterBank(1),
        SpeciesTable::AngerProfile(0),
    ] {
        let root = emd_root(&document);
        let resource = &document.nodes[root];
        let file = Emd::parse(&document.buffers[resource.buffer][resource.range.clone()]).unwrap();
        let species = file
            .species()
            .find(|species| {
                file.species_table(species.id, kind)
                    .is_ok_and(|table| table.is_some())
            })
            .unwrap()
            .id;
        document = expand_species(&document, species);
        let table = document
            .nodes
            .iter()
            .position(|node| {
                node.kind == Kind::EmdSpeciesTable(species, kind) && node.error.is_none()
            })
            .unwrap();
        document = expand(&document, table).unwrap();
        let record = document.nodes[table].children[0];
        document = expand(&document, record).unwrap();
        fields.push(
            document.nodes[record]
                .fields
                .iter()
                .find(|field| field.writable && !field.name.starts_with("raw_"))
                .unwrap()
                .clone(),
        );
    }
    for field in fields {
        let buffer = field.binding.buffer;
        let range = field.binding.range.clone();
        let mut expected = document.buffers[buffer].to_vec();
        let mut replacement = expected[range.clone()].to_vec();
        replacement[0] ^= 1;
        expected[range.clone()].copy_from_slice(&replacement);
        let edited = crate::edit::apply(&document, buffer, range, &replacement).unwrap();
        let reopened =
            mhf_resource::container::open_layers(&edited.buffers[0], 64 * 1024 * 1024, 8).unwrap();
        assert_eq!(reopened.payload(), expected, "field {}", field.name);
        assert!(!reopened.layers.is_empty());
    }
}

#[test]
#[ignore = "requires local game resource via MHF_EMD_PATH"]
fn real_emd_workbench_edit_preserves_payload_and_envelopes() {
    let path = std::env::var_os("MHF_EMD_PATH").expect("set MHF_EMD_PATH");
    let source = std::fs::read(path).unwrap();
    let document = inspect("mhfemd.bin", source.into());
    let root = document
        .nodes
        .iter()
        .position(|n| n.kind == Kind::Emd)
        .unwrap();
    assert!(document.nodes[root].error.is_none());
    let table = global_table_node(&document, 19);
    let document = expand(&document, table).unwrap();
    let record = document.nodes[table].children[0];
    let document = expand(&document, record).unwrap();
    let field = document.nodes[record]
        .fields
        .iter()
        .find(|f| f.name == "anchor_offset_x")
        .unwrap();
    let buffer = document.nodes[record].buffer;
    let range = field.binding.range.clone();
    let mut expected = document.buffers[buffer].to_vec();
    let original = i16::from_le_bytes(expected[range.clone()].try_into().unwrap());
    let replacement = original.wrapping_add(1).to_le_bytes();
    expected[range.clone()].copy_from_slice(&replacement);
    let edited = crate::edit::apply(&document, buffer, range, &replacement).unwrap();
    let reopened =
        mhf_resource::container::open_layers(&edited.buffers[0], 64 * 1024 * 1024, 8).unwrap();
    assert_eq!(reopened.payload(), expected);
    assert!(!reopened.layers.is_empty());
}

#[test]
fn malformed_named_resource_is_not_silently_probed_as_an_archive() {
    let document = inspect("mhfemd.bin", vec![0; 16].into());
    assert_eq!(document.nodes[0].kind, Kind::Emd);
    assert!(document.nodes[0].children.is_empty());
    assert!(document.nodes[0].error.is_some());
}
