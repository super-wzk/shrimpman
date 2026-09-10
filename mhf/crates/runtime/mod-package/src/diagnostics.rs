use std::collections::{BTreeMap, BTreeSet};

use crate::resolve::{self, Available, Requirement, ResolutionFailure};
use crate::{Candidate, Resolved, Selection, VersionReq};

/// The exact candidate inspected for a Mod and the problems belonging to it.
/// A missing or unavailable selection has no candidate. Healthy selections have no issues.
#[derive(Clone, Debug, Default)]
pub struct ModDiagnostic {
    pub candidate: Option<Candidate>,
    pub issues: Vec<DependencyIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyIssue {
    /// `None` denotes a problem with this Mod's own selection.
    pub dependency: Option<String>,
    pub requirement: Option<VersionReq>,
    pub kind: DependencyIssueKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyIssueKind {
    Missing,
    Disabled,
    VersionConflict,
    Cycle,
    DependencyFailed,
    InvalidPackage,
    InvalidVersion,
}

/// Diagnose the same roots and version constraints as [`crate::resolve`].
///
/// A successful resolution is returned without retaining rejected candidates' errors.
/// On failure, roots are checked independently before inspecting joint conflicts, so an
/// unrelated broken Mod cannot make a healthy Mod's rejected version look selected.
pub fn diagnose_resolution(
    candidates: &[Candidate],
    selections: &BTreeMap<String, Selection>,
    defaults: &BTreeSet<String>,
    required: &BTreeSet<String>,
) -> BTreeMap<String, ModDiagnostic> {
    let available = match resolve::available(candidates) {
        Ok(available) => available,
        Err(failure) => {
            return BTreeMap::from([(
                failure.dependency.clone(),
                ModDiagnostic {
                    candidate: failure.selected.get(&failure.dependency).cloned(),
                    issues: vec![DependencyIssue {
                        dependency: None,
                        requirement: None,
                        kind: DependencyIssueKind::InvalidPackage,
                        message: failure.message,
                    }],
                },
            )]);
        }
    };
    let roots = resolve::roots(selections, defaults, required);
    let mut inspector = Inspector {
        available: &available,
        selections,
        diagnostics: BTreeMap::new(),
    };
    if let Ok(resolved) = resolve::solve_roots(&available, selections, &roots) {
        inspector.healthy(&resolved.mods);
        return inspector.diagnostics;
    }

    let mut healthy_roots = BTreeMap::new();
    for root in &roots {
        match resolve::solve_roots(&available, selections, &BTreeSet::from([root.clone()])) {
            Ok(resolved) => {
                inspector.healthy(&resolved.mods);
                healthy_roots.insert(root.clone(), resolved);
            }
            Err(failure) => {
                if let Some(candidate) = failure.selected.get(root) {
                    inspector.inspect(candidate, &failure, &mut BTreeSet::new());
                } else {
                    inspector.own_issue(
                        root,
                        selections
                            .get(root)
                            .and_then(|selection| selection.version.as_ref()),
                        failure.kind,
                    );
                }
            }
        }
    }
    inspector.joint_conflicts(&healthy_roots);
    inspector.diagnostics
}

struct Inspector<'a, 'c> {
    available: &'a Available<'c>,
    selections: &'a BTreeMap<String, Selection>,
    diagnostics: BTreeMap<String, ModDiagnostic>,
}

impl Inspector<'_, '_> {
    fn healthy<'a>(&mut self, candidates: impl IntoIterator<Item = &'a Candidate>) {
        for candidate in candidates {
            let diagnostic = self
                .diagnostics
                .entry(candidate.manifest.id.clone())
                .or_default();
            if diagnostic.issues.is_empty() {
                diagnostic.candidate = Some(candidate.clone());
            }
        }
    }

    fn own_issue(&mut self, id: &str, requirement: Option<&VersionReq>, kind: DependencyIssueKind) {
        let diagnostic = self.diagnostics.entry(id.into()).or_default();
        if diagnostic.issues.is_empty() {
            diagnostic.candidate = None;
        }
        push_issue(diagnostic, issue(id, None, requirement, kind));
    }

    fn dependency_issue(
        &mut self,
        candidate: &Candidate,
        dependency: &str,
        requirement: &VersionReq,
        kind: DependencyIssueKind,
    ) {
        let mut problem = issue(
            &candidate.manifest.id,
            Some(dependency),
            Some(requirement),
            kind,
        );
        if kind == DependencyIssueKind::DependencyFailed
            && let Some((dependency, cause)) =
                self.diagnostics.get(dependency).and_then(|diagnostic| {
                    diagnostic.issues.iter().find_map(|cause| {
                        cause
                            .dependency
                            .as_deref()
                            .map(|dependency| (dependency, cause))
                    })
                })
        {
            problem.message = if cause.kind == DependencyIssueKind::DependencyFailed {
                cause.message.clone()
            } else {
                format!("依赖的 {dependency} {}", cause.message)
            };
        }
        let diagnostic = self
            .diagnostics
            .entry(candidate.manifest.id.clone())
            .or_default();
        // The public report has one inspected candidate per Mod. Never attach another
        // version's dependency edges to a candidate that already has diagnostics.
        if !diagnostic.issues.is_empty()
            && diagnostic
                .candidate
                .as_ref()
                .is_some_and(|current| !same_candidate(current, candidate))
        {
            return;
        }
        diagnostic.candidate = Some(candidate.clone());
        push_issue(diagnostic, problem);
    }

    fn inspect(
        &mut self,
        candidate: &Candidate,
        failure: &ResolutionFailure,
        visiting: &mut BTreeSet<String>,
    ) {
        let id = &candidate.manifest.id;
        if !visiting.insert(id.clone()) {
            self.propagate_failure(failure);
            return;
        }
        self.healthy(std::iter::once(candidate));
        for (dependency, requirement) in &candidate.manifest.dependencies {
            let constraints = BTreeMap::from([(
                dependency.clone(),
                vec![Requirement {
                    from: Some((id.clone(), candidate.manifest.version.clone())),
                    version: requirement.clone(),
                }],
            )]);
            match resolve::solve(self.available, self.selections, constraints) {
                Ok(resolved) => self.healthy(&resolved.mods),
                Err(dependency_failure) => {
                    let kind = if let Some(provider) = dependency_failure.selected.get(dependency) {
                        self.inspect(provider, &dependency_failure, visiting);
                        DependencyIssueKind::DependencyFailed
                    } else {
                        let kind = dependency_failure.kind;
                        self.own_issue(dependency, Some(requirement), kind);
                        kind
                    };
                    self.dependency_issue(candidate, dependency, requirement, kind);
                }
            }
        }
        if self
            .diagnostics
            .get(id)
            .is_none_or(|diagnostic| diagnostic.issues.is_empty())
        {
            // Each dependency may work alone while their combined constraints fail.
            self.propagate_failure(failure);
        }
        visiting.remove(id);
    }

    fn propagate_failure(&mut self, failure: &ResolutionFailure) {
        self.own_issue(&failure.dependency, None, failure.kind);
        self.propagate(failure.selected.values(), &failure.dependency, failure.kind);
    }

    fn propagate<'a>(
        &mut self,
        candidates: impl Iterator<Item = &'a Candidate> + Clone,
        dependency: &str,
        kind: DependencyIssueKind,
    ) {
        let mut affected = BTreeSet::from([dependency.to_owned()]);
        loop {
            let mut added = false;
            for candidate in candidates.clone() {
                for (provider, requirement) in &candidate.manifest.dependencies {
                    if affected.contains(provider) {
                        self.dependency_issue(
                            candidate,
                            provider,
                            requirement,
                            if provider == dependency {
                                kind
                            } else {
                                DependencyIssueKind::DependencyFailed
                            },
                        );
                        added |= affected.insert(candidate.manifest.id.clone());
                    }
                }
            }
            if !added {
                break;
            }
        }
    }

