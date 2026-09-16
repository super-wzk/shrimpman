//! Typed declarations follow resource context, independently of physical bytes.

use std::{
    any::{Any, TypeId, type_name},
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use crate::inspect::{Document, Kind};

mod equipment;
mod resources;
pub use equipment::{EquipmentModel, from_filename};
pub use resources::ModelResources;
pub(crate) use resources::model_resources;

trait Value: Any + fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any + fmt::Debug + Send + Sync> Value for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
struct Declaration {
    name: &'static str,
    value: Option<Arc<dyn Value>>,
}

/// Each Rust type is one atomic declaration. A block masks that type only.
#[derive(Clone, Default)]
pub struct Metadata {
    declarations: HashMap<TypeId, Declaration>,
}

impl Metadata {
    pub fn insert<T: Any + fmt::Debug + Send + Sync>(&mut self, value: T) {
        self.declarations.insert(
            TypeId::of::<T>(),
            Declaration {
                name: type_name::<T>(),
                value: Some(Arc::new(value)),
            },
        );
    }

    pub fn block<T: 'static>(&mut self) {
        self.declarations.insert(
            TypeId::of::<T>(),
            Declaration {
                name: type_name::<T>(),
                value: None,
            },
        );
    }
}

impl fmt::Debug for Metadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut entries: Vec<_> = self.declarations.values().collect();
        entries.sort_unstable_by_key(|entry| entry.name);
        formatter
            .debug_map()
            .entries(entries.into_iter().map(|entry| (entry.name, &entry.value)))
            .finish()
    }
}

#[derive(Debug)]
pub struct Resolved<'doc, T> {
    pub value: &'doc T,
    pub source: usize,
}

/// Source identity, including blocks, gives resource instances a stable key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    pub type_id: TypeId,
    pub source: usize,
}

/// A borrowed view of declarations along an actual resource traversal path.
/// Resolved values borrow the document, independently of this view or its path.
pub struct Scope<'doc, 'path> {
    document: &'doc Document,
    path: &'path [usize],
}

pub struct Scopes<'doc> {
    document: &'doc Document,
    parents: Vec<Option<usize>>,
}

impl Document {
    /// Build once for a traversal; references do not reparent their targets.
    pub fn metadata(&self) -> Scopes<'_> {
        Scopes::new(self)
    }
}

impl<'doc> Scopes<'doc> {
    fn new(document: &'doc Document) -> Self {
        let mut parents = vec![None; document.nodes.len()];
        for (parent, node) in document.nodes.iter().enumerate() {
            if node.kind == Kind::StageResourceReference {
                continue;
            }
            for &child in &node.children {
                if let Some(slot) = parents.get_mut(child) {
                    *slot = Some(parent);
                }
            }
        }
        Self { document, parents }
    }

    /// Direct ownership path; explicitly traversed reference paths are supplied
    /// to resolve_path instead of being rewritten through this parent index.
    pub fn path(&self, mut node: usize) -> Option<Vec<usize>> {
        let mut path = Vec::new();
        for _ in 0..self.document.nodes.len() {
            self.document.nodes.get(node)?;
            path.push(node);
            if node == self.document.root {
                path.reverse();
                return Some(path);
            }
            node = *self.parents.get(node)?.as_ref()?;
        }
        None
    }

    pub fn resolve<T: 'static>(&self, context_node: usize) -> Option<Resolved<'doc, T>> {
        self.resolve_path(&self.path(context_node)?)
    }

    pub fn resolve_path<T: 'static>(&self, path: &[usize]) -> Option<Resolved<'doc, T>> {
        Scope::new(self.document, path).get()
    }

    pub fn origins(&self, context_node: usize) -> Vec<Origin> {
        self.path(context_node)
            .map_or_else(Vec::new, |path| self.origins_path(&path))
    }

    pub fn origins_path(&self, path: &[usize]) -> Vec<Origin> {
        Scope::new(self.document, path).origins()
    }
}

