use super::{Project, SourceFile, parse};
use crate::ai::{Node, Program, control::EVENT_SLOTS};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn script(program: &Program, root_slot: usize, index: usize) -> &[u8] {
    let Node::Table(root) = &program.nodes[program.root] else {
        panic!()
    };
    let Node::Table(table) = &program.nodes[root.get(root_slot).unwrap()] else {
        panic!()
    };
    let Node::Script(bytes) = &program.nodes[table.get(index).unwrap()] else {
        panic!()
    };
    bytes
}

fn project(entry: &str, modules: &[(&str, &str)]) -> Project {
    let mut project = Project::single(Some(31), 6, entry.into());
    project
        .files
        .extend(modules.iter().map(|(path, source)| SourceFile {
            path: (*path).into(),
            source: (*source).into(),
        }));
    project
}

#[test]
fn target_strategies_encode_in_imported_conditions_and_events() {
    for (name, opcode) in [
        ("AllowedAreas", 0x52),
        ("SameArea", 0x53),
        ("GroundFiltered", 0x5f),
        ("PlayerOrMonster", 0x7e),
        ("TrackedBySlot", 0x12),
        ("LeaderTarget", 0x58),
    ] {
        let helper = format!(
            "fn choose() {{ if self.target.available {{ self.select_target_entity(TargetStrategy::{name}); }} }}"
        );
        let p = project(
            "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.choose(); } events { awareness => h.choose; }",
            &[("maps/31/6/helper.mhai", &helper)],
        );
        let compiled = p.compile().unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [0x54, 0, opcode, 0x54, 2, 0xff, 0]
        );
        assert_eq!(
            script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
            [0x54, 0, opcode, 0x54, 2, 0xff, EVENT_SLOTS[3].ending]
        );
    }
    for body in [
        "self.select_target_entity();",
        "self.select_target_entity(256);",
        "self.select_target_entity(Mode::Attack);",
        "self.select_target_entity(TargetStrategy::Unknown);",
        "self.select_target_entity(TargetStrategy::samearea);",
        "self.select_target_entity(TargetStrategy.SameArea);",
        "self.select_target_entity(TargetStrategy::SameArea, TargetStrategy::LeaderTarget);",
        "if self.select_target_entity(TargetStrategy::SameArea) {}",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn player_slot_selection_encodes_without_implicit_binding_or_refresh() {
    for (literal, slot) in [("0", 0), ("3", 3), ("0x53", 0x53), ("255", 255)] {
        let source =
            format!("mhf_ai 1; species 6; fn main() {{ self.select_target_entity({literal}); }}");
        let compiled = parse(&source).unwrap().compile().unwrap();
        assert_eq!(script(&compiled.program, 0, 0), [6, 1, 0, slot, 0xff, 0]);
    }
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.choose(); } events { awareness => h.choose; }",
        &[(
            "maps/31/6/helper.mhai",
            "fn choose() { if self.target.available { self.select_target_entity(3); self.update_target_position(); } }",
        )],
    );
    let compiled = p.compile().unwrap();
    for (root, ending) in [(0, 0), (EVENT_SLOTS[3].root_index, EVENT_SLOTS[3].ending)] {
        assert_eq!(
            script(&compiled.program, root, 0),
            [0x54, 0, 6, 1, 0, 3, 0x4d, 0x54, 2, 0xff, ending]
        );
    }
    for body in [
        "self.select_target_entity(-1);",
        "self.select_target_entity(1.5);",
        "self.select_target_entity(3, 4);",
        "self.select_target_entity(Direction::Forward500);",
        "if self.select_target_entity(3) {}",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn mode_target_and_random_methods_encode_and_validate() {
    let p = project(
        "mhf_ai 1; species 6; map 31; fn main() { self.set_mode(Mode::Normal); self.set_mode(Mode::Attack); self.update_target_position(); self.increment_random_value(); }",
        &[],
    );
    assert_eq!(
        script(&p.compile().unwrap().program, 0, 0),
        [0x40, 0, 0x40, 1, 0x4d, 0x84, 0xff, 0]
    );
    for body in [
        "self.set_mode();",
        "self.set_mode(1);",
        "self.set_mode(Mode::Other);",
        "self.update_target_position(1);",
        "self.increment_random_value(1);",
        "self.increment_random_value;",
        "if self.update_target_position() {}",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn target_binding_methods_encode_without_arguments() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.bind(); } events { awareness => h.bind; }",
        &[(
            "maps/31/6/helper.mhai",
            "fn bind() { self.bind_awareness_target(); self.bind_current_target(); }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x11, 0x13, 0xff, 0]);
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0x11, 0x13, 0xff, EVENT_SLOTS[3].ending]
    );
    for body in [
        "self.bind_awareness_target;",
        "self.bind_awareness_target(1);",
        "self.bind_current_target;",
        "self.bind_current_target(1);",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn annotated_functions_survive_imports_disk_reload_and_renaming() {
    let dir = Directory::new();
    dir.write(
        "common/6/main.mhai",
        "mhf_ai 1; species 6; base native; import \"combat.mhai\" as c; fn main() { c.attack(); }",
    );
    dir.write("common/6/combat.mhai", "@slot(table = 1, index = 200) fn attack() { helper(); nop(); } fn helper() { wait(2); return; wait(9); }");
    let mut p = Project::load(&dir.0, 31, 6).unwrap().unwrap();
    let compiled = p.compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x81, 200, 0xff, 0]);
    assert_eq!(script(&compiled.program, 1, 200), [0x48, 2, 0x92, 0xff, 1]);
    for file in &mut p.files {
        file.source = file.source.replace("attack", "renamed");
    }
    assert_eq!(p.compile().unwrap().program, compiled.program);
    p.files.push(SourceFile {
        path: "common/6/other.mhai".into(),
        source: "@slot(table = 1, index = 200) fn other() {}".into(),
    });
    p.files[0].source = p.files[0]
        .source
        .replace("fn main", "import \"other.mhai\" as o; fn main");
    assert!(
        p.compile()
            .unwrap_err()
            .to_string()
            .contains("duplicate native slot")
    );
}

#[test]
fn invalid_slot_annotations_are_rejected() {
    for body in [
        "@slot(table = 0, index = 1) fn f() {}",
        "@slot(table = 14, index = 1) fn f() {}",
        "@slot(table = 271, index = 1) fn f() {}",
        "@slot(table = 1, index = 256) fn f() {}",
        "@slot(table = 1, index = 1) fn main() {}",
        "@slot(table = 1, index = 1) states {}",
        "@slot(table = 1, index = 1) fn f() {} @slot(table = 1, index = 1) fn g() {}",
        "@slot(table = 1, index = 1) fn f() {} states { fight => f; }",
        "@slot(table = 1, index = 1) fn f() {} events { awareness => f; }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; {body}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn native_calls_preserve_cross_level_transfers() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { secondary(); } @slot(table = 15, index = 0) fn secondary() { primary(); } @slot(table = 1, index = 0) fn primary() {}")
        .unwrap().compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x82, 0, 0, 0xff, 0]);
    assert_eq!(script(&compiled.program, 15, 0), [0x81, 0, 0xff, 2]);
    assert_eq!(script(&compiled.program, 1, 0), [0xff, 1]);
}

