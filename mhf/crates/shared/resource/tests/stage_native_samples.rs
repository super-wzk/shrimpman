use mhf_resource::{
    container::{SimpleArchive, StageArchive, open_layers},
    stage::{
        Hits, KEffect, Lighting, ObjectPackage, PlacementTable, RenderTables, ResourceReference,
    },
};
use std::{collections::BTreeMap, path::PathBuf};

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original stage files only"]
fn actual_hd_lighting_and_render_tables_match_the_native_layout() {
    let root = PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut files: Vec<_> = std::fs::read_dir(root.join("dat/stage-hd"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pac"))
        .collect();
    files.sort();
    let mut versions = BTreeMap::new();
    let mut tables = 0;
    let mut legacy_tables = Vec::new();
    let mut errors = Vec::new();
    for path in &files {
        let source = std::fs::read(path).unwrap();
        let opened = open_layers(&source, 512 * 1024 * 1024, 16).unwrap();
        let archive = SimpleArchive::parse(opened.payload(), 16).unwrap();
        let bytes = archive.payload(0).unwrap();
        match Lighting::probe(bytes) {
            Ok(file) => {
                *versions.entry(file.version_bits).or_insert(0usize) += 1;
                assert_eq!(file.as_bytes(), bytes);
                if path.file_name().unwrap() == "st001-hd.pac" {
                    assert_eq!(file.counts, [5, 13, 30, 0, 0]);
                    assert_eq!(file.post_process.tone_mapping.count, 3);
                    assert_eq!(bytes.len(), 2393);
                }
            }
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
        if let Some(entry) = archive.entries.get(2)
            && entry.size > 0
        {
            let bytes = entry.payload(opened.payload()).unwrap();
            if bytes.starts_with(&1u16.to_le_bytes()) {
                // 11394DA0 explicitly rejects the historical version-1 input.
                legacy_tables.push(path.file_name().unwrap().to_string_lossy().into_owned());
                assert!(RenderTables::parse(bytes).is_err());
            } else {
                match RenderTables::probe(bytes) {
                    Ok(file) => {
                        tables += 1;
                        assert_eq!(file.as_bytes(), bytes);
                    }
                    Err(error) => errors.push(format!("{} render tables: {error}", path.display())),
                }
            }
        }
    }
    eprintln!(
        "HD files={}, lighting versions={versions:x?}, render tables={tables}, legacy={legacy_tables:?}",
        files.len()
    );
    assert!(!files.is_empty());
    assert!(
        errors.is_empty(),
        "{} errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads the original SD stage corpus"]
fn actual_sd_collision_effect_placements_and_resource_links_preserve_original_sources() {
    #[derive(Default, Debug)]
    struct Counts {
        directories: usize,
        placements: usize,
        object_packages: usize,
        hits: usize,
        effect_keys: usize,
        keffects: usize,
        reference_members: usize,
        resolved_references: usize,
        unresolved_references: Vec<String>,
    }
    fn walk(source: &[u8], path: &str, depth: usize, counts: &mut Counts) {
        assert!(depth <= 16, "{path}: unexpected recursive container depth");
        let opened = open_layers(source, 512 * 1024 * 1024, 16).unwrap();
        let bytes = opened.payload();
        if bytes.starts_with(b"HITS") {
            let file = Hits::parse(bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(file.trailing.is_empty(), "{path}: HITS trailing bytes");
            assert_eq!(file.as_bytes(), bytes);
            counts.hits += 1;
            return;
        }
        if bytes.starts_with(b"KEFFECT") {
            let file = KEffect::parse(bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(file.trailing.is_empty(), "{path}: KEFFECT trailing bytes");
            assert_eq!(file.as_bytes(), bytes);
            counts.keffects += 1;
            counts.effect_keys += file.records.len();
            return;
        }
        if let Ok(directory) = StageArchive::probe(bytes, 65_536) {
            counts.directories += 1;
            let placements =
                PlacementTable::probe(directory.entries[0].entry.payload(bytes).unwrap()).unwrap();
            counts.placements += placements.placements.len();
            for resource in &directory.entries {
                if let Some(id) = resource.resource_id
                    && resource.entry.size != 0
                    && let Ok(package) =
                        ObjectPackage::probe(resource.entry.payload(bytes).unwrap(), 65_536)
                {
                    for member in &package.members {
                        if ResourceReference::has_magic(member.bytes) {
                            counts.reference_members += 1;
                            match directory.resolve_member(id, member.kind, 65_536) {
                                Ok(resolved) => {
                                    counts.resolved_references += 1;
                                    assert_eq!(
                                        resolved.bytes.as_ptr(),
                                        bytes[resolved.offset..].as_ptr()
                                    );
                                    assert!(!ResourceReference::has_magic(resolved.bytes));
                                }
                                Err(error) => counts.unresolved_references.push(format!(
                                    "{path}, resource {id}, kind {}: {error}",
                                    member.kind
                                )),
                            }
                        }
                    }
                }
                if resource.entry.size != 0 {
                    walk(
                        resource.entry.payload(bytes).unwrap(),
                        &format!("{path}/{}", resource.entry.index),
                        depth + 1,
                        counts,
                    );
                }
            }
            return;
        }
        if ObjectPackage::probe(bytes, 65_536).is_ok() {
            counts.object_packages += 1;
        }
        if let Ok(archive) = SimpleArchive::parse(bytes, 65_536) {
            for entry in &archive.entries {
                if entry.size != 0 {
                    walk(
                        entry.payload(bytes).unwrap(),
                        &format!("{path}/{}", entry.index),
                        depth + 1,
                        counts,
                    );
                }
            }
        }
    }
    let root = PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let mut files: Vec<_> = std::fs::read_dir(root.join("dat/stage"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pac"))
        .collect();
    files.sort();
    let mut counts = Counts::default();
    for path in files {
        walk(
            &std::fs::read(&path).unwrap(),
            &path.file_name().unwrap().to_string_lossy(),
            0,
            &mut counts,
        );
    }
    eprintln!("SD corpus: {counts:?}");
    assert!(counts.directories > 0 && counts.hits > 0 && counts.keffects > 0);
    assert_eq!(counts.resolved_references, counts.reference_members);
    assert!(counts.unresolved_references.is_empty());
}