impl<'doc, 'path> Scope<'doc, 'path> {
    pub fn new(document: &'doc Document, path: &'path [usize]) -> Self {
        Self { document, path }
    }

    pub fn get<T: 'static>(&self) -> Option<Resolved<'doc, T>> {
        for source in self.declaration_order()? {
            if let Some(declaration) = self.document.nodes[source]
                .metadata
                .declarations
                .get(&TypeId::of::<T>())
            {
                // Finding a block stops inheritance just like finding a value.
                let value = declaration.value.as_deref()?.as_any().downcast_ref::<T>()?;
                return Some(Resolved { value, source });
            }
        }
        None
    }

    pub fn origins(&self) -> Vec<Origin> {
        let Some(order) = self.declaration_order() else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut origins = Vec::new();
        for source in order {
            for &type_id in self.document.nodes[source].metadata.declarations.keys() {
                if seen.insert(type_id) {
                    origins.push(Origin { type_id, source });
                }
            }
        }
        origins.sort_unstable_by_key(|origin| origin.type_id);
        origins
    }

    fn declaration_order(&self) -> Option<Vec<usize>> {
        if !self.valid_path() {
            return None;
        }
        let path = self.path;
        let mut order = Vec::with_capacity(path.len());
        let mut end = path.len();
        while end > 0 {
            let mut start = end - 1;
            // Wrappers and references describe the same resource as their
            // payload. A real container child starts a closer resource scope.
            while start > 0 && self.document.nodes[path[start - 1]].kind.is_transparent() {
                start -= 1;
            }
            let group = &path[start..end];
            let is_reference = |source: &&usize| {
                self.document.nodes[**source].kind == Kind::StageResourceReference
            };
            // A reference supplies local declarations for its target. Resolve
            // those before defaults stored on the target's physical payload.
            order.extend(group.iter().rev().filter(is_reference).copied());
            order.extend(
                group
                    .iter()
                    .rev()
                    .filter(|source| !is_reference(source))
                    .copied(),
            );
            end = start;
        }
        Some(order)
    }

