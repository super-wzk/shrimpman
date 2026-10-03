//! Native-established windows behind links in a species record.
//!
//! These views do not infer allocation sizes from neighboring offsets. The
//! anger view is a confirmed 60-byte prefix; parameter banks 0 and 1 are two
//! independently validated windows, not a claim about the total bank count.

use super::{Emd, RecordKind, SPECIES_STRIDE, Table};
use crate::{Error, Result, binary::Reader};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpeciesTable {
    /// Native accessors index 200 records of 40 bytes per bank. Only banks 0
    /// and 1 have established consumers (111A5080 and 10050530).
    ParameterBank(u8),
    /// One of twelve nullable links at species +72. The view includes the
    /// fixed fields and eleven health-bucket multipliers read by 10851240.
    AngerProfile(u8),
}

impl<'a> Emd<'a> {
    pub fn species_table(&self, species: u8, table: SpeciesTable) -> Result<Option<Table<'a>>> {
        if species >= self.species_count() {
            return Err(Error::new(
                self.species_offset,
                "EMD species index out of range",
            ));
        }
        let record = self.species_offset + usize::from(species) * SPECIES_STRIDE;
        let (link, displacement, count, stride, kind) = match table {
            SpeciesTable::ParameterBank(bank) => {
                if bank >= 2 {
                    return Err(Error::new(
                        record + 176,
                        "EMD parameter bank extent is not established",
                    ));
                }
                (
                    176,
                    usize::from(bank) * 8000,
                    200,
                    40,
                    RecordKind::SpeciesParameter,
                )
            }
            SpeciesTable::AngerProfile(profile) => {
                if profile >= 12 {
                    return Err(Error::new(
                        record + 72,
                        "EMD anger profile index out of range",
                    ));
                }
                (
                    72 + usize::from(profile) * 4,
                    0,
                    1,
                    60,
                    RecordKind::AngerProfile,
                )
            }
        };
        let base = Reader::new(self.bytes).read_at::<u32>(record + link)?.value as usize;
        // 10AFA180 relocates both kinds of link only when nonzero.
        if base == 0 {
            return Ok(None);
        }
        // Validate the original link before adding the bank displacement: an
        // invalid root/header pointer must not become valid by adding 8000.
        self.table(base, 1, stride, kind)?;
        let offset = base
            .checked_add(displacement)
            .ok_or_else(|| Error::new(record + link, "EMD species table offset overflow"))?;
        self.table(offset, count, stride, kind).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::dword as put32;

    fn fixture(size: usize) -> Vec<u8> {
        let mut bytes = vec![0; size];
        put32(&mut bytes, 0, 96);
        bytes[100] = 1;
        put32(&mut bytes, 12, 144);
        bytes
    }

    #[test]
    fn parameter_banks_use_native_windows_and_validate_each_bank() {
        let mut bytes = fixture(16_400);
        put32(&mut bytes, 144 + 176, 400);
        let file = Emd::parse(&bytes).unwrap();
        for bank in 0..2 {
            let table = file
                .species_table(0, SpeciesTable::ParameterBank(bank))
                .unwrap()
                .unwrap();
            let start = 400 + usize::from(bank) * 8000;
            assert_eq!(table.range, start..start + 8000);
            assert_eq!((table.count, table.stride), (200, 40));
            assert_eq!(table.kind, RecordKind::SpeciesParameter);
            assert_eq!(table.record(199).unwrap().0, start + 7960);
            assert!(table.record(200).is_err());
        }
        assert!(
            file.species_table(0, SpeciesTable::ParameterBank(2))
                .is_err()
        );
        bytes.truncate(16_399);
        let file = Emd::parse(&bytes).unwrap();
        assert!(
            file.species_table(0, SpeciesTable::ParameterBank(0))
                .is_ok()
        );
        assert!(
            file.species_table(0, SpeciesTable::ParameterBank(1))
                .is_err()
        );
    }

    #[test]
    fn anger_profiles_preserve_aliases_and_limit_the_view_to_the_known_prefix() {
        let mut bytes = fixture(460);
        put32(&mut bytes, 144 + 72, 400);
        put32(&mut bytes, 144 + 72 + 11 * 4, 400);
        bytes[400..].fill(0xa5);
        let file = Emd::parse(&bytes).unwrap();
        for profile in [0, 11] {
            let table = file
                .species_table(0, SpeciesTable::AngerProfile(profile))
                .unwrap()
                .unwrap();
            assert_eq!(table.range, 400..460);
            assert_eq!((table.count, table.stride), (1, 60));
            assert_eq!(table.kind, RecordKind::AngerProfile);
            assert_eq!(table.record(0).unwrap().1, &[0xa5; 60]);
        }
        assert!(
            file.species_table(0, SpeciesTable::AngerProfile(12))
                .is_err()
        );
        bytes.truncate(459);
        assert!(
            Emd::parse(&bytes)
                .unwrap()
                .species_table(0, SpeciesTable::AngerProfile(0))
                .is_err()
        );
    }

    #[test]
    fn nullable_links_and_invalid_offsets_are_distinguished() {
        let mut bytes = fixture(16_400);
        for kind in [
            SpeciesTable::ParameterBank(0),
            SpeciesTable::AngerProfile(0),
        ] {
            assert!(
                Emd::parse(&bytes)
                    .unwrap()
                    .species_table(0, kind)
                    .unwrap()
                    .is_none()
            );
            assert!(Emd::parse(&bytes).unwrap().species_table(1, kind).is_err());
        }
        for offset in [1, 96, 132 - 1, u32::MAX] {
            put32(&mut bytes, 144 + 176, offset);
            put32(&mut bytes, 144 + 72, offset);
            let file = Emd::parse(&bytes).unwrap();
            assert!(
                file.species_table(0, SpeciesTable::ParameterBank(1))
                    .is_err()
            );
            assert!(
                file.species_table(0, SpeciesTable::AngerProfile(0))
                    .is_err()
            );
        }
    }
}
