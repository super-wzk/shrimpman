//! Source records remain distinct; only validated target offsets share links.

use std::collections::BTreeMap;

use mhf_resource::emd::{Emd, HEADER_SIZE, ROOT_SIZE, Table};

use super::relations::{RecordRef, RelationError};

#[derive(Debug, Default)]
pub(super) struct ScriptLinks {
    pub records: Vec<ScriptRecord>,
    pub errors: Vec<RelationError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ScriptRecord {
    pub reference: RecordRef,
    pub links: Vec<ScriptLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScriptLink {
    pub species: u8,
    pub source: ScriptSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScriptSource {
    Condition {
        record: usize,
        key: u8,
        selector: u8,
    },
    Native(NativeBinding),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NativeBinding {
    pub species: u8,
    pub record: usize,
    pub group: &'static str,
    pub slot: u8,
    pub parameter: Option<u8>,
}

impl ScriptLinks {
    pub(super) fn new(file: &Emd<'_>, bindings: &[NativeBinding]) -> Self {
        let mut result = Self::default();
        let mut offsets = Vec::new();
        let mut aliases: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        if let Some(table) = result.table(file, 9) {
            for record in 0..table.count {
                // Root 9 precedes root 17, so its source index is also its
                // position in records, including entries with bad pointers.
                result.records.push(ScriptRecord::new(9, record));
                let target = match table.record(record) {
                    Ok((_, bytes)) => target(file, bytes, 0),
                    Err(error) => {
                        result.fail(9, error);
                        None
                    }
                };
                offsets.push(target);
                if let Some(target) = target {
                    aliases.entry(target).or_default().push(record);
                }
            }
        }

        for &binding in bindings {
            if binding.species >= file.species_count() {
                continue;
            }
            let Some(&offset) = offsets.get(binding.record) else {
                continue;
            };
            let link = ScriptLink {
                species: binding.species,
                source: ScriptSource::Native(binding),
            };
            // An explicit native index remains repairable even when its
            // pointer is invalid. Only alias propagation needs a valid target.
            if let Some(related) = offset.and_then(|offset| aliases.get(&offset)) {
                for &record in related {
                    result.records[record].add(link);
                }
            } else {
                result.records[binding.record].add(link);
            }
        }

        if let Some(table) = result.table(file, 17) {
            for record in 0..table.count {
                let mut source = ScriptRecord::new(17, record);
                match table.record(record) {
                    Ok((_, bytes)) if bytes[0] < file.species_count() => {
                        let link = ScriptLink {
                            species: bytes[0],
                            source: ScriptSource::Condition {
                                record,
                                key: bytes[1],
                                selector: bytes[2],
                            },
                        };
                        source.add(link);
                        if let Some(related) =
                            target(file, bytes, 4).and_then(|offset| aliases.get(&offset))
                        {
                            for &record in related {
                                result.records[record].add(link);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => result.fail(17, error),
                }
                result.records.push(source);
            }
        }
        result
    }

    /// Select species associations, or unassigned records when species is absent.
    pub(super) fn records_for(
        &self,
        slot: usize,
        species: Option<u8>,
    ) -> impl Iterator<Item = &ScriptRecord> {
        self.records.iter().filter(move |record| {
            record.reference.slot == slot
                && match species {
                    Some(species) => record.links.iter().any(|link| link.species == species),
                    None => record.links.is_empty(),
                }
        })
    }

    fn table<'a>(&mut self, file: &Emd<'a>, slot: usize) -> Option<Table<'a>> {
        match file.root_table(slot) {
            Ok(Some(table)) => Some(table),
            Ok(None) => {
                self.fail(slot, "EMD script table is absent");
                None
            }
            Err(error) => {
                self.fail(slot, error);
                None
            }
        }
    }

    fn fail(&mut self, slot: usize, error: impl ToString) {
        self.errors.push(RelationError {
            slot,
            directory: None,
            message: error.to_string(),
        });
    }
}

impl ScriptRecord {
    fn new(slot: usize, record: usize) -> Self {
        Self {
            reference: RecordRef {
                slot,
                directory: None,
                record,
                fallback: false,
            },
            links: Vec::new(),
        }
    }

    fn add(&mut self, link: ScriptLink) {
        if !self.links.contains(&link) {
            self.links.push(link);
        }
    }
}

fn target(file: &Emd<'_>, bytes: &[u8], at: usize) -> Option<usize> {
    let offset = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    (offset >= ROOT_SIZE
        && offset < file.as_bytes().len()
        && !(file.header_offset..file.header_offset + HEADER_SIZE).contains(&offset))
    .then_some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture(root9: &[u32], root17: &[(u8, u8, u8, u32)]) -> Vec<u8> {
        let mut bytes = vec![0; 4096];
        put32(&mut bytes, 0, 96);
        put32(&mut bytes, 12, 144);
        bytes[100] = 4;
        put32(&mut bytes, 9 * 4, 1024);
        put32(&mut bytes, 17 * 4, 1408);
        bytes[112..114].copy_from_slice(&(root9.len() as u16).to_le_bytes());
        bytes[120..122].copy_from_slice(&(root17.len() as u16).to_le_bytes());
        for (index, &offset) in root9.iter().enumerate() {
            put32(&mut bytes, 1024 + 4 * index, offset);
        }
        for (index, &(species, key, selector, offset)) in root17.iter().enumerate() {
            let at = 1408 + 8 * index;
            bytes[at..at + 3].copy_from_slice(&[species, key, selector]);
            put32(&mut bytes, at + 4, offset);
        }
        bytes[2048..2050].copy_from_slice(&[0xff, 2]);
        bytes[2112..2114].copy_from_slice(&[0xff, 2]);
        bytes
    }

    fn row(links: &ScriptLinks, slot: usize, record: usize) -> &ScriptRecord {
        links
            .records
            .iter()
            .find(|row| row.reference.slot == slot && row.reference.record == record)
            .unwrap()
    }

    fn condition(species: u8, record: usize, key: u8, selector: u8) -> ScriptLink {
        ScriptLink {
            species,
            source: ScriptSource::Condition {
                record,
                key,
                selector,
            },
        }
    }

    fn native(species: u8, record: usize, parameter: Option<u8>) -> NativeBinding {
        NativeBinding {
            species,
            record,
            group: "candidate group",
            slot: 1,
            parameter,
        }
    }

    #[test]
    fn shared_targets_keep_all_conditions_and_distinct_source_records() {
        let bytes = fixture(
            &[2048, 2112, 2048],
            &[
                (1, 2, 3, 2048),
                (2, 4, 5, 2048),
                (1, 7, 8, 2048),
                (1, 2, 3, 2048),
            ],
        );
        let links = ScriptLinks::new(&Emd::parse(&bytes).unwrap(), &[]);
        assert!(links.errors.is_empty());
        assert_eq!(links.records.len(), 7);
        let expected = [
            condition(1, 0, 2, 3),
            condition(2, 1, 4, 5),
            condition(1, 2, 7, 8),
            condition(1, 3, 2, 3),
        ];
        for index in [0, 2] {
            assert_eq!(row(&links, 9, index).links, expected);
        }
        // Identical script bytes at another offset do not establish a link.
        assert!(row(&links, 9, 1).links.is_empty());
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(row(&links, 17, index).links, [expected]);
        }
        assert_eq!(
            links
                .records
                .iter()
                .map(|row| (row.reference.slot, row.reference.record))
                .collect::<Vec<_>>(),
            [(9, 0), (9, 1), (9, 2), (17, 0), (17, 1), (17, 2), (17, 3)]
        );
    }

    #[test]
    fn native_aliases_preserve_binding_origins_without_changing_condition_owners() {
        let bytes = fixture(&[2048, 2048, 2112, 0, 0], &[(2, 6, 7, 2048)]);
        let first = native(1, 0, Some(2));
        let second = NativeBinding {
            group: "other group",
            slot: 3,
            ..native(3, 1, None)
        };
        let invalid = native(1, 3, Some(0));
        let links = ScriptLinks::new(
            &Emd::parse(&bytes).unwrap(),
            &[first, second, first, invalid],
        );
        assert!(links.errors.is_empty());
        let expected = [
            ScriptLink {
                species: 1,
                source: ScriptSource::Native(first),
            },
            ScriptLink {
                species: 3,
                source: ScriptSource::Native(second),
            },
            condition(2, 0, 6, 7),
        ];
        for index in [0, 1] {
            assert_eq!(row(&links, 9, index).links, expected);
        }
        assert!(row(&links, 9, 2).links.is_empty());
        assert!(row(&links, 9, 4).links.is_empty());
        assert_eq!(
            row(&links, 9, 3).links,
            [ScriptLink {
                species: 1,
                source: ScriptSource::Native(invalid)
            }]
        );
        assert_eq!(row(&links, 17, 0).links, [condition(2, 0, 6, 7)]);
    }

    #[test]
    fn invalid_targets_do_not_match_but_explicit_conditions_remain_repairable() {
        let offsets = [0, 1, 95, 96, 131, 4096, u32::MAX];
        let conditions: Vec<_> = offsets.iter().map(|&offset| (1, 2, 3, offset)).collect();
        let bytes = fixture(&offsets, &conditions);
        let links = ScriptLinks::new(&Emd::parse(&bytes).unwrap(), &[]);
        assert!(links.errors.is_empty());
        for index in 0..offsets.len() {
            assert!(row(&links, 9, index).links.is_empty());
            assert_eq!(row(&links, 17, index).links, [condition(1, index, 2, 3)]);
        }
    }

    #[test]
    fn valid_target_boundaries_do_not_require_decoding_script_contents() {
        let bytes = fixture(&[132, 4095], &[(0, 1, 2, 132), (3, 4, 5, 4095)]);
        let links = ScriptLinks::new(&Emd::parse(&bytes).unwrap(), &[]);
        assert!(links.errors.is_empty());
        assert_eq!(row(&links, 9, 0).links, [condition(0, 0, 1, 2)]);
        assert_eq!(row(&links, 9, 1).links, [condition(3, 1, 4, 5)]);
    }

    #[test]
    fn missing_species_and_native_records_do_not_create_links() {
        let bytes = fixture(&[2048], &[(4, 2, 3, 2048)]);
        let links = ScriptLinks::new(
            &Emd::parse(&bytes).unwrap(),
            &[native(4, 0, None), native(1, 1, None)],
        );
        assert!(links.errors.is_empty());
        assert_eq!(links.records.len(), 2);
        assert!(links.records.iter().all(|record| record.links.is_empty()));
    }

    #[test]
    fn damaged_tables_are_isolated_and_errors_identify_the_source_slot() {
        for damaged in [9, 17] {
            let mut bytes = fixture(&[2048], &[(2, 3, 4, 2048)]);
            put32(&mut bytes, damaged * 4, 4096);
            let binding = native(1, 0, None);
            let links = ScriptLinks::new(&Emd::parse(&bytes).unwrap(), &[binding]);
            assert_eq!(links.errors.len(), 1);
            assert_eq!(links.errors[0].slot, damaged);
            assert_eq!(links.errors[0].directory, None);
            assert!(!links.errors[0].message.is_empty());
            assert_eq!(links.records.len(), 1);
            if damaged == 9 {
                assert_eq!(row(&links, 17, 0).links, [condition(2, 0, 3, 4)]);
            } else {
                assert_eq!(
                    row(&links, 9, 0).links,
                    [ScriptLink {
                        species: 1,
                        source: ScriptSource::Native(binding)
                    }]
                );
            }
        }
        let mut bytes = fixture(&[2048], &[(2, 3, 4, 2048)]);
        for slot in [9, 17] {
            put32(&mut bytes, slot * 4, 4096);
        }
        let links = ScriptLinks::new(&Emd::parse(&bytes).unwrap(), &[]);
        assert!(links.records.is_empty());
        assert_eq!(
            links
                .errors
                .iter()
                .map(|error| error.slot)
                .collect::<Vec<_>>(),
            [9, 17]
        );
    }
}