    fn valid_path(&self) -> bool {
        let path = self.path;
        if path.first() != Some(&self.document.root) || path.len() > self.document.nodes.len() {
            return false;
        }
        let mut seen = HashSet::with_capacity(path.len());
        path.iter()
            .all(|&node| self.document.nodes.get(node).is_some() && seen.insert(node))
            && path
                .windows(2)
                .all(|pair| self.document.nodes[pair[0]].children.contains(&pair[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::Node;

    #[derive(Debug, PartialEq, Eq)]
    struct Model {
        id: u16,
        variant: Option<u16>,
    }

    fn node(kind: Kind, children: &[usize]) -> Node {
        Node {
            name: String::new(),
            kind,
            buffer: 0,
            range: 0..0,
            fields: Vec::new(),
            metadata: Metadata::default(),
            children: children.into(),
            action: None,
            deferred: false,
            error: None,
        }
    }

    fn document() -> Document {
        Document {
            buffers: vec![Arc::from([])],
            root: 0,
            nodes: vec![
                node(Kind::Archive, &[1, 2]),
                node(Kind::Archive, &[3]),
                node(Kind::Archive, &[4]),
                node(Kind::Fmod, &[]),
                node(Kind::StageResourceReference, &[3]),
            ],
        }
    }

    #[test]
    fn scope_values_borrow_the_document_independently_of_the_path() {
        let mut document = document();
        document.nodes[1].metadata.insert(7_u32);
        let resolved = {
            let path = vec![0, 1, 3];
            let scope = Scope::new(&document, &path);
            assert_eq!(scope.origins(), document.metadata().origins(3));
            scope.get::<u32>().unwrap()
        };
        assert_eq!((*resolved.value, resolved.source), (7, 1));
        assert!(std::ptr::eq(
            resolved.value,
            document.metadata().resolve::<u32>(3).unwrap().value
        ));
    }

    #[test]
    fn types_inherit_independently_and_nearest_values_override_atomically() {
        let mut document = document();
        document.nodes[0].metadata.insert(Model {
            id: 7,
            variant: Some(2),
        });
        document.nodes[0].metadata.insert("shared".to_owned());
        document.nodes[1].metadata.insert(Model {
            id: 9,
            variant: None,
        });
        let scopes = document.metadata();
        let model = scopes.resolve::<Model>(3).unwrap();
        assert_eq!(
            model.value,
            &Model {
                id: 9,
                variant: None
            }
        );
        assert_eq!(model.source, 1);
        assert_eq!(scopes.resolve::<String>(3).unwrap().source, 0);
        assert_eq!(scopes.resolve::<Model>(2).unwrap().value.id, 7);
        assert!(scopes.resolve::<u32>(3).is_none());
    }

    #[test]
    fn blocks_stop_only_their_type_and_contribute_to_source_identity() {
        let mut document = document();
        document.nodes[0].metadata.insert(Model {
            id: 7,
            variant: None,
        });
        document.nodes[0].metadata.insert(123_u32);
        let original = document.clone();
        document.nodes[1].metadata.block::<Model>();
        let scopes = document.metadata();
        assert!(scopes.resolve::<Model>(3).is_none());
        assert_eq!(*scopes.resolve::<u32>(3).unwrap().value, 123);
        assert!(scopes.origins(3).contains(&Origin {
            type_id: TypeId::of::<Model>(),
            source: 1
        }));
        assert_ne!(scopes.origins(3), scopes.origins(2));
        assert_eq!(original.metadata().resolve::<Model>(3).unwrap().value.id, 7);
        document.nodes[3].metadata.insert(Model {
            id: 11,
            variant: Some(5),
        });
        assert_eq!(document.metadata().resolve::<Model>(3).unwrap().source, 3);
    }

    #[test]
    fn reference_context_uses_the_actual_path_and_keeps_physical_ownership() {
        let mut document = document();
        document.nodes[1].metadata.insert(Model {
            id: 11,
            variant: None,
        });
        document.nodes[2].metadata.insert(Model {
            id: 22,
            variant: None,
        });
        document.nodes[1]
            .metadata
            .insert("target-directory-only".to_owned());
        let scopes = document.metadata();
        assert_eq!(scopes.path(3).unwrap(), [0, 1, 3]);
        assert_eq!(scopes.path(4).unwrap(), [0, 2, 4]);
        assert_eq!(scopes.resolve::<Model>(3).unwrap().value.id, 11);
        assert_eq!(scopes.resolve::<Model>(4).unwrap().value.id, 22);
        let through_reference = [0, 2, 4, 3];
        assert_eq!(
            scopes
                .resolve_path::<Model>(&through_reference)
                .unwrap()
                .source,
            2
        );
        assert!(scopes.resolve_path::<String>(&through_reference).is_none());
        assert_eq!(scopes.origins_path(&through_reference), scopes.origins(4));
        document.nodes[4].metadata.block::<Model>();
        let scopes = document.metadata();
        assert!(scopes.resolve_path::<Model>(&through_reference).is_none());
        assert!(scopes.origins_path(&through_reference).contains(&Origin {
            type_id: TypeId::of::<Model>(),
            source: 4
        }));
    }

    #[test]
    fn reference_declarations_override_target_payload_defaults() {
        let mut document = document();
        document.nodes[3].metadata.insert(11_u32);
        document.nodes[3].metadata.insert("payload-only".to_owned());
        document.nodes[4].metadata.insert(22_u32);
        let through_reference = [0, 2, 4, 3];
        let scopes = document.metadata();
        let resolved = scopes.resolve_path::<u32>(&through_reference).unwrap();
        assert_eq!((*resolved.value, resolved.source), (22, 4));
        assert_eq!(scopes.resolve::<u32>(3).unwrap().source, 3);
        assert_eq!(
            scopes
                .resolve_path::<String>(&through_reference)
                .unwrap()
                .source,
            3
        );
        assert!(scopes.origins_path(&through_reference).contains(&Origin {
            type_id: TypeId::of::<u32>(),
            source: 4
        }));
        document.nodes[4].metadata.block::<u32>();
        let scopes = document.metadata();
        assert!(scopes.resolve_path::<u32>(&through_reference).is_none());
        assert!(scopes.origins_path(&through_reference).contains(&Origin {
            type_id: TypeId::of::<u32>(),
            source: 4
        }));
    }

    #[test]
    fn container_children_override_reference_defaults_across_transparent_wrappers() {
        let mut document = Document {
            buffers: vec![Arc::from([])],
            root: 0,
            nodes: vec![
                node(Kind::Archive, &[1, 2]),
                node(Kind::Archive, &[3]),
                node(Kind::StageResourceReference, &[3]),
                node(Kind::Ecd, &[4]),
                node(Kind::Exf, &[5]),
                node(Kind::Jkr, &[6]),
                node(Kind::Archive, &[7]),
                node(Kind::Fmod, &[]),
            ],
        };
        document.nodes[2].metadata.insert(22_u32);
        document.nodes[6].metadata.insert(11_u32);
        document.nodes[7].metadata.insert(33_u32);
        let container_path = [0, 2, 3, 4, 5, 6];
        let child_path = [0, 2, 3, 4, 5, 6, 7];
        let scopes = document.metadata();
        assert_eq!(
            scopes.resolve_path::<u32>(&container_path).unwrap().source,
            2
        );
        let resolved = scopes.resolve_path::<u32>(&child_path).unwrap();
        assert_eq!((*resolved.value, resolved.source), (33, 7));
        assert_eq!(scopes.origins_path(&child_path), scopes.origins(7));
        // Nested references share this resource scope; the nearest one wins.
        document.nodes[5].kind = Kind::StageResourceReference;
        document.nodes[5].metadata.insert(55_u32);
        let scopes = document.metadata();
        assert_eq!(
            scopes.resolve_path::<u32>(&container_path).unwrap().source,
            5
        );
        assert!(scopes.origins_path(&container_path).contains(&Origin {
            type_id: TypeId::of::<u32>(),
            source: 5
        }));
        assert_eq!(scopes.resolve_path::<u32>(&child_path).unwrap().source, 7);
    }

    #[test]
    fn origin_keys_ignore_hashmap_insertion_order_and_unrelated_ancestors() {
        let mut document = document();
        document.nodes[0].metadata.insert(1_u32);
        document.nodes[0].metadata.insert(false);
        let first = document.metadata().origins(3);
        let mut reordered = Metadata::default();
        reordered.insert(false);
        reordered.insert(1_u32);
        document.nodes[0].metadata = reordered;
        assert_eq!(first, document.metadata().origins(3));
        assert_eq!(first, document.metadata().origins(4));
        let mut instances = HashSet::new();
        assert!(instances.insert(first));
        assert!(!instances.insert(document.metadata().origins(4)));
    }

    #[test]
    fn invalid_indices_disconnected_paths_and_cycles_cannot_resolve() {
        let mut document = document();
        document.nodes[0].metadata.insert(1_u32);
        let scopes = document.metadata();
        for path in [
            vec![],
            vec![1, 3],
            vec![0, 4, 3],
            vec![0, 2, 4, 99],
            vec![0, 2, 4, 3, 4],
        ] {
            assert!(scopes.resolve_path::<u32>(&path).is_none());
            assert!(scopes.origins_path(&path).is_empty());
        }
        assert!(scopes.path(99).is_none());
        document.nodes[0].children.clear();
        document.nodes[3].children.push(1);
        assert!(document.metadata().path(3).is_none());
        assert!(document.metadata().resolve::<u32>(3).is_none());
    }
}
