//! Count the same scoped instances as loading, sharing work across subtrees.

use super::{is_loadable_resource, resource_node};
use crate::{
    inspect::Document,
    metadata::{Origin, Scope},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Hash, PartialEq, Eq)]
struct Instance {
    payload: usize,
    origins: Vec<Origin>,
}

struct State {
    payload: usize,
    parents: Vec<usize>,
}

/// Ignore detail-only branches before creating metadata keys. These account
/// for most nodes in expanded DAT tables and model inspectors.
fn reachable(document: &Document, payloads: &[Option<usize>]) -> Vec<bool> {
    let mut parents = vec![Vec::new(); document.nodes.len()];
    let mut found = vec![false; document.nodes.len()];
    let mut pending = Vec::new();
    for (index, payload) in payloads.iter().enumerate() {
        let Some(payload) = payload else { continue };
        let node = &document.nodes[*payload];
        if is_loadable_resource(node.kind) {
            found[index] = true;
            pending.push(index);
        } else {
            for &child in &node.children {
                if let Some(parents) = parents.get_mut(child) {
                    parents.push(index);
                }
            }
        }
    }
    while let Some(child) = pending.pop() {
        for &parent in &parents[child] {
            if !std::mem::replace(&mut found[parent], true) {
                pending.push(parent);
            }
        }
    }
    found
}

struct Traversal<'doc> {
    document: &'doc Document,
    payloads: Vec<Option<usize>>,
    reachable: Vec<bool>,
    instances: HashMap<Instance, usize>,
    states: Vec<State>,
    pending: Vec<(usize, Vec<usize>)>,
}

impl Traversal<'_> {
    fn intern(&mut self, mut path: Vec<usize>) -> Option<usize> {
        let mut node = *path.last()?;
        if !self.reachable.get(node).copied()? {
            return None;
        }
        let payload = self.payloads[node]?;
        while node != payload {
            node = self.document.nodes[node].children[0];
            path.push(node);
        }
        let instance = Instance {
            payload,
            origins: Scope::new(self.document, &path).origins(),
        };
        if let Some(&index) = self.instances.get(&instance) {
            return Some(index);
        }
        let index = self.states.len();
        self.instances.insert(instance, index);
        self.states.push(State {
            payload,
            parents: Vec::new(),
        });
        self.pending.push((index, path));
        Some(index)
    }
}