#[test]
fn mode_is_encodes_enum_arguments_and_rejects_invalid_calls() {
    for (name, value) in [("Normal", 0), ("Attack", 1)] {
        let compiled = parse(&format!("mhf_ai 1; species 6; fn main() {{ if self.mode_is(Mode::{name}) {{ nop(); }} else {{ reset; }} }}"))
            .unwrap().compile().unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [0x0b, 0, value, 0x92, 0x0b, 1, 0xff, 0, 0x0b, 2, 0xff, 0]
        );
    }
    for body in [
        "if self.mode_is {}",
        "if self.mode_is() {}",
        "if self.mode_is(256) {}",
        "if self.mode_is(0) {}",
        "if self.mode_is(1) {}",
        "if self.mode_is(Mode.attack) {}",
        "if self.mode_is(Mode::attack) {}",
        "if self.mode_is(Mode::Unknown) {}",
        "if self.mode_is(Other::Attack) {}",
        "if self.mode_is(Mode::Attack, Mode::Normal) {}",
        "if self.mode_is(Mode: :Attack) {}",
        "if self.mode_is(1, 2) {}",
        "if self.mode_is(self.enraged) {}",
        "self.mode_is(1);",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.check(); }",
        &[(
            "maps/31/6/helper.mhai",
            "fn check() { if self.mode_is(Mode::Attack) { nop(); } }",
        )],
    );
    assert_eq!(
        script(&p.compile().unwrap().program, 0, 0),
        [0x0b, 0, 1, 0x92, 0x0b, 2, 0xff, 0]
    );
}

