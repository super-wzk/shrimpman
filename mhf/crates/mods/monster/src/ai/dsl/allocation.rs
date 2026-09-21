//! Deterministic subscript allocation. Table choice follows the first reachable
//! call path, not a promise that every native call will save a return cursor.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use super::{compile::called_functions, parser::Document, slot::NativeSlot};
use crate::ai::{Error, Result};

pub(super) struct Allocation {
    pub slots: HashMap<String, NativeSlot>,
    pub automatic: BTreeSet<NativeSlot>,
}

impl Allocation {
    pub fn new(document: &Document) -> Result<Self> {
        let functions: HashMap<_, _> = document
            .functions
            .iter()
            .filter(|function| function.name != "main")
            .map(|function| (function.name.as_str(), function))
            .collect();
        let mut allocation = Self {
            slots: document.native_functions.clone(),
            automatic: BTreeSet::new(),
        };
        let mut used: BTreeSet<_> = allocation.slots.values().copied().collect();
        let mut pending = VecDeque::new();
        for body in document
            .functions
            .iter()
            .filter(|function| function.name == "main")
            .map(|function| &function.body)
            .chain(
                document
                    .states
                    .iter()
                    .filter_map(|entry| entry.body.as_ref()),
            )
            .chain(
                document
                    .events
                    .iter()
                    .filter_map(|entry| entry.body.as_ref()),
            )
        {
            pending.extend(called_functions(body).into_iter().map(|name| (name, 0)));
        }
        let mut visited = HashSet::new();
        loop {
            let Some((name, stage)) = pending.pop_front() else {
                // Unused functions still have scripts and are checked. Source
                // order also makes disconnected components deterministic.
                let Some(function) = document.functions.iter().find(|function| {
                    function.name != "main" && !visited.contains(function.name.as_str())
                }) else {
                    break;
                };
                pending.push_back((function.name.as_str(), 0));
                continue;
            };
            let Some(function) = functions.get(name) else {
                continue; // Command/action names and missing names are checked by the encoder.
            };
            if !visited.insert(name) {
                continue;
            }
            let slot = match allocation.slots.get(name) {
                Some(&slot) => slot,
                None => {
                    let tables = match stage {
                        0 => 1..=1,
                        1 => 15..=270,
                        _ => 9..=9,
                    };
                    let slot = tables
                        .flat_map(|table| {
                            (0..=u8::MAX).map(move |index| NativeSlot { table, index })
                        })
                        .find(|slot| !used.contains(slot))
                        .ok_or_else(|| {
                            Error::new(format!(
                                "no automatic subscript slot available for '{name}'"
                            ))
                        })?;
                    used.insert(slot);
                    allocation.automatic.insert(slot);
                    allocation.slots.insert(name.to_owned(), slot);
                    slot
                }
            };
            let stage = match slot.table {
                1 => 1,
                9 => stage,
                _ => 2,
            };
            pending.extend(
                called_functions(&function.body)
                    .into_iter()
                    .map(|name| (name, stage)),
            );
        }
        Ok(allocation)
    }
}

/// Functions that can run in an event lane must retain that lane's restrictions
/// on main-state transfers, even though calls no longer expand their bodies.
pub(super) fn event_functions(document: &Document) -> HashSet<&str> {
    let functions: HashMap<_, _> = document
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect();
    let mut pending: Vec<_> = document
        .events
        .iter()
        .filter_map(|entry| entry.body.as_ref())
        .flat_map(|body| called_functions(body))
        .collect();
    let mut visited = HashSet::new();
    while let Some(name) = pending.pop() {
        if let Some(function) = functions.get(name)
            && visited.insert(name)
        {
            pending.extend(called_functions(&function.body));
        }
    }
    visited
}
