//! Equipment identity uses the same resource-tree scopes as other metadata.

use super::ResourceRef;
use crate::metadata::EquipmentModel;

impl ResourceRef {
    pub fn equipment_model(&self) -> Option<EquipmentModel> {
        self.scope()
            .get::<EquipmentModel>()
            .map(|resolved| *resolved.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspect::{Kind, inspect},
        metadata::from_filename,
    };
    use std::sync::Arc;

    #[test]
    fn equipment_identity_follows_loading_context_and_refreshes_from_that_context() {
        let mut document = inspect("scope.bin", Arc::from(*b"mesh"));
        let template = document.nodes[0].clone();
        let node = |kind, children: &[usize]| {
            let mut node = template.clone();
            node.kind = kind;
            node.children = children.into();
            node
        };
        document.nodes = vec![
            node(Kind::Archive, &[1, 3]),
            node(Kind::Archive, &[2]),
            node(Kind::Fmod, &[]),
            node(Kind::Archive, &[4]),
            node(Kind::StageResourceReference, &[2]),
        ];
        document.nodes[1]
            .metadata
            .insert(from_filename("wi521.bin").unwrap());
        document.nodes[3]
            .metadata
            .insert(from_filename("n47w0099.bin").unwrap());
        let document = Arc::new(document);
        let original = ResourceRef::new(document.clone(), 2);
        let referenced = ResourceRef::new(document.clone(), 4);
        assert!(original.same_source(&referenced));
        assert!(!original.same_instance(&referenced));
        assert_eq!(original.equipment_model().unwrap().model_id, 4521);
        assert_eq!(referenced.equipment_model().unwrap().model_id, 99);
        assert_eq!(
            ResourceRef::new(document.clone(), 3).loadable_resources()[0]
                .equipment_model()
                .unwrap()
                .model_id,
            99
        );
        let mut updated = (*document).clone();
        updated.nodes[3]
            .metadata
            .insert(from_filename("n47w0100.bin").unwrap());
        let updated = referenced.remap(Arc::new(updated)).unwrap();
        assert!(referenced.same_origin(&updated));
        assert_eq!(updated.equipment_model().unwrap().model_id, 100);
        assert_eq!(original.equipment_model().unwrap().model_id, 4521);
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