#[test]
fn tracked_players_check_is_a_condition_method_with_native_side_effects() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.check(); } events { awareness => h.check; }",
        &[(
            "maps/31/6/helper.mhai",
            "fn check() { if self.check_tracked_players() { if self.target.available { nop(); } } else { reset; } if self.check_tracked_players() {} }",
        )],
    );
    let compiled = p.compile().unwrap();
    let body = [
        2, 0, 0x54, 0, 0x92, 0x54, 2, 2, 1, 0xff, 0, 2, 2, 2, 0, 2, 2,
    ];
    assert_eq!(
        script(&compiled.program, 0, 0),
        [&body[..], &[0xff, 0]].concat()
    );
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [&body[..], &[0xff, EVENT_SLOTS[3].ending]].concat()
    );
    for body in [
        "if self.check_tracked_players {}",
        "if self.check_tracked_players(1) {}",
        "self.check_tracked_players();",
        "if self.target.check_tracked_players() {}",
        "if self.flashed() {}",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn target_available_resolves_in_imported_helpers_and_events() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.check(); } events { awareness => h.check; }",
        &[(
            "maps/31/6/helper.mhai",
            "fn check() { if self.target.available { if self.enraged { nop(); } } else { reset; } }",
        )],
    );
    let compiled = p.compile().unwrap();
    let body = [0x54, 0, 0x35, 0, 0x92, 0x35, 2, 0x54, 1, 0xff, 0, 0x54, 2];
    assert_eq!(
        script(&compiled.program, 0, 0),
        [&body[..], &[0xff, 0]].concat()
    );
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [&body[..], &[0xff, EVENT_SLOTS[3].ending]].concat()
    );
    for body in [
        "if self.target {}",
        "if self.target.unknown {}",
        "if self.target.available() {}",
        "self.target.available = 1;",
        "if self.target.available.extra {}",
    ] {
        assert!(parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err());
    }
}

#[test]
fn distance_match_compiles_imports_and_native_fallback_without_changing_thresholds() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { match self.target_distance_group() { 1 => h.attack(); 2 => { self.update_target_position(); } else => reset forget_target; } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn attack() { random { 1 => nop(); } }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x83, 0, 2, 0x83, 1, 0x80, 0, 1, 0x80, 1, 32, 0x92, 0x80, 0xff, 0x83, 2, 0x4d, 0x83, 3,
            0xff, 0xf7, 0x83, 0xff, 0xff, 0
        ]
    );
    let compiled = parse("mhf_ai 1; species 6; base native; events { awareness => { match self.target_distance_group() { 1 => {} else => {} } } }").unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [
            0x83,
            0,
            1,
            0x83,
            1,
            0x83,
            2,
            0x83,
            0xff,
            0xff,
            EVENT_SLOTS[3].ending
        ]
    );
    for branches in [
        "",
        "else => {}",
        "1 => {}",
        "2 => {} else => {}",
        "1 => {} 1 => {} else => {}",
        "1 => {} 3 => {} else => {}",
        "1 => {} else => {} 2 => {}",
        "1 => {} 2 => {} 3 => {} 4 => {} 5 => {} else => {}",
    ] {
        assert!(parse(&format!("mhf_ai 1; species 6; fn main() {{ match self.target_distance_group() {{ {branches} }} }}")).is_err(), "{branches}");
    }
    for body in [
        "self.target_distance_group();",
        "if self.target_distance_group() {}",
        "match self.target_distance_group(1) { 1 => {} else => {} }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn waypoint_selection_preserves_explicit_refresh_and_checks_index_width() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { self.select_target_point(0); self.update_target_position(); action[0:1](0); self.select_target_point(255); }").unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [6, 2, 1, 0, 0x4d, 5, 0, 1, 0, 6, 2, 1, 255, 0xff, 0]
    );
    for body in [
        "self.select_target_point();",
        "self.select_target_point(256);",
        "self.select_target_point(-1);",
        "self.select_target_point(1.5);",
        "self.select_target_point(0, 1);",
        "if self.select_target_point(0) {}",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn relative_target_points_encode_only_selection_and_validate_arguments() {
    for (name, selector) in [
        ("Forward500", 0),
        ("Left500", 1),
        ("Right500", 2),
        ("Backward500", 3),
        ("Forward1000", 8),
        ("Left1000", 9),
        ("Right1000", 10),
        ("Backward1000", 11),
    ] {
        let source = format!(
            "mhf_ai 1; species 6; fn main() {{ self.select_target_point(Direction::{name}); }}"
        );
        let compiled = parse(&source).unwrap().compile().unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [6, 6, selector, 0, 0xff, 0]
        );
    }
    for body in [
        "self.select_target_point(Direction::Unknown);",
        "self.select_target_point(Direction::Forward);",
        "self.select_target_point(Direction::Forward750);",
        "self.select_target_point(Direction::forward);",
        "self.select_target_point(Direction.Forward500);",
        "self.select_target_point(Direction::Forward500, 1000);",
        "self.select_target_point(TargetStrategy::SameArea);",
        "if self.select_target_point(Direction::Forward500) {}",
        "self.select_waypoint(0);",
        "self.select_target(TargetStrategy::SameArea);",
        "self.refresh_target();",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
    for declaration in [
        "fn Direction() {}",
        "actions { Direction = [0:1]; }",
        "import \"helper.mhai\" as Direction;",
    ] {
        assert!(
            parse(&format!(
                "mhf_ai 1; species 6; {declaration} fn main() {{}}"
            ))
            .is_err(),
            "{declaration}"
        );
    }
}

#[test]
fn active_condition_encodes_nested_branches_and_validates_property_syntax() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { if self.active { if self.active { nop(); } else { self.update_target_position(); } } else { reset; } }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x08, 0, 0x08, 0, 0x92, 0x08, 1, 0x4d, 0x08, 2, 0x08, 1, 0xff, 0, 0x08, 2, 0xff, 0
        ]
    );
    for body in ["if self.active() {}", "self.active;", "self.active = 1;"] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn enraged_conditions_work_in_states_events_and_mixed_branches() {
    let compiled = parse("mhf_ai 1; species 6; events { rage_entered => react; } fn main() { if self.enraged { if self.flashed { reset; } else { nop(); } } else { restart; } } fn react() { if self.enraged { nop(); } }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x35, 0, 0x39, 0, 0xff, 0, 0x39, 1, 0x92, 0x39, 2, 0x35, 1, 4, 0x35, 2, 0xff, 0,
        ]
    );
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[4].root_index, 0),
        [0x35, 0, 0x92, 0x35, 2, 0xff, EVENT_SLOTS[4].ending]
    );
    for body in ["self.enraged = 1;", "if self.enraged() {}"] {
        assert!(parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err());
    }
}

