use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TranslationKey {
    Resource {
        resource_id: &'static str,
        group_id: &'static str,
        translation_group: u32,
        record_id: u32,
        part: u16,
    },
    Stage {
        stage: u16,
        section: u16,
        record: u16,
    },
}

impl fmt::Display for TranslationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource {
                resource_id,
                group_id,
                record_id,
                part,
                ..
            } => {
                write!(formatter, "{resource_id}:{group_id}:{record_id}")?;
                if *part != 0 {
                    write!(formatter, ":{part:02}")?;
                }
                Ok(())
            }
            Self::Stage {
                stage,
                section,
                record,
            } => {
                write!(formatter, "stage:{stage:03}:{section:04X}:{record:04X}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TranslationKey;

    #[test]
    fn translation_keys_include_the_stable_group_identity() {
        let resource = TranslationKey::Resource {
            resource_id: "mhfdat",
            group_id: "melee_weapon_descriptions",
            translation_group: 16,
            record_id: 42,
            part: 1,
        };
        let quest = TranslationKey::Resource {
            resource_id: "mhfinf",
            group_id: "quest",
            translation_group: 17,
            record_id: 25001,
            part: 7,
        };

        assert_eq!(
            resource.to_string(),
            "mhfdat:melee_weapon_descriptions:42:01"
        );
        assert_eq!(quest.to_string(), "mhfinf:quest:25001:07");
        assert_eq!(
            TranslationKey::Stage {
                stage: 1,
                section: 0x17,
                record: 0x2A
            }
            .to_string(),
            "stage:001:0017:002A"
        );
    }
}