    fn joint_conflicts(&mut self, roots: &BTreeMap<String, Resolved>) {
        let mut remaining: BTreeSet<_> = roots.keys().cloned().collect();
        while !remaining.is_empty() {
            if let Ok(resolved) = resolve::solve_roots(self.available, self.selections, &remaining)
            {
                self.healthy(&resolved.mods);
                break;
            }
            // Remove unrelated roots with the same solver. A failed branch alone is
            // not proof that every root (or every candidate it tried) is broken.
            let mut core = remaining.clone();
            for root in &remaining {
                let mut trial = core.clone();
                trial.remove(root);
                if !trial.is_empty()
                    && resolve::solve_roots(self.available, self.selections, &trial).is_err()
                {
                    core = trial;
                }
            }
            let mut requirements: BTreeMap<&str, Vec<(&Candidate, &VersionReq)>> = BTreeMap::new();
            for root in &core {
                for candidate in &roots[root].mods {
                    for (dependency, requirement) in &candidate.manifest.dependencies {
                        requirements
                            .entry(dependency)
                            .or_default()
                            .push((candidate, requirement));
                    }
                }
            }
            for (dependency, requirements) in requirements {
                let compatible =
                    self.available
                        .get(dependency)
                        .into_iter()
                        .flatten()
                        .any(|candidate| {
                            requirements.iter().all(|(_, requirement)| {
                                requirement.matches(&candidate.manifest.version)
                            }) && self
                                .selections
                                .get(dependency)
                                .and_then(|selection| selection.version.as_ref())
                                .is_none_or(|requirement| {
                                    requirement.matches(&candidate.manifest.version)
                                })
                        });
                if !compatible {
                    self.own_issue(dependency, None, DependencyIssueKind::VersionConflict);
                    for (candidate, requirement) in requirements {
                        self.dependency_issue(
                            candidate,
                            dependency,
                            requirement,
                            DependencyIssueKind::VersionConflict,
                        );
                    }
                    for resolved in roots.values() {
                        self.propagate(
                            resolved.mods.iter(),
                            dependency,
                            DependencyIssueKind::VersionConflict,
                        );
                    }
                }
            }
            for root in &core {
                let diagnostic = self.diagnostics.entry(root.clone()).or_default();
                if diagnostic.issues.is_empty() {
                    diagnostic.candidate = roots[root]
                        .mods
                        .iter()
                        .find(|candidate| candidate.manifest.id == *root)
                        .cloned();
                    diagnostic.issues.push(DependencyIssue {
                        dependency: None,
                        requirement: None,
                        kind: DependencyIssueKind::VersionConflict,
                        message: format!(
                            "{} 同时启用时，无法找到兼容的依赖组合",
                            core.iter().cloned().collect::<Vec<_>>().join("、")
                        ),
                    });
                }
                remaining.remove(root);
            }
        }
    }
}