#[test]
fn flashed_conditions_encode_nested_branches_and_keep_entry_tails() {
    let compiled = parse("mhf_ai 1; species 6; events { awareness => react; } fn main() { if self.flashed { if self.flashed { reset; } else { restart; } } else { nop(); } wait(3); } fn react() { if self.flashed { reset; } }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x39, 0, 0x39, 0, 0xff, 0, 0x39, 1, 4, 0x39, 2, 0x39, 1, 0x92, 0x39, 2, 0x48, 3, 0xff,
            0,
        ]
    );
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0x39, 0, 0xff, 0, 0x39, 2, 0xff, EVENT_SLOTS[3].ending]
    );
    let compiled = parse("mhf_ai 1; species 6; states { fight => attack; } fn main() { if self.flashed { transition fight; } restart; } fn attack() {}")
        .unwrap().compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x39, 0, 7, 1, 0x39, 2, 4]);
}

#[test]
fn condition_branches_resolve_imported_helpers_and_actions() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { if self.flashed { h.prepare(); } else { h.hit(2); } }",
        &[(
            "maps/31/6/helper.mhai",
            "actions { hit = [3:6]; } fn prepare() { if self.flashed { hit(1); } }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x39, 0, 0x39, 0, 5, 3, 6, 1, 0x39, 2, 0x39, 1, 5, 3, 6, 2, 0x39, 2, 0xff, 0,
        ]
    );
}

#[test]
fn block_returns_only_skip_the_current_functions_continuation() {
    for (body, expected) in [
        (
            "if self.flashed { return; } wait(1);",
            "if self.flashed {} else { wait(1); }",
        ),
        (
            "if self.flashed { wait(1); } else { return; } wait(2);",
            "if self.flashed { wait(1); wait(2); } else {}",
        ),
        (
            "if self.flashed { return; } else { return; } wait(9);",
            "if self.flashed {} else {}",
        ),
        (
            "if self.flashed { if self.active { return; } wait(1); } wait(2);",
            "if self.flashed { if self.active {} else { wait(1); wait(2); } } else { wait(2); }",
        ),
        (
            "if self.check_tracked_players() { return; } wait(1);",
            "if self.check_tracked_players() {} else { wait(1); }",
        ),
        (
            "random { 1 => { return; } 1 => wait(1); } wait(2);",
            "random { 1 => {} 1 => { wait(1); wait(2); } }",
        ),
        (
            "match self.target_distance_group() { 1 => { return; } else => wait(1); } wait(2);",
            "match self.target_distance_group() { 1 => {} else => { wait(1); wait(2); } }",
        ),
        (
            "if self.flashed { reset; } else { return; } wait(2);",
            "if self.flashed { reset; wait(2); } else {}",
        ),
    ] {
        for entry in [
            "fn main() { helper(); wait(3); }",
            "fn main() { if self.active { helper(); wait(3); } }",
            "events { awareness => { helper(); wait(3); } }",
        ] {
            let compile = |body| {
                parse(&format!(
                    "mhf_ai 1; species 6; base native; {entry} fn helper() {{ {body} }}"
                ))
                .unwrap()
                .compile()
                .unwrap()
            };
            let actual = compile(body);
            let expected = compile(expected);
            assert_eq!(actual.program, expected.program, "{entry}: {body}");
        }
    }
}

