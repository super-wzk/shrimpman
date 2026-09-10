use mhf_mod_package::{
    Candidate, DependencyIssueKind, Kind, Manifest, ModDiagnostic, Selection, Version, VersionReq,
    diagnose_resolution, resolve,
};
use std::collections::{BTreeMap, BTreeSet};

fn candidate(id: &str, version: &str, dependencies: &[(&str, &str)]) -> Candidate {
    Candidate::builtin(Manifest {
        schema: 1,
        id: id.into(),
        name: id.into(),
        version: version.parse().unwrap(),
        kind: Kind::Data,
        entry: None,
        dependencies: dependencies
            .iter()
            .map(|(id, version)| ((*id).into(), version.parse().unwrap()))
            .collect(),
    })
    .unwrap()
}

fn selections(entries: &[(&str, bool)]) -> BTreeMap<String, Selection> {
    entries
        .iter()
        .map(|(id, enabled)| {
            (
                (*id).into(),
                Selection {
                    enabled: Some(*enabled),
                    version: None,
                },
            )
        })
        .collect()
}

fn assert_issue(
    diagnostics: &BTreeMap<String, ModDiagnostic>,
    id: &str,
    dependency: &str,
    kind: DependencyIssueKind,
) {
    assert!(
        diagnostics[id]
            .issues
            .iter()
            .any(|issue| issue.dependency.as_deref() == Some(dependency) && issue.kind == kind),
        "{id} should report {kind:?} for {dependency}"
    );
}

fn assert_healthy(diagnostics: &BTreeMap<String, ModDiagnostic>, id: &str) {
    assert!(
        diagnostics
            .get(id)
            .is_none_or(|diagnostic| diagnostic.issues.is_empty()),
        "{id} should have no dependency issues"
    );
}

fn assert_matches_resolved(
    candidates: &[Candidate],
    selections: &BTreeMap<String, Selection>,
    defaults: &BTreeSet<String>,
    required: &BTreeSet<String>,
) -> BTreeMap<String, ModDiagnostic> {
    let resolved = resolve(candidates, selections, defaults, required).unwrap();
    let diagnostics = diagnose_resolution(candidates, selections, defaults, required);
    for (id, diagnostic) in &diagnostics {
        assert!(diagnostic.issues.is_empty(), "{id}");
    }
    for candidate in resolved.mods {
        let id = &candidate.manifest.id;
        let diagnosed = diagnostics[id].candidate.as_ref().unwrap();
        assert_eq!(diagnosed.manifest, candidate.manifest);
        assert_eq!(diagnosed.source, candidate.source);
    }
    diagnostics
}

#[test]
fn independent_roots_report_missing_and_disabled_dependencies() {
    let candidates = [
        candidate("app.missing", "1.0.0", &[("absent", "^1")]),
        candidate("app.disabled", "1.0.0", &[("disabled", "^1")]),
        candidate("disabled", "1.0.0", &[]),
    ];
    let selections = selections(&[
        ("app.missing", true),
        ("app.disabled", true),
        ("disabled", false),
    ]);
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    assert_issue(
        &diagnostics,
        "app.missing",
        "absent",
        DependencyIssueKind::Missing,
    );
    assert_issue(
        &diagnostics,
        "app.disabled",
        "disabled",
        DependencyIssueKind::Disabled,
    );
}

#[test]
fn one_root_reports_all_broken_direct_dependencies() {
    let candidates = [
        candidate("app", "1.0.0", &[("absent", "^1"), ("disabled", "^2")]),
        candidate("disabled", "2.0.0", &[]),
    ];
    let selections = selections(&[("app", true), ("disabled", false)]);
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    assert_issue(&diagnostics, "app", "absent", DependencyIssueKind::Missing);
    assert_issue(
        &diagnostics,
        "app",
        "disabled",
        DependencyIssueKind::Disabled,
    );
    assert!(diagnostics["app"].issues.iter().any(|issue| {
        issue.dependency.as_deref() == Some("disabled")
            && issue.requirement.as_ref() == Some(&VersionReq::parse("^2").unwrap())
    }));
}

