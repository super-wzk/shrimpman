use std::collections::HashSet;

use super::{
    ATTACK_STRIDE, AUXILIARY_STRIDE, EXTRA_STRIDE, FieldLayout, HITBOX_STRIDE, attack, auxiliary,
    hitbox, parameters,
};
use crate::{PathSegment, ResourcePath, dat};

fn check_fields(fields: &[FieldLayout], stride: usize) {
    let mut keys = HashSet::new();
    let mut used = vec![false; stride];
    for field in fields {
        assert!(keys.insert(field.key), "duplicate schema key {}", field.key);
        ResourcePath::from_parts("schema.bin", [PathSegment::Field(field.key.into())])
            .unwrap_or_else(|error| panic!("invalid schema key {}: {error}", field.key));
        let start = usize::from(field.offset);
        let end = start + field.scalar.size();
        assert!(end <= stride, "{} exceeds its record", field.key);
        assert!(
            used[start..end].iter().all(|value| !value),
            "overlapping field {}",
            field.key
        );
        used[start..end].fill(true);
    }
}

#[test]
fn dat_schemas_have_unique_keys_and_disjoint_bounded_fields() {
    for table in dat::DATA_TABLES {
        if let dat::RecordFormat::Fields(fields) = table.format {
            check_fields(fields, usize::from(table.stride));
        }
    }
}

#[test]
fn all_selected_sdt_schemas_have_valid_unique_field_keys() {
    check_fields(attack::FIELDS, ATTACK_STRIDE);
    let mut bank = vec![0; 800 * EXTRA_STRIDE];
    // Exercise range-selected weapon layouts as well as fixed native selectors.
    for (selector, record) in [(125, 200_i32), (129, 210), (136, 220)] {
        let at = selector * EXTRA_STRIDE + 16;
        bank[at..at + 4].copy_from_slice(&record.to_le_bytes());
        bank[at + 4..at + 8].copy_from_slice(&record.to_le_bytes());
    }
    for category in [100, 106, 107, 140, 141, 160, 999] {
        for index in 0..800 {
            check_fields(auxiliary::fields(category, index), AUXILIARY_STRIDE);
            // The category-140 record selector changes its field layout.
            for state in [0_u32, 2, 1] {
                let mut bytes = [0; EXTRA_STRIDE];
                bytes[20..24].copy_from_slice(&state.to_le_bytes());
                check_fields(
                    parameters::fields(category, index, &bytes, &bank),
                    EXTRA_STRIDE,
                );
            }
        }
    }
    for mode in [0_u16, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128] {
        for shape in [0_u16, 1, 2] {
            let mut bytes = [0; 40];
            bytes[..2].copy_from_slice(&mode.to_le_bytes());
            bytes[2..4].copy_from_slice(&shape.to_le_bytes());
            check_fields(hitbox::fields(&bytes), HITBOX_STRIDE);
        }
    }
}