fn same_candidate(left: &Candidate, right: &Candidate) -> bool {
    left.manifest.id == right.manifest.id
        && left.manifest.version == right.manifest.version
        && left.source == right.source
}

fn push_issue(diagnostic: &mut ModDiagnostic, issue: DependencyIssue) {
    if !diagnostic.issues.iter().any(|existing| {
        existing.dependency == issue.dependency
            && existing.requirement == issue.requirement
            && existing.kind == issue.kind
    }) {
        diagnostic.issues.push(issue);
    }
}

fn issue(
    id: &str,
    dependency: Option<&str>,
    requirement: Option<&VersionReq>,
    kind: DependencyIssueKind,
) -> DependencyIssue {
    let reason = match kind {
        DependencyIssueKind::Missing => "未安装",
        DependencyIssueKind::Disabled => "已禁用",
        DependencyIssueKind::VersionConflict => "没有满足当前选择的兼容版本",
        DependencyIssueKind::Cycle => "存在循环依赖",
        DependencyIssueKind::DependencyFailed => "依赖项不可用",
        DependencyIssueKind::InvalidPackage => "Mod 包无效",
        DependencyIssueKind::InvalidVersion => "版本要求无效",
    };
    let message = if dependency.is_some() {
        reason.into()
    } else {
        format!("{id}：{reason}")
    };
    DependencyIssue {
        dependency: dependency.map(str::to_owned),
        requirement: requirement.cloned(),
        kind,
        message,
    }
}