#[test]
fn entry_and_native_slot_returns_keep_their_endings() {
    let compiled = parse(
        "mhf_ai 1; species 6; base native;
        fn main() { if self.flashed { return; } wait(1); }
        states { idle => { if self.flashed { return; } wait(1); } }
        events { awareness => handler; }
        fn handler() { if self.flashed { return; } wait(1); }
        @slot(table = 1, index = 0) fn sub() { if self.flashed { return; } wait(1); }
        @slot(table = 15, index = 0) fn nested() { if self.flashed { return; } wait(1); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    for (root, index, ending) in [(0, 0, 0), (0, 1, 0), (3, 0, 0xfd)] {
        assert_eq!(
            script(&compiled.program, root, index),
            [0x39, 0, 0x39, 1, 0x48, 1, 0x39, 2, 0xff, ending]
        );
    }
    for (root, ending) in [(1, 1), (15, 2)] {
        assert_eq!(
            script(&compiled.program, root, 0),
            [0x39, 0, 0xff, ending, 0x39, 2, 0x48, 1, 0xff, ending]
        );
    }
}

#[test]
fn inline_returns_inside_native_slots_do_not_return_from_the_slot() {
    let compiled = parse(
        "mhf_ai 1; species 6; base native;
        @slot(table = 1, index = 0) fn sub() { if self.active { helper(); wait(3); } }
        fn helper() { if self.flashed { return; } wait(2); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        script(&compiled.program, 1, 0),
        [
            0x08, 0, 0x39, 0, 0x39, 1, 0x48, 2, 0x39, 2, 0x48, 3, 0x08, 2, 0xff, 1
        ]
    );
}

#[test]
fn raw_native_conditionals_still_reject_inline_returns() {
    for body in [
        "native(0x39, 0); return; native(0x39, 2);",
        "native(0x39, 0); helper(); native(0x39, 2);",
        "native(0x39, 0); if self.active { return; } native(0x39, 2);",
        "if self.active { native(0x39, 0); return; native(0x39, 2); }",
    ] {
        let error = parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ {body} }} fn helper() {{ return; }}"
        ))
        .unwrap()
        .compile()
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("return inside a native conditional"),
            "{error}"
        );
    }
}

#[test]
fn return_restructuring_respects_the_script_size_limit() {
    let body = "if self.flashed { if self.active { return; } }".repeat(18);
    let error = parse(&format!(
        "mhf_ai 1; species 6; fn main() {{ {body} wait(1); }}"
    ))
    .unwrap()
    .compile()
    .unwrap_err();
    assert!(error.to_string().contains("exceeds 64 KiB"), "{error}");
}

#[test]
fn invalid_conditions_fail_without_guessing_native_semantics() {
    for body in [
        "if self.unknown {}",
        "if self.flashed() {}",
        "if !self.flashed {}",
        "if self.flashed && self.flashed {}",
        "if self.flashed nop();",
        "if self.flashed {} else if self.flashed {}",
        "else {}",
        "self.flashed = 1;",
        "if self.flashed { native(0x39, 0); }",
        "if self.flashed { native(0x39, 2); }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}"))
                .and_then(|d| d.compile())
                .is_err(),
            "{body}"
        );
    }
    for name in ["if", "else", "self"] {
        assert!(parse(&format!("mhf_ai 1; species 6; fn {name}() {{}}")).is_err());
    }
}

#[test]
fn explicit_event_reset_does_not_append_the_event_tail() {
    let compiled = parse("mhf_ai 1; species 6; base native; events { awareness => handler; } fn handler() { reset; wait(9); }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0xff, 0]
    );
    for declaration in [
        "fn reset() {}",
        "fn restart() {}",
        "actions { reset = [0:1]; }",
    ] {
        assert!(parse(&format!("mhf_ai 1; species 6; base native; {declaration}")).is_err());
    }
}

#[test]
fn anonymous_entries_share_named_entry_compilation_and_imports() {
    for source in [
        "mhf_ai 1; species 6; fn main() {} states { fight -> main; }",
        "mhf_ai 1; species 6; base native; events { awareness -> {} }",
    ] {
        assert!(parse(source).is_err(), "old arrow must be rejected");
    }
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() {} states { fight = 5 => { h.attack(); } patrol => h.attack; } events { player_detected => { h.attack(); } awareness => { reset forget_target; wait(9); } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn attack() { random { 1 => nop(); 1 => { self.update_target_position(); } } }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 5),
        script(&compiled.program, 0, 6)
    );
    assert!(script(&compiled.program, 0, 5).ends_with(&[0xff, 0]));
    assert!(
        script(&compiled.program, EVENT_SLOTS[2].root_index, 0)
            .ends_with(&[0xff, EVENT_SLOTS[2].ending])
    );
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0xff, 0xf7]
    );
    let compiled = parse("mhf_ai 1; species 6; base native; events { awareness => { reset forget_target; wait(9); } }").unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0xff, 0xf7]
    );
}

