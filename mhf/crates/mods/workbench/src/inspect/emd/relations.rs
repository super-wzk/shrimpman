//! Species associations retain source indices and native record order.
//! Additional profile/actor selectors remain visible to the caller; this is
//! an inventory of associated records, not a runtime selector evaluation.

use super::{native_script_bindings, script_links::ScriptLinks};
use mhf_resource::{
    binary::Reader,
    emd::{Emd, Table},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct RecordRef {
    pub slot: usize,
    pub directory: Option<usize>,
    pub record: usize,
    pub fallback: bool,
}

#[derive(Debug)]
pub(super) struct RelationError {
    pub slot: usize,
    pub directory: Option<usize>,
    pub message: String,
}

#[derive(Debug, Default)]
pub(super) struct Relations {
    pub records: Vec<RecordRef>,
    pub errors: Vec<RelationError>,
}

impl Relations {
    fn fail(&mut self, slot: usize, directory: Option<usize>, error: impl ToString) {
        self.errors.push(RelationError {
            slot,
            directory,
            message: error.to_string(),
        });
    }
}

/// Find every record associated with one species in a supported root.
///
/// Root 16 follows the parallel counts in root 15. Its references retain the
/// group index. Root 19 returns its first record as a marked fallback only
/// when no explicit species match exists. Aliases and repeated matches are
/// preserved. A damaged target contributes an error without stopping later
/// configurations or groups; separate root queries are independent.
pub(super) fn records_for_species(file: &Emd<'_>, species: u8, slot: usize) -> Relations {
    let mut result = Relations::default();
    if species >= file.species_count() {
        result.fail(slot, None, "EMD species index out of range");
        return result;
    }
    match slot {
        2 | 5 | 11 | 12 | 21 => {
            if let Some(table) = read_table(file, slot, None, &mut result) {
                add_indexed_record(&table, slot, None, usize::from(species), &mut result);
            }
        }
        1 | 4 | 10 | 16 => {
            let Some(directory) = read_table(file, slot, None, &mut result) else {
                return result;
            };
            for index in 0..directory.count {
                let Some(table) = read_table(file, slot, Some(index), &mut result) else {
                    continue;
                };
                if slot == 16 {
                    add_matching_records(&table, species, slot, Some(index), &mut result);
                } else {
                    add_indexed_record(
                        &table,
                        slot,
                        Some(index),
                        usize::from(species),
                        &mut result,
                    );
                }
            }
        }
        9 | 17 => return script_records(file, Some(species), slot),
        7 | 13 | 18 | 19 | 22 => {
            if let Some(table) = read_table(file, slot, None, &mut result) {
                add_matching_records(&table, species, slot, None, &mut result);
                if slot == 19
                    && result.records.is_empty()
                    && result.errors.is_empty()
                    && table.count != 0
                {
                    result.records.push(RecordRef {
                        slot,
                        directory: None,
                        record: 0,
                        fallback: true,
                    });
                }
            }
        }
        _ => result.fail(slot, None, "EMD species association is not established"),
    }
    result
}

/// Records not assigned to an existing species slot, plus root 19's shared
/// first-record default. Known script associations only remove entries when
/// their owning species exists in this resource. Source ordering is retained.
pub(super) fn global_records(file: &Emd<'_>, slot: usize) -> Relations {
    let mut result = Relations::default();
    match slot {
        9 | 17 => return script_records(file, None, slot),
        7 | 13 | 16 | 18 | 19 | 22 => {
            let Some(table) = read_table(file, slot, None, &mut result) else {
                return result;
            };
            let unassigned = |species| species < 0 || species >= i32::from(file.species_count());
            if slot == 16 {
                for directory in 0..table.count {
                    let Some(group) = read_table(file, slot, Some(directory), &mut result) else {
                        continue;
                    };
                    add_keyed_records(
                        &group,
                        slot,
                        Some(directory),
                        unassigned,
                        false,
                        &mut result,
                    );
                }
            } else {
                add_keyed_records(&table, slot, None, unassigned, slot == 19, &mut result);
            }
        }
        _ => result.fail(slot, None, "EMD global association is not established"),
    }
    result
}

pub(super) fn script_links(file: &Emd<'_>) -> ScriptLinks {
    ScriptLinks::new(file, native_script_bindings::ZZ_HD_BINDINGS)
}

fn script_records(file: &Emd<'_>, species: Option<u8>, slot: usize) -> Relations {
    let index = script_links(file);
    Relations {
        records: index
            .records_for(slot, species)
            .map(|record| record.reference)
            .collect(),
        errors: index
            .errors
            .into_iter()
            .filter(|error| error.slot == slot)
            .collect(),
    }
}

fn species_key(slot: usize, bytes: &[u8]) -> mhf_resource::Result<i32> {
    let reader = Reader::new(bytes);
    Ok(match slot {
        7 => i32::from(reader.read_at::<u8>(0)?.value),
        13 | 22 => i32::from(reader.read_at::<u16>(0)?.value),
        16 => i32::from(reader.read_at::<i16>(16)?.value),
        18 => i32::from(reader.read_at::<u8>(16)?.value),
        19 => i32::from(reader.read_at::<u16>(2)?.value),
        _ => unreachable!("only established keyed tables are scanned"),
    })
}

fn read_table<'a>(
    file: &Emd<'a>,
    slot: usize,
    directory: Option<usize>,
    result: &mut Relations,
) -> Option<Table<'a>> {
    let table = match directory {
        Some(index) => file.directory_table(slot, index),
        None => file.root_table(slot),
    };
    match table {
        Ok(Some(table)) => Some(table),
        Ok(None) => {
            result.fail(
                slot,
                directory,
                "EMD association table is absent or unresolved",
            );
            None
        }
        Err(error) => {
            result.fail(slot, directory, error);
            None
        }
    }
}

