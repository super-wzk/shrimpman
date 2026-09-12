//! Resource identities from the supported client's equipment filename builders.

use super::ResourceRef;
use mhf_resource::{container::MhaArchive, effect::ModelEffectBinding};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EquipmentModel {
    pub model_id: u16,
    part_code: u16,
    /// Ordinary weapons do not distinguish sex; their branch tests only mode 2.
    variant: Option<u16>,
}

impl EquipmentModel {
    pub fn matches(self, binding: &ModelEffectBinding) -> bool {
        self.model_id == binding.model_id
            && self.part_code == binding.part_code
            && self
                .variant
                .map_or(binding.variant != 2, |variant| variant == binding.variant)
    }
}

fn decimal(text: &str, width: usize) -> Option<u16> {
    if text.len() < width
        || (text.len() > width && text.starts_with('0'))
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok()
}

fn filename(name: &str) -> Option<EquipmentModel> {
    let name = name.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let exact_stem = name.strip_suffix(".bin")?;
    let stem = exact_stem.strip_suffix("-so").unwrap_or(exact_stem);
    let bytes = stem.as_bytes();
    // 108E0550: weapon\\w%c%03d, character = model_id / 1000 + 'e'.
    if bytes.len() == 5 && bytes[0] == b'w' && bytes[1].is_ascii() && bytes[1] >= b'e' {
        let bucket = u16::from(bytes[1] - b'e');
        return Some(EquipmentModel {
            model_id: bucket
                .checked_mul(1000)?
                .checked_add(decimal(&stem[2..], 3)?)?,
            part_code: 1,
            variant: None,
        });
    }
    // 10AE25F0 / 10AE2430: special-mode weapon, head, and body packages.
    for (prefix, part_code) in [("n47w", 1), ("n47h", 2), ("n47b", 3)] {
        if let Some(number) = exact_stem.strip_prefix(prefix) {
            return Some(EquipmentModel {
                model_id: decimal(number, 4)?,
                part_code,
                variant: Some(2),
            });
        }
    }
    // 108E06F0 and 11887984: native parts slots 0/2/3/4/5/6.
    let (variant, part) = if let Some(part) = stem.strip_prefix("m_") {
        (0, part)
    } else {
        (1, stem.strip_prefix("f_")?)
    };
    for (prefix, part_code) in [
        ("leg", 6),
        ("hair", 2),
        ("head", 2),
        ("body", 3),
        ("arm", 4),
        ("wst", 5),
    ] {
        if let Some(number) = part.strip_prefix(prefix) {
            return Some(EquipmentModel {
                model_id: decimal(number, 3)?,
                part_code,
                variant: Some(variant),
            });
        }
    }
    None
}

impl ResourceRef {
    pub fn equipment_model(&self) -> Option<EquipmentModel> {
        let mut current = self.node;
        let mut seen = vec![false; self.document.nodes.len()];
        loop {
            if std::mem::replace(&mut seen[current], true) {
                return None;
            }
            if current == self.document.root {
                return filename(&self.document.nodes[current].name);
            }
            let (parent, node) = self.document.nodes.iter().enumerate().find(|(_, node)| {
                node.kind != super::Kind::StageResourceReference && node.children.contains(&current)
            })?;
            if node.kind == super::Kind::Mha {
                // The tree caption contains an ordinal prefix. Read the actual
                // archive member name, independently of inspector formatting.
                let bytes = self.document.bytes(parent)?;
                let archive = MhaArchive::parse(bytes, bytes.len()).ok()?;
                let member = node.children.iter().position(|&child| child == current)?;
                let name = std::str::from_utf8(archive.entries.get(member)?.name).ok()?;
                if let Some(identity) = filename(name) {
                    return Some(identity);
                }
            }
            current = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_filename_builders_distinguish_equipment_parts_and_modes() {
        let binding = |part_code, variant, model_id| ModelEffectBinding {
            part_code,
            weapon_class: 7,
            variant,
            model_id,
            definition_ids: [0; 8],
        };
        let weapon = filename("dat/weapon/wi521.bin").unwrap();
        assert_eq!(weapon.model_id, 4521);
        assert!(weapon.matches(&binding(1, 0, 4521)));
        assert!(weapon.matches(&binding(1, 1, 4521)));
        assert!(!weapon.matches(&binding(1, 2, 4521)));
        assert!(!weapon.matches(&binding(3, 0, 4521)));
        assert_eq!(filename("wi521-so.bin"), Some(weapon));
        assert!(
            filename("n47w4521.bin")
                .unwrap()
                .matches(&binding(1, 2, 4521))
        );
        assert!(
            filename("n47h0042.bin")
                .unwrap()
                .matches(&binding(2, 2, 42))
        );
        assert!(
            filename("n47b1234.bin")
                .unwrap()
                .matches(&binding(3, 2, 1234))
        );
        assert!(
            filename("parts/f01/f_body1234.bin")
                .unwrap()
                .matches(&binding(3, 1, 1234))
        );
        assert!(
            !filename("m_body1234.bin")
                .unwrap()
                .matches(&binding(3, 1, 1234))
        );
        assert!(
            filename("m_leg012.bin")
                .unwrap()
                .matches(&binding(6, 0, 12))
        );
        for name in [
            "wi52.bin",
            "wi0521.bin",
            "wi521-copy.bin",
            "m_body12.bin",
            "m_body00012.bin",
            "n47b042.bin",
            "n47arm0042.bin",
            "em001.pac",
            "model4521.bin",
        ] {
            assert!(filename(name).is_none(), "{name}");
        }
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; verifies identity through original named containers"]
    fn original_named_weapon_resource_has_its_native_model_id() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let document = std::sync::Arc::new(crate::inspect::inspect(
            "wi500.abn",
            std::fs::read(root.join("dat/extend/wi500.abn"))
                .unwrap()
                .into(),
        ));
        let groups = super::super::AssetBundle::find_with_nodes(document).0;
        let weapon = groups
            .iter()
            .find(|group| group.name.contains("wi521.bin"))
            .unwrap();
        assert_eq!(weapon.model.equipment_model().unwrap().model_id, 4521);
    }
}