#[test]
fn target_angle_bounds_quantize_clamp_and_keep_entry_endings() {
    for (min, max, lower, upper, warned) in [
        ("45", "135", 32, 96, false),
        ("1.40625", "358.59375", 1, 255, false),
        ("0.703125", "30", 1, 21, true),
        ("359", "360", 255, 255, true),
        ("90", "90", 64, 64, false),
    ] {
        let source = format!(
            "mhf_ai 1; species 6; base native; events {{ awareness => {{ if self.target_angle_in({min}, {max}) {{ nop(); }} else {{ self.update_target_position(); }} }} }}"
        );
        let compiled = parse(&source).unwrap().compile().unwrap();
        assert_eq!(
            script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
            [
                0x78,
                0,
                lower,
                upper,
                0x92,
                0x78,
                1,
                0x4d,
                0x78,
                2,
                0xff,
                EVENT_SLOTS[3].ending
            ]
        );
        assert_eq!(!compiled.warnings.is_empty(), warned);
    }
    for body in [
        "if self.target_angle_in(90, 45) {}",
        "if self.target_angle_in(-45, 45) {}",
        "if self.target_angle_in(0, 360.1) {}",
        "if self.target_angle_in(0) {}",
        "if self.target_angle_in(0, 1, 2) {}",
        "if self.target_angle_in {}",
        "self.target_angle_in(0, 90);",
        "wait(1.5);",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn explicit_zero_random_weights_keep_branches_and_do_not_receive_rounding_shares() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { random { 0 => self.update_target_position(); 1 => nop(); 1 => nop(); 1 => nop(); 0 => self.increment_random_value(); } }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x80, 0, 5, 0x80, 1, 0, 0x4d, 0x80, 2, 11, 0x92, 0x80, 3, 11, 0x92, 0x80, 4, 10, 0x92,
            0x80, 5, 0, 0x84, 0x80, 0xff, 0xff, 0
        ]
    );
    for body in [
        "random { 0 => nop(); 0 => nop(); }",
        "random { 0 => nop(); 1 => nop(); 1000 => nop(); }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}"))
                .unwrap()
                .compile()
                .is_err()
        );
    }
}

#[test]
fn random_weights_normalize_and_imported_branches_expand() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { random { 1 => h.attack(); 2 => { h.attack(); self.update_target_position(); } 1 => reset forget_target; } }",
        &[("maps/31/6/helper.mhai", "fn attack() { nop(); }")],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x80, 0, 3, 0x80, 1, 8, 0x92, 0x80, 2, 16, 0x92, 0x4d, 0x80, 3, 8, 0xff, 0xf7, 0x80,
            0xff, 0xff, 0
        ]
    );
    assert!(compiled.warnings.is_empty());
    let compiled =
        parse("mhf_ai 1; species 6; fn main() { random { 1 => nop(); 1 => nop(); 1 => nop(); } }")
            .unwrap()
            .compile()
            .unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x80, 0, 3, 0x80, 1, 11, 0x92, 0x80, 2, 11, 0x92, 0x80, 3, 10, 0x92, 0x80, 0xff, 0xff,
            0
        ]
    );
    assert!(
        compiled
            .warnings
            .iter()
            .any(|warning| warning.message.contains("11, 11, 10"))
    );
    for body in [
        "random {}",
        "random { 0 => nop(); }",
        "random { 1 -> nop(); }",
        "random { 1 => nop(); 1000 => nop(); }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}"))
                .and_then(|document| document.compile())
                .is_err(),
            "{body}"
        );
    }
}

#[test]
fn reset_forget_target_is_a_terminal_reset_variant() {
    let compiled = parse("mhf_ai 1; species 6; base native; events { awareness => handler; } fn handler() { reset forget_target; wait(9); }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0xff, 0xf7]
    );
    for body in [
        "reset forget_target();",
        "reset unknown;",
        "restart forget_target;",
        "forget_target;",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn main_is_the_unique_entry_and_restart_reenters_it() {
    let compiled = parse("mhf_ai 1; species 6; states { fight => attack; } fn main() { transition fight; } fn attack() { restart; }")
        .unwrap().compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [7, 1]);
    assert_eq!(script(&compiled.program, 0, 1), [4]);
    let compiled = parse("mhf_ai 1; species 6; fn main() {}")
        .unwrap()
        .compile()
        .unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0xff, 0]);
    for body in [
        "fn main() {} states { other = 0 => helper; } fn helper() {}",
        "fn main() {} fn main() {}",
        "fn main() { main(); }",
        "fn main() { transition main; }",
        "fn main() {} events { awareness => main; }",
        "states { main => helper; } fn helper() {}",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 6; base native; {body}"))
                .and_then(|d| d.compile())
                .is_err(),
            "{body}"
        );
    }
    let compiled = parse(
        "mhf_ai 1; species 6; base native; states { fight => helper; } fn helper() { restart; }",
    )
    .unwrap()
    .compile()
    .unwrap();
    let Node::Table(root) = &compiled.program.nodes[compiled.program.root] else {
        panic!()
    };
    let Node::Table(states) = &compiled.program.nodes[root.get(0).unwrap()] else {
        panic!()
    };
    assert!(!states.declares(0));
    let p = project(
        "mhf_ai 1; species 6; map 31; base native; import \"helper.mhai\" as h; fn main() {}",
        &[("maps/31/6/helper.mhai", "fn main() {}")],
    );
    assert!(p.compile().is_err());
}

