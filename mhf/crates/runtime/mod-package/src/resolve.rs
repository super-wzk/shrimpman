use crate::{Candidate, Error, Result};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Selection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionReq>,
}

#[derive(Clone, Debug)]
pub struct Resolved {
    /// Each provider precedes its consumers. One candidate per mod ID.
    pub mods: Vec<Candidate>,
}

#[derive(Clone)]
struct Requirement {
    from: String,
    version: VersionReq,
}

/// Choose the newest compatible versions, backtracking across transitive
/// constraints. Explicit disabling also applies to required dependencies.
pub fn resolve(
    candidates: &[Candidate],
    selections: &BTreeMap<String, Selection>,
    defaults: &BTreeSet<String>,
    required: &BTreeSet<String>,
) -> Result<Resolved> {
    let mut available: BTreeMap<&str, Vec<&Candidate>> = BTreeMap::new();
    for candidate in candidates {
        candidate.manifest.validate()?;
        available
            .entry(&candidate.manifest.id)
            .or_default()
            .push(candidate);
    }
    for versions in available.values_mut() {
        versions.sort_by(|a, b| b.manifest.version.cmp(&a.manifest.version));
        for pair in versions.windows(2) {
            if pair[0].manifest.version == pair[1].manifest.version {
                return Err(Error::new(format!(
                    "{} {}: multiple implementation sources",
                    pair[0].manifest.id, pair[0].manifest.version
                )));
            }
        }
    }
    let mut roots = required.clone();
    roots.extend(
        defaults
            .iter()
            .filter(|id| selections.get(*id).and_then(|s| s.enabled) != Some(false))
            .cloned(),
    );
    roots.extend(
        selections
            .iter()
            .filter(|(_, selection)| selection.enabled == Some(true))
            .map(|(id, _)| id.clone()),
    );
    let constraints = roots
        .into_iter()
        .map(|id| {
            let version = selections
                .get(&id)
                .and_then(|selection| selection.version.clone())
                .unwrap_or(VersionReq::STAR);
            (
                id,
                vec![Requirement {
                    from: "selection".into(),
                    version,
                }],
            )
        })
        .collect();
    let selected = choose(&available, selections, BTreeMap::new(), constraints)?;
    let mut ordered = Vec::new();
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    for id in selected.keys() {
        visit(id, &selected, &mut visited, &mut stack, &mut ordered)?;
    }
    Ok(Resolved {
        mods: ordered.into_iter().cloned().collect(),
    })
}

fn choose<'a>(
    available: &BTreeMap<&str, Vec<&'a Candidate>>,
    selections: &BTreeMap<String, Selection>,
    selected: BTreeMap<String, &'a Candidate>,
    constraints: BTreeMap<String, Vec<Requirement>>,
) -> Result<BTreeMap<String, &'a Candidate>> {
    for (id, requirements) in &constraints {
        if selections.get(id).and_then(|s| s.enabled) == Some(false) {
            return Err(Error::new(format!(
                "{id}: explicitly disabled, required by {}",
                requirements
                    .iter()
                    .map(|r| r.from.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if let Some(candidate) = selected.get(id)
            && !matches(candidate, requirements, selections.get(id))
        {
            return Err(conflict(id, requirements));
        }
    }
    let Some((id, requirements)) = constraints
        .iter()
        .find(|(id, _)| !selected.contains_key(*id))
    else {
        // Cycles may depend on the chosen version, so reject here while version
        // backtracking is still possible, not only when building the final order.
        let mut ordered = Vec::new();
        let mut visited = BTreeSet::new();
        for id in selected.keys() {
            visit(id, &selected, &mut visited, &mut Vec::new(), &mut ordered)?;
        }
        return Ok(selected);
    };
    let mut failure = conflict(id, requirements);
    for candidate in available.get(id.as_str()).into_iter().flatten() {
        if !matches(candidate, requirements, selections.get(id)) {
            continue;
        }
        let mut next = selected.clone();
        next.insert(id.clone(), *candidate);
        let mut constraints = constraints.clone();
        for (dependency, version) in &candidate.manifest.dependencies {
            constraints
                .entry(dependency.clone())
                .or_default()
                .push(Requirement {
                    from: format!("{} {}", candidate.manifest.id, candidate.manifest.version),
                    version: version.clone(),
                });
        }
        match choose(available, selections, next, constraints) {
            Ok(solution) => return Ok(solution),
            Err(error) => failure = error,
        }
    }
    Err(failure)
}

fn matches(
    candidate: &Candidate,
    requirements: &[Requirement],
    selection: Option<&Selection>,
) -> bool {
    requirements
        .iter()
        .all(|r| r.version.matches(&candidate.manifest.version))
        && selection
            .and_then(|s| s.version.as_ref())
            .is_none_or(|r| r.matches(&candidate.manifest.version))
}

fn conflict(id: &str, requirements: &[Requirement]) -> Error {
    Error::new(format!(
        "{id}: no compatible version ({})",
        requirements
            .iter()
            .map(|r| format!("{} requires {}", r.from, r.version))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn visit<'a>(
    id: &str,
    selected: &BTreeMap<String, &'a Candidate>,
    visited: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    ordered: &mut Vec<&'a Candidate>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if let Some(start) = stack.iter().position(|item| item == id) {
        return Err(Error::new(format!(
            "dependency cycle: {} -> {id}",
            stack[start..].join(" -> ")
        )));
    }
    stack.push(id.to_owned());
    let candidate = selected[id];
    for dependency in candidate.manifest.dependencies.keys() {
        visit(dependency, selected, visited, stack, ordered)?;
    }
    stack.pop();
    visited.insert(id.to_owned());
    ordered.push(candidate);
    Ok(())
}
