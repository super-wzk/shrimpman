use mhf_resource::{
    container::open_layers,
    emd::{Emd, SpeciesTable},
};

/// Run with MHF_EMD_PATH pointing at a local, unmodified game resource.
#[test]
#[ignore = "requires local game resource via MHF_EMD_PATH"]
fn real_emd_tables_and_nested_directories() {
    let path = std::env::var_os("MHF_EMD_PATH").expect("set MHF_EMD_PATH");
    let source = std::fs::read(path).unwrap();
    let decoded = open_layers(&source, 64 * 1024 * 1024, 8).unwrap();
    let file = Emd::parse(&decoded).unwrap();
    eprintln!(
        "decoded={} header={} species={}",
        decoded.len(),
        file.header_offset,
        file.species().count()
    );
    eprintln!(
        "header prefix: {:02x?}",
        &decoded[file.header_offset..file.header_offset + 36]
    );
    let mut failures = Vec::new();
    for slot in 0..24 {
        match file.root_table(slot) {
            Ok(Some(table)) => {
                eprintln!(
                    "root {slot}: {:?}, {} x {} {:?}",
                    table.range, table.count, table.stride, table.kind
                );
                if matches!(slot, 1 | 3 | 4 | 6 | 7 | 10 | 16 | 19) {
                    for index in 0..table.count {
                        if let Err(error) = file.directory_table(slot, index) {
                            failures.push(format!("root {slot}[{index}]: {error}"));
                        }
                    }
                }
            }
            Ok(None) => {
                let offset = file.root_offset(slot).unwrap();
                eprintln!(
                    "root {slot}: unresolved @ {offset}, prefix={:02x?}",
                    decoded.get(offset..offset + 32)
                );
            }
            Err(error) => failures.push(format!("root {slot}: {error}")),
        }
    }
    let mut directories = std::collections::BTreeSet::new();
    let mut active_links = 0;
    let mut parameter_banks = 0;
    let mut anger_profiles = 0;
    for species in file.species() {
        for kind in (0..2)
            .map(SpeciesTable::ParameterBank)
            .chain((0..12).map(SpeciesTable::AngerProfile))
        {
            match file.species_table(species.id, kind) {
                Ok(Some(table)) => {
                    for field in table.kind.fields() {
                        assert!(field.offset + field.scalar.size() <= table.stride);
                    }
                    match kind {
                        SpeciesTable::ParameterBank(_) => parameter_banks += 1,
                        SpeciesTable::AngerProfile(_) => anger_profiles += 1,
                    }
                }
                Ok(None) => {}
                Err(error) => failures.push(format!("species {} {kind:?}: {error}", species.id)),
            }
        }
        let Some(table) = file.directory_table(3, usize::from(species.id)).unwrap() else {
            continue;
        };
        if !directories.insert(table.range.start) {
            continue;
        }
        for index in 0..table.count {
            let (_, entry) = table.record(index).unwrap();
            let target = u32::from_le_bytes(entry[..4].try_into().unwrap());
            let gate = u32::from_le_bytes(entry[4..].try_into().unwrap());
            active_links += usize::from(target != 0 && gate != 0);
        }
    }
    eprintln!(
        "species +184: {} distinct directories, {active_links} active links",
        directories.len()
    );
    eprintln!(
        "species windows: {parameter_banks} parameter banks, {anger_profiles} anger profiles"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
