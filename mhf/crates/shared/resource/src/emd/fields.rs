//! Established scalar accesses. Unlisted bytes are deliberately not typed.
use super::RecordKind;
use crate::binary::ScalarType;

#[derive(Clone, Debug)]
pub struct FieldLayout {
    pub offset: usize,
    pub name: String,
    pub scalar: ScalarType,
}

impl RecordKind {
    pub fn fields(self) -> Vec<FieldLayout> {
        use ScalarType::*;
        let mut fields = Vec::new();
        let mut add = |offset, name: String, scalar| {
            fields.push(FieldLayout {
                offset,
                name,
                scalar,
            })
        };
        match self {
            Self::Header => {
                add(4, "species_slot_count".into(), U8);
                add(26, "table_18_count".into(), U8);
                for (offset, label) in [
                    (12, "table_07_count"),
                    (16, "table_09_count"),
                    (18, "table_13_count"),
                    (20, "table_14_count"),
                    (22, "table_16_count"),
                    (24, "table_17_count"),
                    (28, "table_19_count"),
                    (34, "table_22_count"),
                ] {
                    add(offset, label.into(), U16);
                }
            }
            Self::Species => {
                add(8, "scaling_records_offset".into(), U32);
                for index in 0..12 {
                    add(12 + 4 * index, format!("profile_{index:02}_offset"), U32);
                    add(
                        72 + 4 * index,
                        format!("anger_profile_{index:02}_offset"),
                        U32,
                    );
                }
                for index in 0..4 {
                    add(168 + 2 * index, format!("health_base_{index}"), I16);
                }
                add(176, "parameter_banks_offset".into(), U32);
                add(184, "parameter_directory_200_offset".into(), U32);
                add(188, "initial_actor_3392".into(), U8);
            }
            Self::Pointers => add(0, "offset".into(), U32),
            Self::PartParameters => {
                for index in 0..9 {
                    add(index * 2, format!("part_{index}_initial_value"), I16);
                }
                add(18, "initial_actor_2924".into(), U8);
            }
            Self::FixedParameters => {
                add(5, "initial_actor_834".into(), U8);
                add(32, "other_part_recovery_ratio".into(), F32);
                add(42, "value_2a".into(), I16);
                add(44, "request_timer_limit".into(), I16);
                add(48, "timer_3214_base".into(), I16);
            }
            Self::Classification => add(4, "display_category".into(), U8),
            Self::SpeciesLookup => {
                add(0, "species_id".into(), U8);
                add(4, "probability_rows_offset".into(), U32);
                add(8, "health_multiplier".into(), F32);
            }
            Self::PartMap => {
                for index in 0..9 {
                    add(index * 2, format!("part_{index}_mapped_index"), I16);
                }
            }
            Self::Modifiers | Self::SpeciesModifiers => {
                add(0, "species_id".into(), U16);
                if self == Self::Modifiers {
                    add(2, "profile_selector_negative_is_wildcard".into(), I16);
                }
                for offset in (4..28).step_by(4) {
                    add(offset, format!("multiplier_{offset:02x}"), F32);
                }
            }
            Self::KeyedMultiplier => {
                add(0, "key_00".into(), I16);
                add(2, "key_02".into(), I16);
                add(4, "key_04".into(), I32);
                add(8, "multiplier".into(), F32);
            }
            Self::Count => add(0, "record_count".into(), U16),
            Self::PointerRecord => {
                add(0, "species_id".into(), U8);
                add(1, "actor_3389_key".into(), U8);
                add(2, "selector".into(), U8);
                add(4, "offset".into(), U32);
            }
            Self::ParameterLink => {
                add(0, "target_offset".into(), U32);
                add(4, "value_04".into(), U32);
            }
            Self::SpeciesValues => {
                for index in 0..8 {
                    add(2 * index, format!("value_{index}"), I16);
                }
                add(16, "species_id".into(), U8);
            }
            Self::SpeciesAssociation => {
                add(2, "species_id".into(), U16);
                add(4, "anchor_bone_index".into(), I16);
                add(6, "anchor_offset_x".into(), I16);
                add(8, "anchor_offset_y".into(), I16);
                add(10, "anchor_offset_z".into(), I16);
                add(26, "action_rule_count".into(), U16);
                add(28, "action_rules_offset".into(), U32);
            }
            Self::ActionRule => {
                add(0, "result_nonzero".into(), U8);
                add(1, "action_group".into(), U8);
                add(2, "action_id".into(), U16);
            }
            Self::Category => add(0, "classification_key".into(), U16),
            Self::GroupRecord => {
                add(4, "actor_2394_key".into(), U32);
                add(12, "value_0c".into(), U16);
                add(14, "value_0e".into(), U16);
                add(16, "species_id".into(), I16);
            }
            Self::Parameters80 | Self::Parameters90 => {}
        }
        fields.sort_by_key(|field| field.offset);
        fields
    }
}