pub(crate) fn loadable_resource_counts(document: &Document) -> Vec<usize> {
    let payloads = (0..document.nodes.len())
        .map(|node| resource_node(document, node).ok())
        .collect::<Vec<_>>();
    let mut traversal = Traversal {
        reachable: reachable(document, &payloads),
        document,
        payloads,
        instances: HashMap::new(),
        states: Vec::new(),
        pending: Vec::new(),
    };
    // Build the physical parent index once. Traversed reference edges append
    // their actual context below, without inheriting the target's old parents.
    let scopes = document.metadata();
    let starts = (0..document.nodes.len())
        .map(|node| {
            if !traversal.reachable[node] {
                return None;
            }
            traversal.intern(scopes.path(node).unwrap_or_else(|| vec![node]))
        })
        .collect::<Vec<_>>();
    let mut terminals = Vec::new();
    while let Some((index, path)) = traversal.pending.pop() {
        let node = &document.nodes[traversal.states[index].payload];
        if is_loadable_resource(node.kind) {
            terminals.push(index);
            continue;
        }
        for &child in &node.children {
            let mut context = path.clone();
            context.push(child);
            if let Some(child) = traversal.intern(context) {
                traversal.states[child].parents.push(index);
            }
        }
    }

    // A state is a payload plus its effective declaration origins. Descendant
    // behavior depends on those declarations, not the particular alias used
    // to reach them. Propagate terminal identities through this shared graph.
    // The worklist also handles malformed container cycles without recursion.
    let empty = Arc::new(HashSet::<usize>::new());
    let mut contents = vec![empty; traversal.states.len()];
    let mut queued = vec![false; contents.len()];
    let mut pending = VecDeque::new();
    for index in terminals {
        contents[index] = Arc::new(HashSet::from([index]));
        queued[index] = true;
        pending.push_back(index);
    }
    for state in &mut traversal.states {
        state.parents.sort_unstable();
        state.parents.dedup();
    }
    while let Some(child) = pending.pop_front() {
        queued[child] = false;
        let resources = contents[child].clone();
        for &parent in &traversal.states[child].parents {
            if contents[parent].is_empty() {
                contents[parent] = resources.clone();
            } else if resources
                .iter()
                .any(|resource| !contents[parent].contains(resource))
            {
                Arc::make_mut(&mut contents[parent]).extend(resources.iter().copied());
            } else {
                continue;
            }
            if !std::mem::replace(&mut queued[parent], true) {
                pending.push_back(parent);
            }
        }
    }
    starts
        .into_iter()
        .map(|state| state.map_or(0, |index| contents[index].len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspect::{Kind, Node},
        preview::ResourceRef,
    };

    fn node(kind: Kind, children: Vec<usize>) -> Node {
        Node {
            name: String::new(),
            kind,
            buffer: 0,
            range: 0..4,
            fields: Vec::new(),
            metadata: Default::default(),
            children,
            deferred: false,
            error: None,
        }
    }

    fn matches_loading(document: Document) -> Vec<usize> {
        let counts = loadable_resource_counts(&document);
        let document = Arc::new(document);
        for (node, &count) in counts.iter().enumerate() {
            assert_eq!(
                count,
                ResourceRef::new(document.clone(), node)
                    .loadable_resources()
                    .len(),
                "node {node}"
            );
        }
        counts
    }

    #[test]
    fn all_resource_kinds_keep_distinct_parent_scopes_through_shared_containers() {
        let effect = mhf_resource::dat::DATA_TABLES.len();
        let kinds = [
            Kind::Fmod,
            Kind::Fskl,
            Kind::Dds,
            Kind::Png,
            Kind::Motion,
            Kind::DatRecord(effect),
            Kind::DatRecord(effect + 1),
            Kind::DatRecord(effect + 2),
            Kind::DatRecord(effect + 3),
        ];
        let mut document = Document {
            root: 0,
            buffers: vec![Arc::from(*b"data")],
            nodes: vec![
                node(Kind::Archive, vec![1, 2, 3, 4]),
                node(Kind::Ecd, vec![5]),
                node(Kind::StageResourceReference, vec![1]),
                node(Kind::StageResourceReference, vec![1]),
                node(Kind::StageResourceReference, vec![1]),
                node(Kind::Archive, (6..6 + kinds.len()).collect()),
            ],
        };
        document
            .nodes
            .extend(kinds.into_iter().map(|kind| node(kind, Vec::new())));
        document.nodes[5].metadata.insert(1_u32);
        document.nodes[2].metadata.insert(2_u32);
        document.nodes[3].metadata.insert(3_u32);
        let counts = matches_loading(document.clone());
        assert_eq!(
            counts[0],
            kinds.len() * 3,
            "the fourth reference shares the original scope"
        );
        for &count in &counts[1..=5] {
            assert_eq!(count, kinds.len());
        }
        // A child declaration shadows every importing container's default.
        document.nodes[6].metadata.insert(9_u32);
        assert_eq!(matches_loading(document)[0], kinds.len() * 3 - 2);
    }

    #[test]
    fn complete_resources_hide_details_and_broken_wrappers_stay_empty() {
        let document = Document {
            root: 0,
            buffers: vec![Arc::from(*b"data")],
            nodes: vec![
                node(Kind::Archive, vec![1, 3]),
                node(Kind::Fmod, vec![2]),
                node(Kind::Png, Vec::new()),
                node(Kind::Jkr, vec![4]),
                node(Kind::Ecd, vec![3]),
            ],
        };
        assert_eq!(matches_loading(document), [1, 1, 1, 0, 0]);
    }

    #[test]
    fn shared_reachability_converges_even_for_invalid_container_cycles() {
        let document = Document {
            root: 0,
            buffers: vec![Arc::from(*b"data")],
            nodes: vec![
                node(Kind::Archive, vec![1]),
                node(Kind::Archive, vec![2, 3]),
                node(Kind::Archive, vec![1]),
                node(Kind::Fskl, Vec::new()),
            ],
        };
        assert_eq!(matches_loading(document), [1, 1, 1, 1]);
    }
}
