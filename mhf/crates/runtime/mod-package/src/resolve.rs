use crate::{Candidate, Error, Result, diagnostics::DependencyIssueKind};
use semver::{Version, VersionReq};
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
pub(super) struct Requirement {
    pub from: Option<(String, Version)>,
    pub version: VersionReq,
}

impl Requirement {
    fn source(&self) -> String {
        self.from.as_ref().map_or_else(
            || "当前选择".into(),
            |(id, version)| format!("{id} {version}"),
        )
    }
}

#[derive(Debug)]
pub(super) struct ResolutionFailure {
    pub dependency: String,
    pub kind: DependencyIssueKind,
    pub selected: BTreeMap<String, Candidate>,
    pub message: String,
}

impl ResolutionFailure {
    fn package(candidate: &Candidate, message: String) -> Self {
        Self {
            dependency: candidate.manifest.id.clone(),
            kind: DependencyIssueKind::InvalidPackage,
            selected: BTreeMap::from([(candidate.manifest.id.clone(), candidate.clone())]),
            message,
        }
    }
}

pub(super) type Available<'a> = BTreeMap<&'a str, Vec<&'a Candidate>>;

/// Choose the newest compatible versions, backtracking across transitive
/// constraints. Explicit disabling also applies to required dependencies.
pub fn resolve(
    candidates: &[Candidate],
    selections: &BTreeMap<String, Selection>,
    defaults: &BTreeSet<String>,
    required: &BTreeSet<String>,
) -> Result<Resolved> {
    let available = available(candidates).map_err(|failure| Error::new(failure.message))?;
    solve_roots(
        &available,
        selections,
        &roots(selections, defaults, required),
    )
    .map_err(|failure| Error::new(failure.message))
}

pub(super) fn available(
    candidates: &[Candidate],
) -> std::result::Result<Available<'_>, ResolutionFailure> {
    let mut available: Available<'_> = BTreeMap::new();
    for candidate in candidates {
        candidate
            .manifest
            .validate()
            .map_err(|error| ResolutionFailure::package(candidate, error.to_string()))?;
        available
            .entry(&candidate.manifest.id)
            .or_default()
            .push(candidate);
    }
    for versions in available.values_mut() {
        versions.sort_by(|a, b| b.manifest.version.cmp(&a.manifest.version));
        for pair in versions.windows(2) {
            if pair[0].manifest.version == pair[1].manifest.version {
                return Err(ResolutionFailure::package(
                    pair[0],
                    format!(
                        "{} {}：存在多个实现来源",
                        pair[0].manifest.id, pair[0].manifest.version
                    ),
                ));
            }
        }
    }
    Ok(available)
}

pub(super) fn roots(
    selections: &BTreeMap<String, Selection>,
    defaults: &BTreeSet<String>,
    required: &BTreeSet<String>,
) -> BTreeSet<String> {
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
    roots
}

pub(super) fn solve_roots(
    available: &Available<'_>,
    selections: &BTreeMap<String, Selection>,
    roots: &BTreeSet<String>,
) -> std::result::Result<Resolved, ResolutionFailure> {
    let constraints = roots
        .iter()
        .map(|id| {
            let version = selections
                .get(id)
                .and_then(|selection| selection.version.clone())
                .unwrap_or(VersionReq::STAR);
            (
                id.clone(),
                vec![Requirement {
                    from: None,
                    version,
                }],
            )
        })
        .collect();
    solve(available, selections, constraints)
}

pub(super) fn solve(
    available: &Available<'_>,
    selections: &BTreeMap<String, Selection>,
    constraints: BTreeMap<String, Vec<Requirement>>,
) -> std::result::Result<Resolved, ResolutionFailure> {
    let ordered = choose(available, selections, BTreeMap::new(), constraints)?;
    Ok(Resolved {
        mods: ordered.into_iter().cloned().collect(),
    })
}

fn choose<'a>(
    available: &Available<'a>,
    selections: &BTreeMap<String, Selection>,
    selected: BTreeMap<String, &'a Candidate>,
    constraints: BTreeMap<String, Vec<Requirement>>,
) -> std::result::Result<Vec<&'a Candidate>, ResolutionFailure> {
    for (id, requirements) in &constraints {
        if selections.get(id).and_then(|s| s.enabled) == Some(false) {
            return Err(failure(
                id,
                DependencyIssueKind::Disabled,
                &selected,
                format!(
                    "{id}：已禁用，但仍被以下来源依赖：{}",
                    requirements
                        .iter()
                        .map(Requirement::source)
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            ));
        }
        if let Some(candidate) = selected.get(id)
            && !matches(candidate, requirements, selections.get(id))
        {
            return Err(conflict(
                id,
                DependencyIssueKind::VersionConflict,
                requirements,
                &selected,
            ));
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
        return Ok(ordered);
    };
    let mut failure = conflict(
        id,
        if available.contains_key(id.as_str()) {
            DependencyIssueKind::VersionConflict
        } else {
            DependencyIssueKind::Missing
        },
        requirements,
        &selected,
    );
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
                    from: Some((
                        candidate.manifest.id.clone(),
                        candidate.manifest.version.clone(),
                    )),
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

fn conflict(
    id: &str,
    kind: DependencyIssueKind,
    requirements: &[Requirement],
    selected: &BTreeMap<String, &Candidate>,
) -> ResolutionFailure {
    failure(
        id,
        kind,
        selected,
        format!(
            "{id}：没有兼容版本（{}）",
            requirements
                .iter()
                .map(|requirement| format!(
                    "{} 要求版本 {}",
                    requirement.source(),
                    requirement.version
                ))
                .collect::<Vec<_>>()
                .join("；")
        ),
    )
}

fn failure(
    id: &str,
    kind: DependencyIssueKind,
    selected: &BTreeMap<String, &Candidate>,
    message: String,
) -> ResolutionFailure {
    ResolutionFailure {
        dependency: id.into(),
        kind,
        selected: selected
            .iter()
            .map(|(id, candidate)| (id.clone(), (*candidate).clone()))
            .collect(),
        message,
    }
}

fn visit<'a>(
    id: &str,
    selected: &BTreeMap<String, &'a Candidate>,
    visited: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    ordered: &mut Vec<&'a Candidate>,
) -> std::result::Result<(), ResolutionFailure> {
    if visited.contains(id) {
        return Ok(());
    }
    if let Some(start) = stack.iter().position(|item| item == id) {
        return Err(failure(
            id,
            DependencyIssueKind::Cycle,
            selected,
            format!("存在循环依赖：{} -> {id}", stack[start..].join(" -> ")),
        ));
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