#[test]
fn transitive_failure_marks_consumers_and_target_without_marking_healthy_providers() {
    let candidates = [
        candidate("app", "1.0.0", &[("middle", "^1")]),
        candidate("middle", "1.0.0", &[("base", "^1"), ("config", "^1")]),
        candidate("base", "1.0.0", &[("config", "^1")]),
        candidate("config", "1.0.0", &[]),
    ];
    let selections = selections(&[("app", true), ("base", false)]);
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    assert!(!diagnostics["app"].issues.is_empty());
    assert_issue(
        &diagnostics,
        "middle",
        "base",
        DependencyIssueKind::Disabled,
    );
    assert!(
        diagnostics["base"]
            .issues
            .iter()
            .any(|issue| issue.kind == DependencyIssueKind::Disabled)
    );
    assert_healthy(&diagnostics, "config");
}

#[test]
fn incompatible_shared_versions_mark_both_roots() {
    let candidates = [
        candidate("app.a", "1.0.0", &[("shared", "^1")]),
        candidate("app.b", "1.0.0", &[("shared", "^2")]),
        candidate("shared", "1.0.0", &[]),
        candidate("shared", "2.0.0", &[]),
    ];
    for id in ["app.a", "app.b"] {
        assert_matches_resolved(
            &candidates,
            &selections(&[(id, true)]),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
    }
    let selections = selections(&[("app.a", true), ("app.b", true)]);
    assert!(resolve(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new()).is_err());
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    for id in ["app.a", "app.b"] {
        assert_issue(
            &diagnostics,
            id,
            "shared",
            DependencyIssueKind::VersionConflict,
        );
    }
}

#[test]
fn broken_newest_version_falls_back_without_reporting_abandoned_dependencies() {
    let candidates = [
        candidate("app", "1.0.0", &[]),
        candidate("app", "2.0.0", &[("absent", "^1")]),
    ];
    let diagnostics = assert_matches_resolved(
        &candidates,
        &selections(&[("app", true)]),
        &BTreeSet::new(),
        &BTreeSet::new(),
    );

    assert_eq!(
        diagnostics["app"]
            .candidate
            .as_ref()
            .unwrap()
            .manifest
            .version,
        Version::new(1, 0, 0)
    );
    assert_healthy(&diagnostics, "absent");
}

#[test]
fn unrelated_failure_does_not_report_an_abandoned_version_of_a_healthy_root() {
    let candidates = [
        candidate("app.a", "1.0.0", &[("absent.a", "^1")]),
        candidate("app.a", "2.0.0", &[]),
        candidate("app.b", "1.0.0", &[("absent.b", "^1")]),
    ];
    let selections = selections(&[("app.a", true), ("app.b", true)]);
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    assert_healthy(&diagnostics, "app.a");
    assert_healthy(&diagnostics, "absent.a");
    assert_eq!(
        diagnostics["app.a"]
            .candidate
            .as_ref()
            .unwrap()
            .manifest
            .version,
        Version::new(2, 0, 0)
    );
    assert_issue(
        &diagnostics,
        "app.b",
        "absent.b",
        DependencyIssueKind::Missing,
    );
}

#[test]
fn default_and_required_roots_match_successful_resolution() {
    let candidates = [
        candidate("base", "1.0.0", &[("config", "^1")]),
        candidate("login", "1.0.0", &[("config", "^1")]),
        candidate("config", "1.0.0", &[]),
    ];
    let defaults = BTreeSet::from(["login".into()]);
    let required = BTreeSet::from(["base".into()]);
    for selections in [BTreeMap::new(), selections(&[("login", false)])] {
        assert_matches_resolved(&candidates, &selections, &defaults, &required);
    }
    assert_matches_resolved(
        &candidates,
        &selections(&[("absent.default", false)]),
        &BTreeSet::from(["absent.default".into()]),
        &required,
    );
}

#[test]
fn explicitly_disabled_required_root_is_reported_without_marking_independent_defaults() {
    let candidates = [
        candidate("base", "1.0.0", &[("config", "^1")]),
        candidate("login", "1.0.0", &[("config", "^1")]),
        candidate("config", "1.0.0", &[]),
    ];
    let selections = selections(&[("base", false)]);
    let defaults = BTreeSet::from(["login".into()]);
    let required = BTreeSet::from(["base".into()]);
    assert!(resolve(&candidates, &selections, &defaults, &required).is_err());
    let diagnostics = diagnose_resolution(&candidates, &selections, &defaults, &required);

    assert!(
        diagnostics["base"]
            .issues
            .iter()
            .any(|issue| issue.kind == DependencyIssueKind::Disabled)
    );
    assert_healthy(&diagnostics, "login");
    assert_healthy(&diagnostics, "config");
}

#[test]
fn three_incompatible_roots_are_all_reported() {
    let candidates = [
        candidate("app.a", "1.0.0", &[("shared", "^1")]),
        candidate("app.b", "1.0.0", &[("shared", "^2")]),
        candidate("app.c", "1.0.0", &[("shared", "^3")]),
        candidate("shared", "1.0.0", &[]),
        candidate("shared", "2.0.0", &[]),
        candidate("shared", "3.0.0", &[]),
    ];
    let selections = selections(&[("app.a", true), ("app.b", true), ("app.c", true)]);
    assert!(resolve(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new()).is_err());
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    for id in ["app.a", "app.b", "app.c"] {
        assert_issue(
            &diagnostics,
            id,
            "shared",
            DependencyIssueKind::VersionConflict,
        );
    }
}

#[test]
fn one_roots_joint_dependency_conflict_marks_both_branches_and_preserves_healthy_providers() {
    let candidates = [
        candidate(
            "app",
            "1.0.0",
            &[("a", "^1"), ("b", "^1"), ("config", "^1")],
        ),
        candidate("a", "1.0.0", &[("shared", "^1")]),
        candidate("b", "1.0.0", &[("shared", "^2")]),
        candidate("shared", "1.0.0", &[]),
        candidate("shared", "2.0.0", &[]),
        candidate("config", "1.0.0", &[]),
    ];
    for id in ["a", "b"] {
        assert_matches_resolved(
            &candidates,
            &selections(&[(id, true)]),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
    }
    let selections = selections(&[("app", true)]);
    assert!(resolve(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new()).is_err());
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    for id in ["a", "b"] {
        assert_issue(
            &diagnostics,
            id,
            "shared",
            DependencyIssueKind::VersionConflict,
        );
    }
    assert!(diagnostics["app"].issues.iter().any(|issue| {
        matches!(issue.dependency.as_deref(), Some("a" | "b"))
            && matches!(
                issue.kind,
                DependencyIssueKind::DependencyFailed | DependencyIssueKind::VersionConflict
            )
    }));
    assert!(
        diagnostics["shared"]
            .issues
            .iter()
            .any(|issue| issue.kind == DependencyIssueKind::VersionConflict)
    );
    assert_healthy(&diagnostics, "config");
}

#[test]
fn unavoidable_cycle_marks_both_members_without_marking_healthy_providers() {
    let candidates = [
        candidate("a", "1.0.0", &[("b", "^1"), ("config", "^1")]),
        candidate("b", "1.0.0", &[("a", "^1")]),
        candidate("config", "1.0.0", &[]),
    ];
    let selections = selections(&[("a", true)]);
    assert!(resolve(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new()).is_err());
    let diagnostics =
        diagnose_resolution(&candidates, &selections, &BTreeSet::new(), &BTreeSet::new());

    for id in ["a", "b"] {
        assert!(
            diagnostics[id].issues.iter().any(|issue| matches!(
                issue.kind,
                DependencyIssueKind::Cycle | DependencyIssueKind::DependencyFailed
            )),
            "{id} should report its circular dependency"
        );
    }
    assert_healthy(&diagnostics, "config");
}