#[test]
fn map_declaration_matches_project_scope() {
    let common = "mhf_ai 1; species 163; base native;";
    let specific = "mhf_ai 1; species 163; map 97; base native;";
    Project::single(None, 163, common.into()).compile().unwrap();
    Project::single(Some(97), 163, specific.into())
        .compile()
        .unwrap();
    assert!(
        Project::single(Some(97), 163, common.into())
            .compile()
            .is_err()
    );
    assert!(
        Project::single(None, 163, specific.into())
            .compile()
            .is_err()
    );
    assert!(parse("mhf_ai 1; species 163; map any; base native;").is_err());
}

#[test]
fn module_calls_expand_with_local_names_and_context_specific_endings() {
    let entry = "mhf_ai 1; species 6; map 31; base native;
        import \"#common/combat.mhai\" as combat;
        states { fight = 4 => fight; recovery => recovery; }
        events { dung_reaction => combat.attack; invalid_ground => combat.attack; }
        fn main() { combat.attack(); transition fight; }
        fn fight() { transition recovery; }
        fn recovery() { restart; }";
    let p = project(
        entry,
        &[
            (
                "common/6/combat.mhai",
                "import \"movement.mhai\" as move; fn attack() { move.prepare(); action[3:6](0); }",
            ),
            (
                "common/6/movement.mhai",
                "fn prepare() { wait(5); return; nop(); }",
            ),
        ],
    );
    let c = p.compile().unwrap();
    assert_eq!(script(&c.program, 0, 0), [0x48, 5, 5, 3, 6, 0, 7, 4]);
    assert_eq!(script(&c.program, 0, 4), [7, 5]);
    assert_eq!(script(&c.program, 0, 5), [4]);
    assert_eq!(
        script(&c.program, EVENT_SLOTS[0].root_index, 0),
        [0x48, 5, 5, 3, 6, 0, 0xff, 0xf5]
    );
    assert_eq!(
        script(&c.program, EVENT_SLOTS[1].root_index, 0),
        [0x48, 5, 5, 3, 6, 0, 0xff, 0xf6]
    );
}

#[test]
fn named_events_map_independently_of_order_and_get_their_verified_endings() {
    let text = "mhf_ai 1; species 6; base native;
        events { bait_detected => end; group_signal => end; rage_entered => end;
                 awareness => end; player_detected => end; invalid_ground => end; dung_reaction => end; }
        fn end() { nop(); }";
    let compiled = parse(text).unwrap().compile().unwrap();
    for (slot, ending) in [0xf5, 0xf6, 0xfc, 0xfd, 0xf8, 0xf9, 0xfa]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            script(&compiled.program, EVENT_SLOTS[slot].root_index, 0),
            [0x92, 0xff, ending]
        );
    }
    let doc =
        parse("mhf_ai 1; species 6; events { group_signal => end; dung_reaction => end; rage_entered => end; } fn end() {}")
            .unwrap();
    assert_eq!(
        doc.events.iter().map(|e| e.slot).collect::<Vec<_>>(),
        [5, 0, 4]
    );
}

#[test]
fn rejects_invalid_calls_and_bindings() {
    for body in [
        "states { a => absent; } fn end() {}",
        "states { a => end; } fn end() { end(); }",
        "states { a => end; } fn end() { other(); } fn other() { end(); }",
        "states { a => end; } fn end() { other(1); transition a; } fn other() {}",
        "events { bait_detected = 6 => end; } fn end() {}",
        "events { 0 => end; } fn end() {}",
        "events { event_0 => end; } fn end() {}",
        "events { kehai => end; } fn end() {}",
        "events { awareness => end; } fn end() { kehai_end(); }",
        "events { invalid_ground => end; } fn end() { no_floor_end(); }",
        "events { player_detected => end; } fn end() { find_end(); }",
        "events { awareness => end; awareness => end; } fn end() {}",
        "events { awareness => end; } fn end() {} fn end() {}",
    ] {
        let text = format!("mhf_ai 1; species 6; base native; {body}");
        assert!(parse(&text).and_then(|d| d.compile()).is_err(), "{text}");
    }
}

#[test]
fn state_functions_reset_on_fallthrough_without_changing_explicit_endings() {
    for (body, expected) in [
        ("", vec![0xff, 0x00]),
        ("action[0:1](0);", vec![0x05, 0, 1, 0, 0xff, 0x00]),
        ("return; wait(9);", vec![0xff, 0x00]),
        ("helper(); wait(2);", vec![0x48, 1, 0x48, 2, 0xff, 0x00]),
        ("restart; wait(9);", vec![0x04]),
        ("restart;", vec![0x04]),
        ("reset; wait(9);", vec![0xff, 0x00]),
        ("reset forget_target; wait(9);", vec![0xff, 0xf7]),
        ("reset_helper(); wait(9);", vec![0xff, 0x00]),
        ("native(0xff, 0x00);", vec![0xff, 0x00]),
        ("stop();", vec![0x68]),
    ] {
        let source = format!(
            "mhf_ai 1; species 6; base native;
             fn main() {{ {body} }} fn helper() {{ wait(1); return; wait(9); }} fn reset_helper() {{ reset; }}"
        );
        let compiled = parse(&source).unwrap().compile().unwrap();
        assert_eq!(script(&compiled.program, 0, 0), expected, "{body}");
    }
}