fn add_indexed_record(
    table: &Table<'_>,
    slot: usize,
    directory: Option<usize>,
    record: usize,
    result: &mut Relations,
) {
    match table.record(record) {
        Ok(_) => result.records.push(RecordRef {
            slot,
            directory,
            record,
            fallback: false,
        }),
        Err(error) => result.fail(slot, directory, error),
    }
}

fn add_matching_records(
    table: &Table<'_>,
    species: u8,
    slot: usize,
    directory: Option<usize>,
    result: &mut Relations,
) {
    add_keyed_records(
        table,
        slot,
        directory,
        |key| key == i32::from(species),
        false,
        result,
    );
}

fn add_keyed_records(
    table: &Table<'_>,
    slot: usize,
    directory: Option<usize>,
    matches: impl Fn(i32) -> bool,
    include_default: bool,
    result: &mut Relations,
) {
    for record in 0..table.count {
        let key = table
            .record(record)
            .and_then(|(_, bytes)| species_key(slot, bytes));
        let fallback = include_default && record == 0;
        match key {
            Ok(key) if fallback || matches(key) => result.records.push(RecordRef {
                slot,
                directory,
                record,
                fallback,
            }),
            Ok(_) => {}
            Err(error) => result.fail(slot, directory, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 8192];
        put32(&mut bytes, 0, 96);
        put32(&mut bytes, 12, 144);
        bytes[100] = 3;
        bytes
    }

    fn record(slot: usize, directory: Option<usize>, record: usize) -> RecordRef {
        RecordRef {
            slot,
            directory,
            record,
            fallback: false,
        }
    }

    #[test]
    fn direct_tables_use_species_index_and_queries_do_not_share_failures() {
        let mut bytes = fixture();
        for slot in [2, 5, 11, 12, 21] {
            put32(&mut bytes, slot * 4, 1024);
        }
        put32(&mut bytes, 2 * 4, u32::MAX);
        let file = Emd::parse(&bytes).unwrap();
        let broken = records_for_species(&file, 2, 2);
        assert!(broken.records.is_empty());
        assert_eq!(broken.errors.len(), 1);
        assert_eq!(broken.errors[0].slot, 2);
        assert_eq!(broken.errors[0].directory, None);
        assert!(!broken.errors[0].message.is_empty());
        for slot in [5, 11, 12, 21] {
            let result = records_for_species(&file, 2, slot);
            assert_eq!(result.records, [record(slot, None, 2)]);
            assert!(result.errors.is_empty());
        }
        assert_eq!(records_for_species(&file, 3, 5).errors.len(), 1);
    }

    #[test]
    fn profile_order_and_aliases_survive_a_bad_middle_target() {
        for slot in [1, 4, 10] {
            let mut bytes = fixture();
            put32(&mut bytes, slot * 4, 1024);
            let count = if slot == 10 { 3 } else { 12 };
            for index in 0..count {
                put32(&mut bytes, 1024 + index * 4, 2048);
            }
            put32(&mut bytes, 1028, u32::MAX);
            let file = Emd::parse(&bytes).unwrap();
            let result = records_for_species(&file, 2, slot);
            assert_eq!(
                result.records,
                (0..count)
                    .filter(|index| *index != 1)
                    .map(|index| record(slot, Some(index), 2))
                    .collect::<Vec<_>>()
            );
            assert_eq!(result.errors.len(), 1);
            assert_eq!(result.errors[0].directory, Some(1));
        }
    }

    #[test]
    fn keyed_records_keep_all_conditions_in_source_order() {
        for (slot, count_offset, stride, key_offset, wide) in [
            (7, 12, 12, 0, false),
            (13, 18, 28, 0, true),
            (17, 24, 8, 0, false),
            (18, 26, 18, 16, false),
            (19, 28, 32, 2, true),
            (22, 34, 28, 0, true),
        ] {
            let mut bytes = fixture();
            put32(&mut bytes, slot * 4, 1024);
            put16(&mut bytes, 96 + count_offset, 4);
            for (index, species) in [2, 1, 2, 2].into_iter().enumerate() {
                let at = 1024 + index * stride;
                if wide {
                    put16(&mut bytes, at + key_offset, species);
                } else {
                    bytes[at + key_offset] = species as u8;
                }
                // Distinct conditions are not evaluated or collapsed here.
                if slot == 13 {
                    put16(&mut bytes, at + 2, [u16::MAX, 0, 3, 9][index]);
                } else if slot == 17 {
                    bytes[at + 1] = index as u8;
                    bytes[at + 2] = (index * 2) as u8;
                }
            }
            let file = Emd::parse(&bytes).unwrap();
            let result = records_for_species(&file, 2, slot);
            assert_eq!(
                result.records,
                [0, 2, 3].map(|index| record(slot, None, index))
            );
            assert!(result.errors.is_empty());
        }
    }

    #[test]
    fn association_default_is_only_used_when_no_explicit_match_exists() {
        let mut bytes = fixture();
        put32(&mut bytes, 19 * 4, 1024);
        put16(&mut bytes, 96 + 28, 2);
        put16(&mut bytes, 1024 + 2, 1);
        put16(&mut bytes, 1056 + 2, 1);
        let file = Emd::parse(&bytes).unwrap();
        let fallback = records_for_species(&file, 2, 19);
        assert_eq!(
            fallback.records,
            [RecordRef {
                fallback: true,
                ..record(19, None, 0)
            }]
        );
        assert!(fallback.errors.is_empty());
        assert_eq!(
            records_for_species(&file, 1, 19).records,
            [record(19, None, 0), record(19, None, 1)]
        );
        put16(&mut bytes, 96 + 28, 0);
        assert!(
            records_for_species(&Emd::parse(&bytes).unwrap(), 2, 19)
                .records
                .is_empty()
        );
    }

    #[test]
    fn grouped_records_keep_group_indices_and_skip_only_damaged_targets() {
        let mut bytes = fixture();
        put16(&mut bytes, 96 + 22, 3);
        put32(&mut bytes, 15 * 4, 1024);
        put32(&mut bytes, 16 * 4, 1100);
        for (index, count) in [3, 1, 2].into_iter().enumerate() {
            put16(&mut bytes, 1024 + 2 * index, count);
        }
        put32(&mut bytes, 1100, 2048);
        put32(&mut bytes, 1104, u32::MAX);
        put32(&mut bytes, 1108, 3072);
        for (index, species) in [2, u16::MAX, 2].into_iter().enumerate() {
            put16(&mut bytes, 2048 + 32 * index + 16, species);
        }
        put16(&mut bytes, 3072 + 16, 1);
        put16(&mut bytes, 3072 + 32 + 16, 2);
        let file = Emd::parse(&bytes).unwrap();
        let result = records_for_species(&file, 2, 16);
        assert_eq!(
            result.records,
            [
                record(16, Some(0), 0),
                record(16, Some(0), 2),
                record(16, Some(2), 1)
            ]
        );
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].slot, 16);
        assert_eq!(result.errors[0].directory, Some(1));
    }

    #[test]
    fn global_and_species_keyed_records_partition_without_losing_multiple_matches() {
        for (slot, count_offset, stride, key_offset, wide) in [
            (7, 12, 12, 0, false),
            (13, 18, 28, 0, true),
            (17, 24, 8, 0, false),
            (18, 26, 18, 16, false),
            (22, 34, 28, 0, true),
        ] {
            let mut bytes = fixture();
            put32(&mut bytes, slot * 4, 1024);
            put16(&mut bytes, 96 + count_offset, 5);
            let outside = if wide { 0x0102 } else { 255 };
            for (index, species) in [2, 3, 2, outside, 0].into_iter().enumerate() {
                let at = 1024 + stride * index + key_offset;
                if wide {
                    put16(&mut bytes, at, species);
                } else {
                    bytes[at] = species as u8;
                }
            }
            let file = Emd::parse(&bytes).unwrap();
            let global = global_records(&file, slot);
            assert!(global.errors.is_empty());
            assert_eq!(
                global.records,
                [record(slot, None, 1), record(slot, None, 3)]
            );
            assert_eq!(
                records_for_species(&file, 2, slot).records,
                [record(slot, None, 0), record(slot, None, 2)]
            );
            let mut partition = global.records;
            for species in 0..file.species_count() {
                partition.extend(records_for_species(&file, species, slot).records);
            }
            partition.sort_by_key(|reference| reference.record);
            assert_eq!(
                partition,
                (0..5)
                    .map(|index| record(slot, None, index))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn global_groups_preserve_negative_keys_and_continue_after_a_bad_group() {
        let mut bytes = fixture();
        put16(&mut bytes, 96 + 22, 3);
        put32(&mut bytes, 15 * 4, 1024);
        put32(&mut bytes, 16 * 4, 1100);
        for (index, count) in [4, 1, 3].into_iter().enumerate() {
            put16(&mut bytes, 1024 + 2 * index, count);
        }
        put32(&mut bytes, 1100, 2048);
        put32(&mut bytes, 1104, u32::MAX);
        put32(&mut bytes, 1108, 3072);
        for (index, species) in [2i16, -1, -254, 3].into_iter().enumerate() {
            put16(&mut bytes, 2048 + index * 32 + 16, species as u16);
        }
        for (index, species) in [1, 2, 3].into_iter().enumerate() {
            put16(&mut bytes, 3072 + index * 32 + 16, species);
        }
        let file = Emd::parse(&bytes).unwrap();
        let global = global_records(&file, 16);
        assert_eq!(
            global.records,
            [
                record(16, Some(0), 1),
                record(16, Some(0), 2),
                record(16, Some(0), 3),
                record(16, Some(2), 2),
            ]
        );
        assert_eq!(global.errors.len(), 1);
        assert_eq!(global.errors[0].directory, Some(1));
        assert_eq!(
            records_for_species(&file, 2, 16).records,
            [record(16, Some(0), 0), record(16, Some(2), 1)]
        );
    }

    #[test]
    fn global_default_is_shared_and_emitted_once_even_when_its_species_is_unknown() {
        let mut bytes = fixture();
        put32(&mut bytes, 19 * 4, 1024);
        put16(&mut bytes, 96 + 28, 4);
        for (index, species) in [1, 2, 255, 2].into_iter().enumerate() {
            put16(&mut bytes, 1024 + index * 32 + 2, species);
        }
        for first_species in [1, 255] {
            put16(&mut bytes, 1024 + 2, first_species);
            let file = Emd::parse(&bytes).unwrap();
            let global = global_records(&file, 19);
            assert!(global.errors.is_empty());
            assert_eq!(
                global.records,
                [
                    RecordRef {
                        fallback: true,
                        ..record(19, None, 0)
                    },
                    record(19, None, 2),
                ]
            );
            assert_eq!(
                records_for_species(&file, 2, 19).records,
                [record(19, None, 1), record(19, None, 3)]
            );
        }
        put16(&mut bytes, 96 + 28, 0);
        assert!(
            global_records(&Emd::parse(&bytes).unwrap(), 19)
                .records
                .is_empty()
        );
    }

    #[test]
    fn scripts_are_partitioned_only_when_the_known_owner_exists() {
        let mut bytes = fixture();
        bytes.resize(51_200, 0);
        put32(&mut bytes, 9 * 4, 50_000);
        put16(&mut bytes, 96 + 16, 300);
        let assigned = [185, 186, 187, 188, 189, 190, 272, 273];
        for count in [0, 3, 146, 147] {
            bytes[100] = count;
            let file = Emd::parse(&bytes).unwrap();
            let global = global_records(&file, 9);
            assert!(global.errors.is_empty());
            assert_eq!(
                global.records,
                (0..300)
                    .filter(|index| count <= 146 || !assigned.contains(index))
                    .map(|index| record(9, None, index))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn global_root_failure_does_not_change_other_queries() {
        let mut bytes = fixture();
        put32(&mut bytes, 7 * 4, u32::MAX);
        put16(&mut bytes, 96 + 12, 1);
        put32(&mut bytes, 17 * 4, 1024);
        put16(&mut bytes, 96 + 24, 1);
        bytes[1024] = 255;
        let file = Emd::parse(&bytes).unwrap();
        let broken = global_records(&file, 7);
        assert!(broken.records.is_empty());
        assert_eq!(broken.errors.len(), 1);
        assert_eq!(broken.errors[0].slot, 7);
        assert_eq!(broken.errors[0].directory, None);
        let intact = global_records(&file, 17);
        assert!(intact.errors.is_empty());
        assert_eq!(intact.records, [record(17, None, 0)]);
    }
}
