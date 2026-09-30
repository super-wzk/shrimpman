use std::collections::HashSet;

use super::{EXTRA_STRIDE, FieldLayout, attack, auxiliary, hitbox, parameters};
use crate::{PathSegment, ResourcePath, binary::ScalarType, dat};

fn check_keys<'a>(keys: impl IntoIterator<Item = &'a str>) {
    let mut seen = HashSet::new();
    for key in keys {
        assert!(seen.insert(key), "duplicate schema key {key}");
        let path = ResourcePath::from_parts("schema.bin", [PathSegment::Field(key.into())])
            .unwrap_or_else(|error| panic!("invalid schema key {key}: {error}"));
        assert_eq!(path.to_string().parse::<ResourcePath>().unwrap(), path);
    }
}

#[test]
fn dat_keys_are_valid_and_unique_within_each_record_schema() {
    for table in dat::DATA_TABLES {
        if let dat::RecordFormat::Fields(fields) = table.format {
            check_keys(fields.iter().map(|field| field.key));
            for field in fields {
                assert_eq!(field.key, format!("field_{:02x}", field.offset));
            }
        }
    }
}

#[test]
fn all_selected_sdt_schemas_have_valid_unique_field_keys() {
    check_keys(attack::FIELDS.iter().map(|field| field.key));
    let mut bank = vec![0; 800 * EXTRA_STRIDE];
    // Exercise range-selected weapon layouts as well as fixed native selectors.
    for (selector, record) in [(125, 200_i32), (129, 210), (136, 220)] {
        let at = selector * EXTRA_STRIDE + 16;
        bank[at..at + 4].copy_from_slice(&record.to_le_bytes());
        bank[at + 4..at + 8].copy_from_slice(&record.to_le_bytes());
    }
    for category in [100, 106, 107, 140, 141, 160, 999] {
        for index in 0..800 {
            check_keys(
                auxiliary::fields(category, index)
                    .iter()
                    .map(|field| field.key),
            );
            // The category-140 record selector changes its field layout.
            for state in [0_u32, 2, 1] {
                let mut bytes = [0; EXTRA_STRIDE];
                bytes[20..24].copy_from_slice(&state.to_le_bytes());
                check_keys(
                    parameters::fields(category, index, &bytes, &bank)
                        .iter()
                        .map(|field| field.key),
                );
            }
        }
    }
    for mode in [0_u16, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128] {
        for shape in [0_u16, 1, 2] {
            let mut bytes = [0; 40];
            bytes[..2].copy_from_slice(&mode.to_le_bytes());
            bytes[2..4].copy_from_slice(&shape.to_le_bytes());
            check_keys(hitbox::fields(&bytes).iter().map(|field| field.key));
        }
    }
}

#[test]
fn attack_core_keys_and_unknown_keys_retain_the_original_storage() {
    for (key, name, offset, scalar) in [
        (
            "startup_count",
            "启动延迟（原始计数）",
            0x00,
            ScalarType::U16,
        ),
        (
            "active_count",
            "有效阶段（原始计数）",
            0x02,
            ScalarType::U16,
        ),
        ("power", "基础威力／动作值", 0x04, ScalarType::U16),
        ("unknown_08", "unknown_08", 0x08, ScalarType::U16),
        ("unknown_0d", "unknown_0d", 0x0d, ScalarType::I8),
        ("unknown_24", "unknown_24", 0x24, ScalarType::U32),
    ] {
        let field = attack::FIELDS
            .iter()
            .find(|field| field.key == key)
            .unwrap();
        assert_eq!(field.name, name);
        assert_eq!(field.offset, offset);
        assert_eq!(field.scalar, scalar);
    }
    let FieldLayout {
        key,
        offset,
        scalar,
        ..
    } = attack::FIELDS[2];
    let renamed = FieldLayout {
        key,
        name: "不同语言的显示名称",
        offset,
        scalar,
    };
    let path = ResourcePath::from_parts(
        "mhfsdt.bin",
        [
            PathSegment::Index(0),
            PathSegment::Field("attacks".into()),
            PathSegment::Index(23),
            PathSegment::Field(renamed.key.into()),
        ],
    )
    .unwrap();
    assert_eq!(path.to_string(), "mhfsdt.bin#0/attacks/23/power");
}