#[test]
fn imports_are_species_scoped_and_cycles_missing_modules_fail() {
    for path in [
        "@/common/6/a.mhai",
        "#common/../7/a.mhai",
        "../7/a.mhai",
        "C:/a.mhai",
        "/a.mhai",
        "#other/a.mhai",
    ] {
        let text = format!("mhf_ai 1; species 6; map 31; base native; import \"{path}\" as bad;");
        assert!(project(&text, &[]).compile().is_err(), "{path}");
    }
    let entry = "mhf_ai 1; species 6; map 31; base native; import \"#common/a.mhai\" as a;";
    assert!(
        project(entry, &[])
            .compile()
            .unwrap_err()
            .to_string()
            .contains("missing imported")
    );
    let p = project(entry, &[("common/6/a.mhai", "import \"a.mhai\" as again;")]);
    assert!(
        p.compile()
            .unwrap_err()
            .to_string()
            .contains("cyclic import")
    );
    let p = project(
        entry,
        &[("common/6/a.mhai", "mhf_ai 1; species 7; base native;")],
    );
    assert!(
        p.compile()
            .unwrap_err()
            .to_string()
            .contains("not another entry")
    );
}

#[test]
fn equivalent_paths_share_one_module_and_other_species_are_rejected() {
    let p = project(
        "mhf_ai 1; species 6; map 31; base native;
        import \"#common/a.mhai\" as a; import \"../../../common/6/./a.mhai\" as b;
        fn main() { a.end(); b.end(); restart; }",
        &[("common/6/a.mhai", "fn end() { nop(); }")],
    );
    assert_eq!(script(&p.compile().unwrap().program, 0, 0), [0x92, 0x92, 4]);
    assert!(p.check_target(32, 6).is_err());
    assert!(p.check_target(31, 7).is_err());
    let mut mismatch = p.clone();
    mismatch.files[0].source = mismatch.files[0].source.replace("species 6", "species 7");
    assert!(mismatch.compile().is_err());
}

#[test]
fn native_conditional_transitions_survive_function_compilation() {
    let text = "mhf_ai 1; species 6; base native;
        fn main() {
            native(0x0b,0,0); restart; native(0x0b,2); native(0xff,0);
        }";
    assert_eq!(
        script(&parse(text).unwrap().compile().unwrap().program, 0, 0),
        [0x0b, 0, 0, 4, 0x0b, 2, 0xff, 0]
    );
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "mhf-ai-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn disk_loading_falls_back_only_when_specific_entry_is_missing_and_preserves_drafts() {
    let dir = Directory::new();
    dir.write("common/6/main.mhai", "mhf_ai 1; species 6; base native; import \"helper.mhai\" as h; events { dung_reaction => h.finish; }");
    dir.write("common/6/helper.mhai", "fn finish() { nop(); }");
    let mut p = Project::load(&dir.0, 31, 6).unwrap().unwrap();
    assert_eq!(p.entry, "common/6/main.mhai");
    p.files[1].source = "fn finish() { wait(5); }".into();
    p.complete(&dir.0).unwrap();
    assert_eq!(
        script(&p.compile().unwrap().program, 14, 0),
        [0x48, 5, 0xff, 0xf5]
    );
    dir.write("maps/31/6/main.mhai", "broken");
    assert!(Project::load(&dir.0, 31, 6).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_cross_species_boundaries() {
    let dir = Directory::new();
    dir.write(
        "common/6/main.mhai",
        "mhf_ai 1; species 6; base native; import \"helper.mhai\" as h;",
    );
    dir.write("common/7/helper.mhai", "fn finish() {}");
    std::os::unix::fs::symlink("../7/helper.mhai", dir.0.join("common/6/helper.mhai")).unwrap();
    assert!(
        Project::load(&dir.0, 31, 6)
            .unwrap_err()
            .to_string()
            .contains("symlink")
    );
}

#[test]
fn shipped_multifile_examples_compile_for_default_and_specific_maps() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/monster-ai");
    for map in [31, 32] {
        let p = Project::load(&root, map, 6).unwrap().unwrap();
        let compiled = p.compile().unwrap();
        assert_eq!(compiled.program.species, 6);
        assert!(p.files.len() >= 4);
        assert!(script(&compiled.program, 0, 0).ends_with(&[7, if map == 31 { 4 } else { 1 }]));
    }
}
