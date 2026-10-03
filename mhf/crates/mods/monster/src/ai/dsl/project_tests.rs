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

fn assert_shared_helper(program: &Program, body: &[u8], entries: &[(usize, u8)]) {
    assert_eq!(script(program, 1, 0), [body, &[0xff, 1]].concat());
    for &(root, ending) in entries {
        assert_eq!(script(program, root, 0), [0x81, 0, 0xff, ending]);
    }
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

fn assert_imported_body(body: &str, bytes: &[u8]) {
    let helper = format!("fn check() {{ {body} }}");
    let compiled = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
         fn main() { h.check(); }
         states { patrol = 1 => h.check; }
         events { awareness => h.check; }",
        &[("maps/31/6/helper.mhai", &helper)],
    )
    .compile()
    .unwrap();
    assert_shared_helper(
        &compiled.program,
        bytes,
        &[(0, 0), (EVENT_SLOTS[3].root_index, EVENT_SLOTS[3].ending)],
    );
    assert_eq!(script(&compiled.program, 0, 1), [0x81, 0, 0xff, 0]);
}

fn rejects(bodies: impl IntoIterator<Item = impl AsRef<str>>) {
    for body in bodies {
        let body = body.as_ref();
        assert!(
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn area_matches_import_calls_and_keep_case_order() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { match self.area { 326 => { h.act(); } 1 => { return; } 326 => {} else => {} } }",
        &[("maps/31/6/helper.mhai", "fn act() { nop(); }")],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(script(&compiled.program, 1, 0), &[0x92, 0xff, 1]);
    let bytes = script(&compiled.program, 0, 0);
    assert!(bytes.starts_with(&[0x15, 0, 3, 0x15, 1, 1, 70, 0x81, 0]));
    assert!(bytes.windows(4).any(|w| w == [0x15, 1, 0, 1]));
}

#[test]
fn target_strategies_encode_in_imported_conditions_and_events() {
    for (name, opcode) in [
        ("SameArea", 0x53),
        ("SameAreaGroundGroup", 0x5f),
        ("SameOrAllowedArea", 0x52),
        ("TrackedPlayer", 0x12),
        ("LeaderTarget", 0x58),
        ("PlayerOrMonster", 0x7e),
    ] {
        assert_imported_body(
            &format!(
                "if self.target_available() {{ self.select_target_entity(EntityTarget::{name}); }}"
            ),
            &[0x54, 0, opcode, 0x54, 2],
        );
    }
    rejects([
        "self.select_target_entity();",
        "self.select_target_entity(256);",
        "self.select_target_entity(Mode::Attack);",
        "self.select_target_entity(EntityTarget::Unknown);",
        "self.select_target_entity(EntityTarget::samearea);",
        "self.select_target_entity(EntityTarget.SameArea);",
        "self.select_target_entity(EntityTarget::SameArea, EntityTarget::LeaderTarget);",
        "if self.select_target_entity(EntityTarget::SameArea) {}",
    ]);
}

#[test]
fn player_slot_selection_encodes_without_implicit_binding_or_refresh() {
    for (literal, slot) in [("0", 0), ("3", 3), ("0x53", 0x53), ("255", 255)] {
        let source =
            format!("mhf_ai 1; species 6; fn main() {{ self.select_target_entity({literal}); }}");
        let compiled = parse(&source).unwrap().compile().unwrap();
        assert_eq!(script(&compiled.program, 0, 0), [6, 1, 0, slot, 0xff, 0]);
    }
    assert_imported_body(
        "if self.target_available() { self.select_target_entity(3); self.resolve_target(); }",
        &[0x54, 0, 6, 1, 0, 3, 0x4d, 0x54, 2],
    );
    rejects([
        "self.select_target_entity(-1);",
        "self.select_target_entity(1.5);",
        "self.select_target_entity(3, 4);",
        "self.select_target_entity(Direction::Forward500);",
        "if self.select_target_entity(3) {}",
    ]);
}

#[test]
fn mode_target_and_random_methods_encode_and_validate() {
    let p = project(
        "mhf_ai 1; species 6; map 31; fn main() { self.set_mode(Mode::Normal); self.set_mode(Mode::Attack); self.resolve_target(); self.increment_random_value(); }",
        &[],
    );
    assert_eq!(
        script(&p.compile().unwrap().program, 0, 0),
        [0x40, 0, 0x40, 1, 0x4d, 0x84, 0xff, 0]
    );
    rejects([
        "self.set_mode();",
        "self.set_mode(1);",
        "self.set_mode(Mode::Other);",
        "self.resolve_target(1);",
        "self.increment_random_value(1);",
        "self.increment_random_value;",
        "if self.resolve_target() {}",
    ]);
}

#[test]
fn native_commands_keep_their_bytes_in_entries_and_imported_conditionals() {
    let cases: &[(&str, &[u8])] = &[
        ("self.replenish_recovery_meter(80);", &[0x4e, 0]),
        ("self.replenish_foraging_meter();", &[0x4f, 0]),
        (
            "self.clear_undetected_player_tracking_timers();",
            &[0x5b, 0],
        ),
        ("self.try_change_area();", &[0x18]),
        (
            "self.bind_awareness_target(); self.bind_current_target();",
            &[0x11, 0x13],
        ),
        (
            "self.select_perception_profile(0); self.select_perception_profile(4); self.select_perception_profile(255); self.bind_scanned_object();",
            &[0x2e, 0, 0x2e, 4, 0x2e, 255, 0x2d],
        ),
        (
            "self.bind_target_ground_point(0); self.bind_target_ground_point(4); self.bind_target_ground_point(255);",
            &[0x49, 0, 0x49, 4, 0x49, 255],
        ),
    ];
    for &(body, bytes) in cases {
        let entry = format!(
            "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
             fn main() {{ {body} h.act(); }}
             states {{ patrol = 1 => {{ {body} h.act(); }} }}
             events {{ awareness => {{ {body} h.act(); }} }}"
        );
        let helper = format!("fn act() {{ if self.target_detected {{ {body} }} }}");
        let compiled = project(&entry, &[("maps/31/6/helper.mhai", &helper)])
            .compile()
            .unwrap();
        assert!(compiled.warnings.is_empty(), "{body}");
        for index in [0, 1] {
            assert_eq!(
                script(&compiled.program, 0, index),
                [bytes, &[0x81, 0, 0xff, 0]].concat(),
                "{body}"
            );
        }
        assert_eq!(
            script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
            [bytes, &[0x81, 0, 0xff, EVENT_SLOTS[3].ending]].concat(),
            "{body}"
        );
        assert_eq!(
            script(&compiled.program, 1, 0),
            [&[0x4a, 0][..], bytes, &[0x4a, 2, 0xff, 1]].concat(),
            "{body}"
        );
    }
}

#[test]
fn recovery_meter_replenishment_requires_the_supported_u8_priority() {
    rejects([
        "self.replenish_recovery_meter();",
        "self.replenish_recovery_meter(0x50, 0x50);",
        "self.replenish_recovery_meter(-1);",
        "self.replenish_recovery_meter(80.5);",
        "self.replenish_recovery_meter(true);",
        "self.replenish_recovery_meter;",
        "if self.replenish_recovery_meter(0x50) {}",
        "context.replenish_recovery_meter(0x50);",
    ]);
    for priority in ["0", "0x4e", "0x40", "0x60", "255"] {
        let error = parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ self.replenish_recovery_meter({priority}); }}"
        ))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("unsupported recovery meter priority"),
            "{error}"
        );
        assert!(error.contains("0x50"), "{error}");
    }
    for priority in ["256", "0x100"] {
        let error = parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ self.replenish_recovery_meter({priority}); }}"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("must be a number from 0 to 255"), "{error}");
        assert!(
            !error.contains("unsupported recovery meter priority"),
            "{error}"
        );
    }
}

#[test]
fn recovery_cooldown_method_name_is_rejected() {
    rejects(["self.extend_recovery_cooldown(80);"]);
}

#[test]
fn argumentless_commands_require_self_statements() {
    for method in [
        "replenish_foraging_meter",
        "clear_undetected_player_tracking_timers",
        "try_change_area",
        "bind_scanned_object",
        "bind_awareness_target",
        "bind_current_target",
    ] {
        rejects(["(0);", "(0, 1);", ";", " = 1;"].map(|suffix| format!("self.{method}{suffix}")));
        rejects([
            format!("context.{method}();"),
            format!("if self.{method}() {{}}"),
        ]);
    }
}

#[test]
fn area_change_initialization_works_in_entries_and_imported_conditionals() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
         fn main() { self.init_area_change(1, 3, 5, 7); h.configure(); }
         states { patrol = 1 => { self.init_area_change(255, 128, 0, 255); h.configure(); } }
         events { awareness => { self.init_area_change(0, 127, 255, 2); h.configure(); } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn configure() {
                if self.target_angle_at_least(45) { self.init_area_change(2, 3, 4, 5); }
                else { self.init_area_change(6, 7, 8, 9); }
                self.try_change_area();
            }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 1, 0),
        [
            0x14, 0, 45, 0x17, 2, 3, 4, 5, 0x14, 1, 0x17, 6, 7, 8, 9, 0x14, 2, 0x18, 0xff, 1,
        ]
    );
    assert_eq!(
        script(&compiled.program, 0, 0),
        [0x17, 1, 3, 5, 7, 0x81, 0, 0xff, 0]
    );
    assert_eq!(
        script(&compiled.program, 0, 1),
        [0x17, 255, 128, 0, 255, 0x81, 0, 0xff, 0]
    );
    let event = &EVENT_SLOTS[3];
    assert_eq!(
        script(&compiled.program, event.root_index, 0),
        [0x17, 0, 127, 255, 2, 0x81, 0, 0xff, event.ending]
    );
}

#[test]
fn area_change_initialization_requires_four_u8_arguments_and_statement_context() {
    for args in ["", "0", "0, 2", "0, 2, 1", "0, 2, 1, 0, 0", "0 2, 1, 0"] {
        let source = format!("mhf_ai 1; species 6; fn main() {{ self.init_area_change({args}); }}");
        assert!(parse(&source).is_err(), "{source}");
    }
    for index in 0..4 {
        for invalid in ["-1", "256", "1.5", "true"] {
            let mut args = ["0", "2", "1", "0"];
            args[index] = invalid;
            let source = format!(
                "mhf_ai 1; species 6; fn main() {{ self.init_area_change({}); }}",
                args.join(", ")
            );
            assert!(parse(&source).is_err(), "{source}");
        }
    }
    rejects([
        "if self.init_area_change(0, 2, 1, 0) {}",
        "self.init_area_change;",
        "self.init_area_change = 1;",
        "context.init_area_change(0, 2, 1, 0);",
    ]);
}

#[test]
fn target_position_available_is_a_side_effecting_condition() {
    assert_imported_body(
        "if self.target_position_available() { nop(); } else { nop(); } if self.target_position_available() { nop(); }",
        &[
            0x5d, 0, 0x92, 0x5d, 1, 0x92, 0x5d, 2, 0x5d, 0, 0x92, 0x5d, 2,
        ],
    );
    rejects([
        "if self.target_position_available {}",
        "if self.target_position_available(1) {}",
        "self.target_position_available();",
        "if self.target.position_available() {}",
    ]);
}

#[test]
fn perception_profile_selection_rejects_invalid_arguments() {
    rejects([
        "self.select_perception_profile();",
        "self.select_perception_profile(256);",
        "if self.select_perception_profile(1) {}",
        "self.select_perception_profile = 1;",
    ]);
}

#[test]
fn target_area_binding_encodes_in_entries_and_nested_imports() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
         fn main() { self.bind_target_area(0); h.bind(); }
         states { patrol = 1 => { self.bind_target_area(300); h.bind(); } }
         events { awareness => { self.bind_target_area(65535); h.bind(); } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn bind() {
                self.select_target_area(300);
                self.bind_target_area(255);
                self.select_target_point(PointTarget::Departure, 255);
                if self.target_detected {
                    self.bind_target_area(0x100);
                } else {
                    self.select_target_area(AreaTarget::TargetPlayer);
                    self.bind_target_area(0xffff);
                }
                self.resolve_target();
            }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert!(compiled.warnings.is_empty());
    assert_eq!(
        script(&compiled.program, 0, 0),
        [0x1a, 0, 0, 0x81, 0, 0xff, 0]
    );
    assert_eq!(
        script(&compiled.program, 0, 1),
        [0x1a, 1, 44, 0x81, 0, 0xff, 0]
    );
    let event = &EVENT_SLOTS[3];
    assert_eq!(
        script(&compiled.program, event.root_index, 0),
        [0x1a, 255, 255, 0x81, 0, 0xff, event.ending]
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [
            0x06, 3, 0, 1, 44, 0x1a, 0, 255, 0x06, 2, 4, 255, 0x4a, 0, 0x1a, 1, 0, 0x4a, 1, 0x06,
            10, 0, 0, 0x1a, 255, 255, 0x4a, 2, 0x4d, 0xff, 1,
        ]
    );
}

#[test]
fn target_area_binding_requires_one_u16_argument_and_statement_context() {
    rejects([
        "self.bind_target_area();",
        "self.bind_target_area(0, 1);",
        "self.bind_target_area(-1);",
        "self.bind_target_area(65536);",
        "self.bind_target_area(0x10000);",
        "self.bind_target_area(1.5);",
        "self.bind_target_area(true);",
        "self.bind_target_area(\"300\");",
        "self.bind_target_area(AreaTarget::TargetPlayer);",
        "self.bind_target_area;",
        "self.bind_target_area = 300;",
        "context.bind_target_area(300);",
        "if self.bind_target_area(300) {}",
        "self.set_destination_area(300);",
        "self.set_next_area(300);",
    ]);
}

#[test]
fn target_ground_point_binding_requires_one_u8_argument_and_statement_context() {
    rejects([
        "self.bind_target_ground_point();",
        "self.bind_target_ground_point(0, 1);",
        "self.bind_target_ground_point(-1);",
        "self.bind_target_ground_point(256);",
        "self.bind_target_ground_point(1.5);",
        "if self.bind_target_ground_point(0) {}",
        "self.bind_target_ground_point;",
        "self.bind_target_ground_point = 1;",
    ]);
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
    assert_eq!(
        script(&compiled.program, 1, 200),
        [0x82, 0, 0, 0x92, 0xff, 1]
    );
    // Native returns preserve subsequent bytes, just as event returns do.
    assert_eq!(
        script(&compiled.program, 15, 0),
        [0x48, 2, 0xff, 2, 0x48, 9, 0xff, 2]
    );
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
fn automatic_functions_have_distinct_scripts_and_native_return_slots() {
    let compiled = parse(
        "mhf_ai 1; species 6;
        fn main() { first(); first(); }
        @slot(table = 1, index = 0) fn reserved() {}
        fn first() { second(); wait(1); }
        fn second() { third(); wait(2); }
        fn third() { fourth(); wait(3); }
        fn fourth() { third(); wait(4); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x81, 1, 0x81, 1, 0xff, 0]);
    assert_eq!(script(&compiled.program, 1, 0), [0xff, 1]);
    assert_eq!(
        script(&compiled.program, 1, 1),
        [0x82, 0, 0, 0x48, 1, 0xff, 1]
    );
    assert_eq!(
        script(&compiled.program, 15, 0),
        [0x16, 0, 0x48, 2, 0xff, 2]
    );
    assert_eq!(script(&compiled.program, 9, 0), [0x16, 1, 0x48, 3, 0xff, 3]);
    assert_eq!(script(&compiled.program, 9, 1), [0x16, 0, 0x48, 4, 0xff, 3]);
    assert_eq!(compiled.program.automatic_slots.len(), 4);
    assert_eq!(compiled.program.relocations.len(), 6);
}

#[test]
fn recursive_calls_compile_without_a_synthetic_call_stack() {
    let compiled = parse(
        "mhf_ai 1; species 6;
        fn main() { again(); }
        fn again() { again(); wait(99); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    // A same-stage 81 is a tail transfer, not a new stack frame.
    assert_eq!(script(&compiled.program, 1, 0), [0x81, 0]);
    let compiled = parse(
        "mhf_ai 1; species 6;
        fn main() { first(); }
        fn first() { second(); }
        fn second() { first(); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(script(&compiled.program, 1, 0), [0x82, 0, 0, 0xff, 1]);
    assert_eq!(script(&compiled.program, 15, 0), [0x81, 0, 0xff, 2]);
}

#[test]
fn table_nine_calls_preserve_their_independent_return_cursor() {
    let compiled = parse(
        "mhf_ai 1; species 6;
        fn main() { special(); primary(); secondary(); }
        states { another => special; }
        events { awareness => special; }
        @slot(table = 9, index = 200) fn special() { nested(); wait(1); }
        @slot(table = 9, index = 201) fn nested() { if self.flashed { return; } nop(); }
        @slot(table = 1, index = 3) fn primary() { special(); wait(2); }
        @slot(table = 15, index = 7) fn secondary() { special(); wait(3); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [0x16, 200, 0x81, 3, 0x82, 0, 7, 0xff, 0]
    );
    assert_eq!(script(&compiled.program, 0, 1), [0x16, 200, 0xff, 0]);
    assert_eq!(script(&compiled.program, 3, 0), [0x16, 200, 0xff, 0xfd]);
    assert_eq!(
        script(&compiled.program, 9, 200),
        [0x16, 201, 0x48, 1, 0xff, 3]
    );
    assert_eq!(
        script(&compiled.program, 9, 201),
        [0x39, 0, 0xff, 3, 0x39, 2, 0x92, 0xff, 3]
    );
    assert_eq!(
        script(&compiled.program, 1, 3),
        [0x16, 200, 0x48, 2, 0xff, 1]
    );
    assert_eq!(
        script(&compiled.program, 15, 7),
        [0x16, 200, 0x48, 3, 0xff, 2]
    );
    assert!(compiled.program.automatic_slots.is_empty());
}

#[test]
fn native_function_returns_keep_raw_condition_closing_markers() {
    for (annotation, table, ending) in [("", 1, 1), ("@slot(table = 9, index = 0)", 9, 3)] {
        let compiled = parse(&format!(
            "mhf_ai 1; species 6;
            fn main() {{ helper(); }}
            {annotation} fn helper() {{ native(0x35, 0); return; native(0x35, 2); nop(); }}"
        ))
        .unwrap()
        .compile()
        .unwrap();
        assert_eq!(
            script(&compiled.program, table, 0),
            [0x35, 0, 0xff, ending, 0x35, 2, 0x92, 0xff, ending]
        );
    }
}

#[test]
fn unused_functions_are_allocated_and_checked_without_inlining() {
    let compiled = parse("mhf_ai 1; species 6; fn main() {} fn unused() { nop(); }")
        .unwrap()
        .compile()
        .unwrap();
    assert_eq!(script(&compiled.program, 1, 0), [0x92, 0xff, 1]);
    assert!(
        parse("mhf_ai 1; species 6; fn main() {} fn unused() { missing(); }")
            .unwrap()
            .compile()
            .is_err()
    );
    let mut program = compiled.program;
    program.automatic_slots.push(program.automatic_slots[0]);
    assert!(
        program
            .validate_lossless()
            .unwrap_err()
            .to_string()
            .contains("duplicate automatic")
    );
}

#[test]
fn relocation_sites_must_be_generated_calls_at_instruction_boundaries() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { wait(129); helper(); } fn helper() {}")
        .unwrap()
        .compile()
        .unwrap();
    let mut program = compiled.program.clone();
    program.relocations[0].offset = 1;
    assert!(program.validate_lossless().is_err());
    let mut program = compiled.program;
    program.relocations.push(program.relocations[0]);
    assert!(program.validate_lossless().is_err());
}

#[test]
fn mode_is_encodes_enum_arguments_and_rejects_invalid_calls() {
    for (name, value) in [("Normal", 0), ("Attack", 1)] {
        let compiled = parse(&format!("mhf_ai 1; species 6; fn main() {{ if self.mode_is(Mode::{name}) {{ nop(); }} else {{ end; }} }}"))
            .unwrap().compile().unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [0x0b, 0, value, 0x92, 0x0b, 1, 0xff, 0, 0x0b, 2, 0xff, 0]
        );
    }
    rejects([
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
    ]);
    assert_imported_body(
        "if self.mode_is(Mode::Attack) { nop(); }",
        &[0x0b, 0, 1, 0x92, 0x0b, 2],
    );
}

#[test]
fn query_conditions_require_empty_parentheses_and_keep_native_encoding() {
    for (name, opcode) in [
        ("self.has_player_in_same_area", 0x28),
        ("context.any_player_carrying", 0x2f),
        ("self.target_available", 0x54),
    ] {
        let compiled = parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ if {name}() {{ nop(); }} else {{ wait(1); }} }}"
        ))
        .unwrap()
        .compile()
        .unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [opcode, 0, 0x92, opcode, 1, 0x48, 1, opcode, 2, 0xff, 0],
            "{name}"
        );
        rejects([
            format!("if {name} {{}}"),
            format!("if {name}(1) {{}}"),
            format!("{name}();"),
        ]);
    }
}

#[test]
fn has_player_in_same_area_is_a_method_condition_with_optional_else() {
    assert_imported_body(
        "if self.has_player_in_same_area() { if self.airborne { nop(); } } else { end; } if self.has_player_in_same_area() {}",
        &[
            0x28, 0, 9, 0, 0x92, 9, 2, 0x28, 1, 0xff, 0, 0x28, 2, 0x28, 0, 0x28, 2,
        ],
    );
    rejects([
        "if self.has_player_in_area() {}",
        "if context.has_player_in_same_area() {}",
        "if context.has_player_in_same_area {}",
        "self.has_player_in_same_area;",
    ]);
}

#[test]
fn target_detected_is_a_property_condition_with_optional_else() {
    assert_imported_body(
        "if self.target_detected { if self.airborne { nop(); } } else { end; } if self.target_detected {}",
        &[
            0x4a, 0, 9, 0, 0x92, 9, 2, 0x4a, 1, 0xff, 0, 0x4a, 2, 0x4a, 0, 0x4a, 2,
        ],
    );
    rejects([
        "if context.target_detected {}",
        "if self.target_detected() {}",
        "self.target_detected;",
    ]);
}

#[test]
fn target_ground_condition_nests_in_shared_imported_helpers() {
    assert_imported_body(
        "if self.target_ground_is(0) {
            if self.target_ground_is(255) { nop(); }
            else { if self.enraged { wait(1); } }
        } else { wait(2); }",
        &[
            0x5a, 0, 0, 0x5a, 0, 255, 0x92, 0x5a, 1, 0x35, 0, 0x48, 1, 0x35, 2, 0x5a, 2, 0x5a, 1,
            0x48, 2, 0x5a, 2,
        ],
    );
}

#[test]
fn scalar_conditions_keep_entry_event_and_helper_return_semantics() {
    for (method, opcode, state_value, helper_value) in [
        ("target_ground_is", 0x5a, 255, 7),
        ("target_angle_at_least", 0x14, 181, 45),
    ] {
        let entry = format!(
            "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
             fn main() {{ if self.{method}(0) {{ return; }} h.check(); }}
             states {{ patrol = 1 => {{ if self.{method}({state_value}) {{ return; }} h.check(); }} }}
             events {{ awareness => {{ if self.{method}(0xff) {{ return; }} h.check(); }} }}"
        );
        let helper =
            format!("fn check() {{ if self.{method}({helper_value}) {{ return; }} nop(); }}");
        let compiled = project(&entry, &[("maps/31/6/helper.mhai", &helper)])
            .compile()
            .unwrap();
        for (index, value) in [(0, 0), (1, state_value)] {
            assert_eq!(
                script(&compiled.program, 0, index),
                [opcode, 0, value, opcode, 1, 0x81, 0, opcode, 2, 0xff, 0],
                "{method}"
            );
        }
        let event = &EVENT_SLOTS[3];
        assert_eq!(
            script(&compiled.program, event.root_index, 0),
            [
                opcode,
                0,
                255,
                0xff,
                event.ending,
                opcode,
                2,
                0x81,
                0,
                0xff,
                event.ending
            ],
            "{method}"
        );
        assert_eq!(
            script(&compiled.program, 1, 0),
            [opcode, 0, helper_value, 0xff, 1, opcode, 2, 0x92, 0xff, 1],
            "{method}"
        );
    }
}

#[test]
fn scalar_conditions_require_one_u8_argument_and_condition_context() {
    for method in ["target_ground_is", "target_angle_at_least"] {
        rejects(
            ["", "0, 1", "-1", "256", "1.5"].map(|args| format!("if self.{method}({args}) {{}}")),
        );
        rejects([
            format!("if context.{method}(45) {{}}"),
            format!("self.{method}(0);"),
            format!("if self.{method} {{}}"),
            format!("self.{method};"),
            format!("self.{method} = 1;"),
        ]);
    }
}

#[test]
fn pending_area_check_nests_in_shared_imported_helpers() {
    assert_imported_body(
        "if self.check_pending_area() {
            if self.target_angle_at_least(45) { nop(); }
        } else {
            if self.check_pending_area() { wait(1); }
        }",
        &[
            0x03, 0, 0x14, 0, 45, 0x92, 0x14, 2, 0x03, 1, 0x03, 0, 0x48, 1, 0x03, 2, 0x03, 2,
        ],
    );
}

#[test]
fn pending_area_check_requires_an_argumentless_condition_method() {
    rejects([
        "if self.check_pending_area {}",
        "if self.check_pending_area(1) {}",
        "if self.check_pending_area(0, 1) {}",
        "if context.check_pending_area() {}",
        "if self.target.check_pending_area() {}",
        "self.check_pending_area();",
        "self.check_pending_area;",
        "self.check_pending_area = 1;",
    ]);
}

#[test]
fn tracked_players_check_is_a_condition_method_with_native_side_effects() {
    assert_imported_body(
        "if self.check_tracked_players() { if self.target_available() { nop(); } } else { end; } if self.check_tracked_players() {}",
        &[
            2, 0, 0x54, 0, 0x92, 0x54, 2, 2, 1, 0xff, 0, 2, 2, 2, 0, 2, 2,
        ],
    );
    rejects([
        "if self.check_tracked_players {}",
        "if self.check_tracked_players(1) {}",
        "self.check_tracked_players();",
        "if self.target.check_tracked_players() {}",
        "if self.flashed() {}",
    ]);
}

#[test]
fn target_available_compiles_in_imported_helpers_and_events() {
    assert_imported_body(
        "if self.target_available() { if self.enraged { nop(); } } else { end; }",
        &[0x54, 0, 0x35, 0, 0x92, 0x35, 2, 0x54, 1, 0xff, 0, 0x54, 2],
    );
    rejects([
        "if self.target {}",
        "if self.target.unknown {}",
        "if self.target.available {}",
        "if self.target.available() {}",
        "self.target.available = 1;",
        "if self.target.available.extra {}",
    ]);
}

#[test]
fn distance_match_compiles_imports_and_native_fallback_without_changing_thresholds() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { match self.target_distance_group() { 1 => h.attack(); 2 => { self.resolve_target(); } else => end forget_target; } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn attack() { random { 1 => nop(); } }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x83, 0, 2, 0x83, 1, 0x81, 0, 0x83, 2, 0x4d, 0x83, 3, 0xff, 0xf7, 0x83, 0xff, 0xff, 0
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
    rejects([
        "self.target_distance_group();",
        "if self.target_distance_group() {}",
        "match self.target_distance_group(1) { 1 => {} else => {} }",
    ]);
}

#[test]
fn waypoint_selection_preserves_explicit_refresh_and_checks_index_width() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { self.select_target_point(0); self.resolve_target(); self.action(0:1, 0); self.select_target_point(255); }").unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [6, 2, 1, 0, 0x4d, 5, 0, 1, 0, 6, 2, 1, 255, 0xff, 0]
    );
    rejects([
        "self.select_target_point();",
        "self.select_target_point(256);",
        "self.select_target_point(-1);",
        "self.select_target_point(1.5);",
        "self.select_target_point(0, 1);",
        "if self.select_target_point(0) {}",
    ]);
}

#[test]
fn landing_and_departure_points_encode_in_entries_and_nested_imports() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
         fn main() {
             self.select_target_point(PointTarget::Landing, 0);
             self.select_target_point(PointTarget::Departure, 0);
             h.choose();
         }
         states { patrol = 1 => {
             self.select_target_point(PointTarget::Landing, 255);
             self.select_target_point(PointTarget::Departure, 255);
             h.choose();
         } }
         events { awareness => {
             self.select_target_point(PointTarget::Landing, 0x7f);
             self.select_target_point(PointTarget::Departure, 0x7f);
             h.choose();
         } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn choose() {
                self.select_target_point(PointTarget::Landing, 0xff);
                self.select_target_point(0);
                self.select_target_point(PointTarget::Default);
                self.select_target_point(Direction::Forward500);
                if self.target_detected {
                    self.select_target_point(PointTarget::Departure, 0xff);
                }
            }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert!(compiled.warnings.is_empty());
    assert_eq!(
        script(&compiled.program, 0, 0),
        [0x06, 2, 3, 0, 0x06, 2, 4, 0, 0x81, 0, 0xff, 0]
    );
    assert_eq!(
        script(&compiled.program, 0, 1),
        [0x06, 2, 3, 255, 0x06, 2, 4, 255, 0x81, 0, 0xff, 0]
    );
    let event = &EVENT_SLOTS[3];
    assert_eq!(
        script(&compiled.program, event.root_index, 0),
        [
            0x06,
            2,
            3,
            127,
            0x06,
            2,
            4,
            127,
            0x81,
            0,
            0xff,
            event.ending
        ]
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [
            0x06, 2, 3, 255, 0x06, 2, 1, 0, 0x06, 2, 0, 0, 0x06, 6, 0, 0, 0x4a, 0, 0x06, 2, 4, 255,
            0x4a, 2, 0xff, 1,
        ]
    );
}

#[test]
fn landing_and_departure_points_require_one_u8_index_after_the_point_kind() {
    for kind in ["Landing", "Departure"] {
        for arguments in ["", ",", ", 256", ", -1", ", 1.5", ", true", ", 0, 1"] {
            let source = format!(
                "mhf_ai 1; species 6; fn main() {{ self.select_target_point(PointTarget::{kind}{arguments}); }}"
            );
            assert!(parse(&source).is_err(), "{source}");
        }
        let source = format!(
            "mhf_ai 1; species 6; fn main() {{ if self.select_target_point(PointTarget::{kind}, 0) {{}} }}"
        );
        assert!(parse(&source).is_err(), "{source}");
    }
    rejects([
        "self.select_target_point(PointTarget::Unknown, 0);",
        "self.select_target_point(PointTarget::Default, 0);",
    ]);
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
    rejects([
        "self.select_target_point(Direction::Unknown);",
        "self.select_target_point(Direction::Forward);",
        "self.select_target_point(Direction::Forward750);",
        "self.select_target_point(Direction::forward);",
        "self.select_target_point(Direction.Forward500);",
        "self.select_target_point(Direction::Forward500, 1000);",
        "self.select_target_point(EntityTarget::SameArea);",
        "if self.select_target_point(Direction::Forward500) {}",
        "self.select_waypoint(0);",
        "self.select_target(EntityTarget::SameArea);",
        "self.refresh_target();",
    ]);
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
    let compiled = parse("mhf_ai 1; species 6; fn main() { if self.active { if self.active { nop(); } else { self.resolve_target(); } } else { end; } }")
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
    let compiled = parse("mhf_ai 1; species 6; events { rage_entered => react; } fn main() { if self.enraged { if self.flashed { end; } else { nop(); } } else { restart; } } fn react() { if self.enraged { nop(); } }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x35, 0, 0x39, 0, 0xff, 0, 0x39, 1, 0x92, 0x39, 2, 0x35, 1, 4, 0x35, 2, 0xff, 0,
        ]
    );
    assert_shared_helper(
        &compiled.program,
        &[0x35, 0, 0x92, 0x35, 2],
        &[(EVENT_SLOTS[4].root_index, EVENT_SLOTS[4].ending)],
    );
    for body in ["self.enraged = 1;", "if self.enraged() {}"] {
        assert!(parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err());
    }
}

#[test]
fn area_timer_expired_encodes_conditions_and_slot_returns() {
    let compiled = parse(
        "mhf_ai 1; species 6;
         fn main() { if self.area_timer_expired { if self.flashed { nop(); } } else { wait(1); } }
         states { idle => { if self.area_timer_expired { nop(); } } }
         events { awareness => { if self.area_timer_expired { nop(); } } }
         @slot(table = 9, index = 0) fn helper() { if self.area_timer_expired { return; } wait(2); }",
    ).unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x29, 0, 0x39, 0, 0x92, 0x39, 2, 0x29, 1, 0x48, 1, 0x29, 2, 0xff, 0,
        ]
    );
    assert_eq!(
        script(&compiled.program, 0, 1),
        [0x29, 0, 0x92, 0x29, 2, 0xff, 0]
    );
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0x29, 0, 0x92, 0x29, 2, 0xff, EVENT_SLOTS[3].ending,]
    );
    assert_eq!(
        script(&compiled.program, 9, 0),
        [0x29, 0, 0xff, 3, 0x29, 2, 0x48, 2, 0xff, 3,]
    );
    rejects([
        "if self.area_timer_expired() {}",
        "self.area_timer_expired;",
        "self.area_timer_expired = 1;",
    ]);
}

#[test]
fn context_query_encodes_callback_branch() {
    let source =
        "mhf_ai 1; species 11; fn main() { match context.query(4) { 1 => nop(); else => end; } }";
    let compiled = parse(source).unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x79, 0, 1, 4, 0x79, 1, 1, 0x92, 0x79, 2, 0xff, 0, 0x79, 3, 0xff, 0,
        ]
    );
    for body in ["self.zenith = 1;", "if self.zenith() {}"] {
        assert!(parse(&format!("mhf_ai 1; species 11; fn main() {{ {body} }}")).is_err());
    }
}

#[test]
fn ordered_matches_preserve_opcodes_optional_else_and_reject_invalid_cases() {
    for (selector, wrong_namespace, opcode, first) in [
        ("self.request", "context.request", 0x1d, 1),
        ("context.debug_mode", "self.debug_mode", 0x94, 0),
        ("self.species", "context.species", 0x70, 0),
    ] {
        for fallback in ["", "else => nop();"] {
            let compiled = parse(&format!(
                "mhf_ai 1; species 1; fn main() {{ match {selector} {{ {first} => wait(3); 255 => {{}} {fallback} }} }}"
            ))
            .unwrap()
            .compile()
            .unwrap();
            let mut expected = vec![opcode, 0, 2, opcode, 1, first, 0x48, 3, opcode, 1, 255];
            if !fallback.is_empty() {
                expected.extend([opcode, 2, 0x92]);
            }
            expected.extend([opcode, 3, 0xff, 0]);
            assert_eq!(script(&compiled.program, 0, 0), expected, "{selector}");
        }
        for body in [
            format!("match {wrong_namespace} {{ {first} => {{}} }}"),
            format!("match {selector}() {{ {first} => {{}} }}"),
            format!("match {selector} {{}}"),
            format!("match {selector} {{ else => {{}} }}"),
            format!("match {selector} {{ 256 => {{}} }}"),
            format!(
                "match {selector} {{ {} => {{}} {first} => {{}} }}",
                first + 1
            ),
            format!("match {selector} {{ {first} => {{}} {first} => {{}} }}"),
            format!("if {selector} {{}}"),
            format!("{selector} = 1;"),
        ] {
            assert!(
                parse(&format!("mhf_ai 1; species 1; fn main() {{ {body} }}"))
                    .and_then(|project| project.compile())
                    .is_err(),
                "{body}"
            );
        }
    }
}

#[test]
fn ordered_matches_rewrite_imported_calls_and_bound_case_count() {
    for (selector, opcode) in [("context.debug_mode", 0x94), ("self.species", 0x70)] {
        let compiled = project(
        &format!("mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() {{ match {selector} {{ 0 => h.choose(); else => h.choose(); }} }}"),
        &[("maps/31/6/helper.mhai", "fn choose() { wait(3); }")],
    ).compile().unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [
                opcode, 0, 1, opcode, 1, 0, 0x81, 0, opcode, 2, 0x81, 0, opcode, 3, 0xff, 0
            ]
        );
        assert_eq!(script(&compiled.program, 1, 0), [0x48, 3, 0xff, 1]);
        for count in [255, 256] {
            let cases = (0..count)
                .map(|value| format!("{value} => {{}} "))
                .collect::<String>();
            let result = parse(&format!(
                "mhf_ai 1; species 1; fn main() {{ match {selector} {{ {cases} }} }}"
            ))
            .and_then(|project| project.compile());
            assert_eq!(result.is_ok(), count == 255);
        }
    }
}

#[test]
fn species_group_preserves_source_order_and_optional_else() {
    let compiled = parse(
        "mhf_ai 1; species 1; fn main() { match self.species_group { 42 => nop(); 1 => { wait(3); } else => end; } }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x2c, 0, 2, 0x2c, 1, 42, 0x92, 0x2c, 1, 1, 0x48, 3, 0x2c, 2, 0xff, 0, 0x2c, 3, 0xff, 0,
        ]
    );

    let compiled = parse(
        "mhf_ai 1; species 1; fn main() { match self.species_group { 42 => nop(); 1 => {} 42 => wait(2); } }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x2c, 0, 3, 0x2c, 1, 42, 0x92, 0x2c, 1, 1, 0x2c, 1, 42, 0x48, 2, 0x2c, 3, 0xff, 0,
        ]
    );

    for body in [
        "match self.species_group() { 1 => {} }",
        "match self.species_group {}",
        "match self.species_group { else => {} }",
        "match self.species_group { 256 => {} }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 1; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn flashed_conditions_encode_nested_branches_and_keep_entry_tails() {
    let compiled = parse("mhf_ai 1; species 6; events { awareness => react; } fn main() { if self.flashed { if self.flashed { end; } else { restart; } } else { nop(); } wait(3); } fn react() { if self.flashed { end; } }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x39, 0, 0x39, 0, 0xff, 0, 0x39, 1, 4, 0x39, 2, 0x39, 1, 0x92, 0x39, 2, 0x48, 3, 0xff,
            0,
        ]
    );
    assert_shared_helper(
        &compiled.program,
        &[0x39, 0, 0xff, 0, 0x39, 2],
        &[(EVENT_SLOTS[3].root_index, EVENT_SLOTS[3].ending)],
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
        [0x39, 0, 0x81, 0, 0x39, 1, 5, 3, 6, 2, 0x39, 2, 0xff, 0,]
    );
}

#[test]
fn entry_returns_restructure_but_function_returns_use_their_slot() {
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
            "if self.check_pending_area() { return; } wait(1);",
            "if self.check_pending_area() {} else { wait(1); }",
        ),
        (
            "if self.check_pending_area() { return; } else { return; } wait(9);",
            "if self.check_pending_area() {} else {}",
        ),
        (
            "match self.target_angle() { 90 => return; } wait(1);",
            "match self.target_angle() { 90 => {} else => wait(1); }",
        ),
        (
            "match self.target_angle() { 90 => wait(1); else => return; } wait(2);",
            "match self.target_angle() { 90 => { wait(1); wait(2); } else => {} }",
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
            "if self.flashed { end; } else { return; } wait(2);",
            "if self.flashed { end; wait(2); } else {}",
        ),
    ] {
        let entry = |body| {
            parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}"))
                .unwrap()
                .compile()
                .unwrap()
                .program
        };
        assert_eq!(entry(body), entry(expected));
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
            let explicit = parse(&format!("mhf_ai 1; species 6; base native; {entry} @slot(table = 1, index = 0) fn helper() {{ {body} }}"))
                .unwrap().compile().unwrap();
            assert_eq!(
                actual.program.nodes, explicit.program.nodes,
                "{entry}: {body}"
            );
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
    assert_eq!(script(&compiled.program, 3, 0), [0x81, 1, 0xff, 0xfd]);
    assert_eq!(
        script(&compiled.program, 1, 1),
        [0x39, 0, 0xff, 1, 0x39, 2, 0x48, 1, 0xff, 1]
    );
    for (root, index, ending) in [(0, 0, 0), (0, 1, 0)] {
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
fn automatically_allocated_helpers_return_to_the_native_caller() {
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
        [0x08, 0, 0x82, 0, 0, 0x48, 3, 0x08, 2, 0xff, 1]
    );
    assert_eq!(
        script(&compiled.program, 15, 0),
        [0x39, 0, 0xff, 2, 0x39, 2, 0x48, 2, 0xff, 2]
    );
}

#[test]
fn raw_native_conditionals_reject_entry_returns_but_allow_function_calls() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { native(0x39, 0); helper(); native(0x39, 2); } fn helper() { return; }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [0x39, 0, 0x81, 0, 0x39, 2, 0xff, 0]
    );
    assert_eq!(script(&compiled.program, 1, 0), [0xff, 1]);
    for body in [
        "native(0x39, 0); return; native(0x39, 2);",
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
    rejects([
        "if self.any_player_carrying() {}",
        "if context.active {}",
        "if context.is_daytime() {}",
        "if self.in_action(256:0) {}",
        "if self.in_action(0:256) {}",
        "if self.in_action(1) {}",
        "if self.near_target_2d(256) {}",
        "if self.near_target_3d(256) {}",
        "if self.in_area(65536) {}",
        "if context.in_area(1) {}",
    ]);
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
fn an_event_call_keeps_its_tail_when_the_callee_resets() {
    let compiled = parse("mhf_ai 1; species 6; base native; events { awareness => handler; } fn handler() { end; wait(9); }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0x81, 0, 0xff, EVENT_SLOTS[3].ending]
    );
    assert_eq!(script(&compiled.program, 1, 0), [0xff, 0]);
    for declaration in ["fn end() {}", "fn restart() {}", "actions { end = [0:1]; }"] {
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
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() {} states { fight = 5 => { h.attack(); } patrol => h.attack; } events { player_detected => { h.attack(); } awareness => { end forget_target; wait(9); } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn attack() { random { 1 => nop(); 1 => { self.resolve_target(); } } }",
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
    let compiled = parse(
        "mhf_ai 1; species 6; base native; events { awareness => { end forget_target; wait(9); } }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0xff, 0xf7]
    );
}

#[test]
fn target_angle_threshold_nests_with_angle_bounds_in_shared_imported_helpers() {
    assert_imported_body(
        "if self.target_angle_at_least(45) {
            if self.target_angle_at_least(180) { nop(); }
            else { if self.target_angle_in(45, 135) { wait(1); } }
        }",
        &[
            0x14, 0, 45, 0x14, 0, 180, 0x92, 0x14, 1, 0x78, 0, 32, 96, 0x48, 1, 0x78, 2, 0x14, 2,
            0x14, 2,
        ],
    );
}

#[test]
fn target_angle_match_quantizes_degree_upper_bounds() {
    for (literal, threshold, warned) in [
        ("0", 0, false),
        ("0.7", 0, true),
        ("0.703125", 1, true),
        ("1.40625", 1, false),
        ("90", 64, false),
        ("270", 192, false),
        ("358.59375", 255, false),
        ("360", 255, true),
    ] {
        let compiled = parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ match self.target_angle() {{ {literal} => nop(); }} }}"
        ))
        .unwrap()
        .compile()
        .unwrap();
        assert_eq!(
            script(&compiled.program, 0, 0),
            [0x20, 0, 1, 0x20, 1, threshold, 0x92, 0x20, 3, 0xff, 0],
            "{literal}"
        );
        assert_eq!(!compiled.warnings.is_empty(), warned, "{literal}");
        if warned {
            let actual = super::condition::Degrees::from_native(threshold).value();
            assert!(
                compiled.warnings[0]
                    .message
                    .contains(&format!("to {actual} degrees"))
            );
        }
    }
}

#[test]
fn target_angle_match_preserves_source_order_and_duplicate_thresholds() {
    let compiled = parse(
        "mhf_ai 1; species 6; fn main() {
            match self.target_angle() {
                180 => nop();
                90 => wait(1);
                90.1 => wait(2);
                180 => {}
                else => {}
            }
        }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x20, 0, 4, 0x20, 1, 128, 0x92, 0x20, 1, 64, 0x48, 1, 0x20, 1, 64, 0x48, 2, 0x20, 1,
            128, 0x20, 2, 0x20, 3, 0xff, 0,
        ]
    );
}

#[test]
fn target_angle_match_resolves_imports_in_each_entry_and_helper_branch() {
    let body = "match self.target_angle() { 90 => h.work(); else => h.hit(1); }";
    let p = project(
        &format!(
            "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
             fn main() {{ {body} }}
             states {{ patrol = 1 => {{ {body} }} }}
             events {{ awareness => {{ {body} }} }}"
        ),
        &[(
            "maps/31/6/helper.mhai",
            "actions { hit = [3:6]; }
             fn work() {
                 match self.target_angle() { 180 => return; else => hit(2); }
                 wait(1);
             }",
        )],
    );
    let compiled = p.compile().unwrap();
    let bytes = [
        0x20, 0, 1, 0x20, 1, 64, 0x81, 0, 0x20, 2, 5, 3, 6, 1, 0x20, 3,
    ];
    for index in [0, 1] {
        assert_eq!(
            script(&compiled.program, 0, index),
            [bytes.as_slice(), &[0xff, 0]].concat()
        );
    }
    let event = &EVENT_SLOTS[3];
    assert_eq!(
        script(&compiled.program, event.root_index, 0),
        [bytes.as_slice(), &[0xff, event.ending]].concat()
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [
            0x20, 0, 1, 0x20, 1, 128, 0xff, 1, 0x20, 2, 5, 3, 6, 2, 0x20, 3, 0x48, 1, 0xff, 1
        ]
    );
}

#[test]
fn target_angle_match_validates_structure_without_requiring_sorted_cases() {
    rejects([
        "match self.target_angle() {}",
        "match self.target_angle() { else => {} }",
        "match self.target_angle() { -1 => {} }",
        "match self.target_angle() { 360.1 => {} }",
        "match self.target_angle() { 90 => {} else => {} 180 => {} }",
        "match self.target_angle() { 90 => {} else => {} else => {} }",
        "match self.target_angle { 90 => {} }",
        "match self.target_angle(1) { 90 => {} }",
        "match context.target_angle { 90 => {} }",
        "match self.target_angle() { <= 90 => {} }",
        "if self.target_angle {}",
        "if self.target_angle() {}",
        "self.target_angle();",
    ]);
    let cases = "90 => {} ".repeat(255);
    assert!(
        parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ match self.target_angle() {{ {cases} }} }}"
        ))
        .unwrap()
        .compile()
        .is_ok()
    );
    assert!(
        parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ match self.target_angle() {{ {cases} 90 => {{}} }} }}"
        ))
        .is_err()
    );
}

#[test]
fn target_angle_match_preserves_handler_and_native_branch_checks() {
    for source in [
        "mhf_ai 1; species 6; fn main() { match self.target_angle() { 90 => h(); } } handler fn h() { pass; }",
        "mhf_ai 1; species 6; fn main() { match self.target_angle() { 90 => {} else => h(); } } handler fn h() { pass; }",
        "mhf_ai 1; species 6; fn main() { handle h() then { match self.target_angle() { 90 => return; } } } handler fn h() { pass; }",
        "mhf_ai 1; species 6; fn main() { handle h() then {} } handler fn h() { helper(); pass; } fn helper() { match self.target_angle() { 90 => { handle g() then {} } } } handler fn g() { pass; }",
        "mhf_ai 1; species 6; fn main() { match self.target_angle() { 90 => native(0x39, 0); } }",
        "mhf_ai 1; species 6; fn main() { native(0x42, 0, 45); match self.target_angle() { 90 => return; } native(0x42, 2); }",
        "mhf_ai 1; species 6; fn main() { match self.request { 1 => { match self.target_angle() { 90 => return; } } } wait(1); }",
        "mhf_ai 1; species 6; fn main() { handle h() then { match self.request { 1 => { match self.target_angle() { 90 => return; } } } } } handler fn h() { pass; }",
    ] {
        assert!(parse(source).unwrap().compile().is_err(), "{source}");
    }
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
            "mhf_ai 1; species 6; base native; events {{ awareness => {{ if self.target_angle_in({min}, {max}) {{ nop(); }} else {{ self.resolve_target(); }} }} }}"
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
    rejects([
        "if self.target_angle_in(90, 45) {}",
        "if self.target_angle_in(-45, 45) {}",
        "if self.target_angle_in(0, 360.1) {}",
        "if self.target_angle_in(0) {}",
        "if self.target_angle_in(0, 1, 2) {}",
        "if self.target_angle_in {}",
        "self.target_angle_in(0, 90);",
        "wait(1.5);",
    ]);
}

#[test]
fn explicit_zero_random_weights_keep_branches_and_do_not_receive_rounding_shares() {
    let compiled = parse("mhf_ai 1; species 6; fn main() { random { 0 => self.resolve_target(); 1 => nop(); 1 => nop(); 1 => nop(); 0 => self.increment_random_value(); } }")
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
fn random_weights_normalize_and_imported_branches_call_helpers() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { random { 1 => h.attack(); 2 => { h.attack(); self.resolve_target(); } 1 => end forget_target; } }",
        &[("maps/31/6/helper.mhai", "fn attack() { nop(); }")],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x80, 0, 3, 0x80, 1, 8, 0x81, 0, 0x80, 2, 16, 0x81, 0, 0x4d, 0x80, 3, 8, 0xff, 0xf7,
            0x80, 0xff, 0xff, 0
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
    let compiled = parse("mhf_ai 1; species 6; base native; events { awareness => handler; } fn handler() { end forget_target; wait(9); }")
        .unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0x81, 0, 0xff, EVENT_SLOTS[3].ending]
    );
    assert_eq!(script(&compiled.program, 1, 0), [0xff, 0xf7]);
    rejects([
        "end forget_target();",
        "reset unknown;",
        "restart forget_target;",
        "forget_target;",
    ]);
}

#[test]
fn main_is_the_unique_entry_and_restart_reenters_it() {
    let compiled = parse("mhf_ai 1; species 6; states { fight => attack; } fn main() { transition fight; } fn attack() { restart; }")
        .unwrap().compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [7, 1]);
    assert_eq!(script(&compiled.program, 0, 1), [0x81, 0, 0xff, 0]);
    assert_eq!(script(&compiled.program, 1, 0), [4]);
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
fn module_calls_allocate_shared_scripts_and_preserve_entry_endings() {
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
                "import \"movement.mhai\" as move; fn attack() { move.prepare(); self.action(3:6, 0); }",
            ),
            (
                "common/6/movement.mhai",
                "fn prepare() { wait(5); return; nop(); }",
            ),
        ],
    );
    let c = p.compile().unwrap();
    assert_eq!(script(&c.program, 0, 0), [0x81, 0, 7, 4]);
    assert_eq!(script(&c.program, 0, 4), [0x81, 1, 0xff, 0]);
    assert_eq!(script(&c.program, 0, 5), [0x81, 2, 0xff, 0]);
    assert_eq!(script(&c.program, 1, 0), [0x82, 0, 0, 5, 3, 6, 0, 0xff, 1]);
    assert_eq!(script(&c.program, 1, 1), [7, 5]);
    assert_eq!(script(&c.program, 1, 2), [4]);
    assert_eq!(script(&c.program, 15, 0), [0x48, 5, 0xff, 2, 0x92, 0xff, 2]);
    for event in &EVENT_SLOTS[..2] {
        assert_eq!(
            script(&c.program, event.root_index, 0),
            [0x81, 0, 0xff, event.ending]
        );
    }
}

#[test]
fn named_events_map_independently_of_order_and_get_their_verified_endings() {
    let text = "mhf_ai 1; species 6; base native;
        events { bait_detected => finish_script; group_signal => finish_script; rage_entered => finish_script;
                 awareness => finish_script; player_detected => finish_script; invalid_ground => finish_script; dung_reaction => finish_script; }
        fn finish_script() { nop(); }";
    let compiled = parse(text).unwrap().compile().unwrap();
    for (slot, ending) in [0xf5, 0xf6, 0xfc, 0xfd, 0xf8, 0xf9, 0xfa]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            script(&compiled.program, EVENT_SLOTS[slot].root_index, 0),
            [0x81, 0, 0xff, ending]
        );
        assert_eq!(script(&compiled.program, 1, 0), [0x92, 0xff, 1]);
    }
    let doc =
        parse("mhf_ai 1; species 6; events { group_signal => finish_script; dung_reaction => finish_script; rage_entered => finish_script; } fn finish_script() {}")
            .unwrap();
    assert_eq!(
        doc.events.iter().map(|e| e.slot).collect::<Vec<_>>(),
        [5, 0, 4]
    );
}

#[test]
fn rejects_invalid_calls_and_bindings() {
    for body in [
        "states { a => absent; } fn finish_script() {}",
        "states { a => finish_script; } fn finish_script() { other(1); transition a; } fn other() {}",
        "events { bait_detected = 6 => finish_script; } fn finish_script() {}",
        "events { 0 => finish_script; } fn finish_script() {}",
        "events { event_0 => finish_script; } fn finish_script() {}",
        "events { kehai => finish_script; } fn finish_script() {}",
        "events { awareness => finish_script; } fn finish_script() { kehai_end(); }",
        "events { invalid_ground => finish_script; } fn finish_script() { no_floor_end(); }",
        "events { player_detected => finish_script; } fn finish_script() { find_end(); }",
        "events { awareness => finish_script; awareness => finish_script; } fn finish_script() {}",
        "events { awareness => finish_script; } fn finish_script() {} fn finish_script() {}",
    ] {
        let text = format!("mhf_ai 1; species 6; base native; {body}");
        assert!(parse(&text).and_then(|d| d.compile()).is_err(), "{text}");
    }
}

#[test]
fn state_functions_reset_on_fallthrough_without_changing_explicit_endings() {
    for (body, expected) in [
        ("", vec![0xff, 0x00]),
        ("self.action(0:1, 0);", vec![0x05, 0, 1, 0, 0xff, 0x00]),
        ("return; wait(9);", vec![0xff, 0x00]),
        ("helper(); wait(2);", vec![0x81, 0, 0x48, 2, 0xff, 0x00]),
        ("restart; wait(9);", vec![0x04]),
        ("restart;", vec![0x04]),
        ("end; wait(9);", vec![0xff, 0x00]),
        ("end forget_target; wait(9);", vec![0xff, 0xf7]),
        (
            "reset_helper(); wait(9);",
            vec![0x81, 0, 0x48, 9, 0xff, 0x00],
        ),
        ("native(0xff, 0x00);", vec![0xff, 0x00]),
        ("stop();", vec![0x68]),
    ] {
        let source = format!(
            "mhf_ai 1; species 6; base native;
             fn main() {{ {body} }} fn helper() {{ wait(1); return; wait(9); }} fn reset_helper() {{ end; }}"
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
        fn main() { a.finish_script(); b.finish_script(); restart; }",
        &[("common/6/a.mhai", "fn finish_script() { nop(); }")],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x81, 0, 0x81, 0, 4]);
    assert_eq!(script(&compiled.program, 1, 0), [0x92, 0xff, 1]);
    assert_eq!(compiled.program.automatic_slots.len(), 1);
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
    assert_shared_helper(&p.compile().unwrap().program, &[0x48, 5], &[(14, 0xf5)]);
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

#[test]
fn area_route_profile_encodes_and_validates_cases() {
    let source = "mhf_ai 1; species 1; fn main() { match self.area_route_profile { 0 => nop(); 255 => {} else => {} } }";
    let compiled = parse(source).unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x57, 0, 2, 0x57, 1, 0, 0, 0x92, 0x57, 1, 0, 255, 0x57, 2, 0x57, 3, 0xff, 0
        ]
    );
    for body in [
        "match self.area_route_profile() { 0 => {} else => {} }",
        "match self.area_route_profile { else => {} }",
        "match self.area_route_profile { 256 => {} else => {} }",
        "match self.area_route_profile { 1 => {} }",
        "match self.area_route_profile { 1 => {} 1 => {} else => {} }",
        "match self.area_route_profile { 2 => {} 1 => {} else => {} }",
    ] {
        assert!(
            parse(&format!("mhf_ai 1; species 1; fn main() {{ {body} }}")).is_err(),
            "{body}"
        );
    }
    let cases: String = (0..255).map(|i| format!("{i} => {{}} ")).collect();
    let valid = format!(
        "mhf_ai 1; species 1; fn main() {{ match self.area_route_profile {{ {cases} else => {{}} }} }}"
    );
    assert!(parse(&valid).unwrap().compile().is_ok());
    let too_many_cases = format!(
        "mhf_ai 1; species 1; fn main() {{ match self.area_route_profile {{ {cases} 255 => {{}} else => {{}} }} }}"
    );
    assert!(parse(&too_many_cases).is_err());
}

#[test]
fn area_route_profile_supports_imports_and_early_return() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.choose(); }",
        &[(
            "maps/31/6/helper.mhai",
            "fn choose() { match self.area_route_profile { 0 => { return; } else => work(); } nop(); } fn work() { wait(1); }",
        )],
    );
    assert!(p.compile().is_ok());
    let passing = "mhf_ai 1; species 1; fn main() {} states { idle => { match self.area_route_profile { 0 => pass; else => {} } } }";
    assert!(parse(passing).unwrap().compile().is_ok());
}

#[test]
fn context_query_rejects_invalid_selectors_and_cases() {
    rejects([
        "if self.zenith {}",
        "match self.context.query(4) { 1 => {} else => {} }",
        "match context.area_route_profile { 1 => {} else => {} }",
        "context.query(4);",
        "if context.query(4) {}",
        "match context.query() { 1 => {} else => {} }",
        "match context.query(256) { 1 => {} else => {} }",
        "match context.query(4) { 256 => {} else => {} }",
        "match context.query(4) { else => {} }",
        "match context.query(4) { 1 => {} }",
        "match context.query(4) { 1 => {} 1 => {} else => {} }",
        "match context.query(4) { 2 => {} 1 => {} else => {} }",
        "match context.query(4) { 1 => {} else => {} 2 => {} }",
    ]);
    let cases: String = (0..=255).map(|i| format!("{i} => {{}} ")).collect();
    assert!(
        parse(&format!(
            "mhf_ai 1; species 6; fn main() {{ match context.query(0) {{ {cases} else => {{}} }} }}"
        ))
        .is_err()
    );
}

#[test]
fn context_query_supports_byte_boundaries_and_nested_matches() {
    let source = "mhf_ai 1; species 14; fn main() {
        match context.query(255) {
            0 => { match context.query(0) { 255 => nop(); else => {} } }
            255 => nop();
            else => {}
        }
    }";
    let compiled = parse(source).unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x79, 0, 2, 255, 0x79, 1, 0, 0x79, 0, 1, 0, 0x79, 1, 255, 0x92, 0x79, 2, 0x79, 3, 0x79,
            1, 255, 0x92, 0x79, 2, 0x79, 3, 0xff, 0,
        ]
    );
    let cases: String = (0..255).map(|i| format!("{i} => {{}} ")).collect();
    let compiled = parse(&format!(
        "mhf_ai 1; species 6; fn main() {{ match context.query(0) {{ {cases} else => {{}} }} }}"
    ))
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(&script(&compiled.program, 0, 0)[..4], &[0x79, 0, 255, 0]);
}

#[test]
fn context_query_imports_and_returns_preserve_continuations() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h; fn main() { h.choose(); wait(3); } events { awareness => h.choose; }",
        &[(
            "maps/31/6/helper.mhai",
            "fn leaf() { nop(); } fn choose() { match context.query(4) { 0 => return; 1 => leaf(); else => leaf(); } wait(2); }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(script(&compiled.program, 0, 0), [0x81, 0, 0x48, 3, 0xff, 0]);
    assert_eq!(
        script(&compiled.program, EVENT_SLOTS[3].root_index, 0),
        [0x81, 0, 0xff, EVENT_SLOTS[3].ending]
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [
            0x79, 0, 2, 4, 0x79, 1, 0, 0xff, 1, 0x79, 1, 1, 0x82, 0, 0, 0x79, 2, 0x82, 0, 0, 0x79,
            3, 0x48, 2, 0xff, 1,
        ]
    );
    assert_eq!(script(&compiled.program, 15, 0), [0x92, 0xff, 2]);
}

#[test]
fn handle_dispatches_to_a_handler_and_keeps_every_outcome_separate() {
    // `pass;` clears the takeover byte and returns from the handler script;
    // `return;` leaves the byte set. The caller checks it after the call.
    let source = "mhf_ai 1; species 6; fn main() {
        handle dispatch() then { end; }
        wait(1);
    }
    handler fn dispatch() {
        if self.flashed { pass; }
        clear_requests();
    }";
    let compiled = parse(source).unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x1b, 0, 1, // request guard
            0x0c, 4, 1, // default takeover
            0x81, 0, // handler script
            0x2b, 0, 4, 1, 0xff, 0, 0x2b, 2, // then { end; }
            0x1b, 2, // end of the protocol
            0x48, 1, // the statement after the block
            0xff, 0,
        ]
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [0x39, 0, 0x0d, 4, 0xff, 1, 0x39, 2, 0x1e, 0xff, 1]
    );
    let explicit = parse(&source.replace("pass;", "mark_unhandled(); return;"))
        .unwrap()
        .compile()
        .unwrap();
    assert_eq!(compiled.program, explicit.program);
}

#[test]
fn handle_keeps_the_slot_call_level_and_the_handler_tail() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
        fn main() { wait(2); handle h.dispatch() then { nop(); } wait(3); }
        events { awareness => h.on_awareness; }",
        &[(
            "maps/31/6/helper.mhai",
            "fn on_awareness() { wait(4); }
             @slot(table = 15, index = 7)
             handler fn leaf() { if self.enraged { pass; } clear_requests(); }
             handler fn dispatch() { leaf(); }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x48, 2, // wait(2)
            // dispatch owns a primary slot and calls the fixed secondary slot.
            0x1b, 0, 1, 0x0c, 4, 1, 0x81, 0, 0x2b, 0, 4, 1, 0x92, 0x2b, 2, 0x1b,
            2, // then { nop(); }
            0x48, 3, 0xff, 0,
        ]
    );
    assert_eq!(script(&compiled.program, 1, 0), [0x82, 0, 7, 0xff, 1]);
    // leaf is table 15, so a pass inside it returns with that level's ending.
    let Node::Table(root) = &compiled.program.nodes[compiled.program.root] else {
        panic!()
    };
    let Node::Table(table) = &compiled.program.nodes[root.get(15).unwrap()] else {
        panic!()
    };
    let Node::Script(leaf) = &compiled.program.nodes[table.get(7).unwrap()] else {
        panic!()
    };
    assert_eq!(
        leaf,
        &[0x35, 0, 0x0d, 0x04, 0xff, 2, 0x35, 2, 0x1e, 0xff, 2]
    );
}

#[test]
fn sequential_handle_blocks_restart_the_protocol_in_order() {
    let source = "mhf_ai 1; species 6; fn main() {
        handle first() then { nop(); }
        handle second() then { }
        wait(2);
    }
    handler fn first() { if self.enraged { return; } pass; }
    handler fn second() { clear_requests(); return; }";
    let compiled = parse(source).unwrap().compile().unwrap();
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x1b, 0, 1, 0x0c, 4, 1, 0x81, 0, 0x2b, 0, 4, 1, 0x92, 0x2b, 2, 0x1b, 2, 0x1b, 0, 1,
            0x0c, 4, 1, 0x81, 1, 0x2b, 0, 4, 1, 0x2b, 2, 0x1b, 2, 0x48, 2, 0xff, 0,
        ]
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [0x35, 0, 0xff, 1, 0x35, 2, 0x0d, 4, 0xff, 1]
    );
    assert_eq!(script(&compiled.program, 1, 1), [0x1e, 0xff, 1]);
}

#[test]
fn request_handlers_reject_uses_outside_the_protocol() {
    for (source, reason) in [
        (
            "mhf_ai 1; species 6; fn main() { dispatch(); } handler fn dispatch() { nop(); }",
            "a handler needs handle",
        ),
        (
            "mhf_ai 1; species 6; handler fn main() { nop(); }",
            "main is not a handler",
        ),
        (
            "mhf_ai 1; species 6; fn main() { wait(1); } events { awareness => watch; } handler fn watch() { nop(); }",
            "a handler is not an event entry",
        ),
        (
            "mhf_ai 1; species 6; fn main() { handle plain() then {} } fn plain() { nop(); }",
            "the target must be a handler",
        ),
        (
            "mhf_ai 1; species 6; fn main() { handle missing() then {} }",
            "the target must exist",
        ),
        (
            "mhf_ai 1; species 6; fn main() { handle dispatch() then { return; } } handler fn dispatch() { nop(); }",
            "then cannot return",
        ),
        (
            "mhf_ai 1; species 6; fn main() { handle dispatch() then { pass; } } handler fn dispatch() { nop(); }",
            "then cannot pass",
        ),
        (
            "mhf_ai 1; species 6; fn main() { handle dispatch() then {} } handler fn dispatch() { handle leaf() then {} } handler fn leaf() { nop(); }",
            "a handler cannot dispatch again",
        ),
        (
            "mhf_ai 1; species 6; fn main() { handle dispatch() then {} } handler fn dispatch() { leaf(); nop(); } handler fn leaf() { nop(); }",
            "a handler call must be the last action",
        ),
    ] {
        assert!(
            parse(source)
                .and_then(|document| document.compile())
                .is_err(),
            "{reason}: {source}"
        );
    }
}

#[test]
fn mark_unhandled_continues_in_entries_and_imported_helpers() {
    let p = project(
        "mhf_ai 1; species 6; map 31; import \"helper.mhai\" as h;
         fn main() { mark_unhandled(); wait(1); h.mark(); }
         states { patrol = 1 => { mark_unhandled(); wait(2); h.mark(); } }
         events { awareness => { mark_unhandled(); wait(3); h.mark(); } }",
        &[(
            "maps/31/6/helper.mhai",
            "fn mark() { if self.enraged { mark_unhandled(); wait(4); } wait(5); }",
        )],
    );
    let compiled = p.compile().unwrap();
    assert!(compiled.warnings.is_empty());
    for (index, delay) in [(0, 1), (1, 2)] {
        assert_eq!(
            script(&compiled.program, 0, index),
            [0x0d, 4, 0x48, delay, 0x81, 0, 0xff, 0]
        );
    }
    let event = &EVENT_SLOTS[3];
    assert_eq!(
        script(&compiled.program, event.root_index, 0),
        [0x0d, 4, 0x48, 3, 0x81, 0, 0xff, event.ending]
    );
    assert_eq!(
        script(&compiled.program, 1, 0),
        [0x35, 0, 0x0d, 4, 0x48, 4, 0x35, 2, 0x48, 5, 0xff, 1]
    );
}

#[test]
fn mark_unhandled_continues_in_handlers_and_then_blocks() {
    let compiled = parse(
        "mhf_ai 1; species 6;
         fn main() {
             handle dispatch() then { mark_unhandled(); wait(2); }
             wait(3);
         }
         handler fn dispatch() { mark_unhandled(); wait(1); }",
    )
    .unwrap()
    .compile()
    .unwrap();
    assert!(compiled.warnings.is_empty());
    assert_eq!(
        script(&compiled.program, 0, 0),
        [
            0x1b, 0, 1, 0x0c, 4, 1, 0x81, 0, 0x2b, 0, 4, 1, 0x0d, 4, 0x48, 2, 0x2b, 2, 0x1b, 2,
            0x48, 3, 0xff, 0,
        ]
    );
    assert_eq!(script(&compiled.program, 1, 0), [0x0d, 4, 0x48, 1, 0xff, 1]);
}

#[test]
fn mark_unhandled_requires_an_argumentless_bare_call() {
    for body in [
        "mark_unhandled(0);",
        "mark_unhandled(0, 1);",
        "mark_unhandled;",
        "mark_unhandled = 0;",
        "self.mark_unhandled();",
        "context.mark_unhandled();",
        "if mark_unhandled() {}",
        "clear_takeover();",
    ] {
        let source = format!("mhf_ai 1; species 6; fn main() {{ {body} }}");
        assert!(
            parse(&source)
                .and_then(|document| document.compile())
                .is_err(),
            "{body}"
        );
    }
}

#[test]
fn ordinary_pass_matches_mark_unhandled_and_return() {
    for body in [
        "if self.enraged { EXIT } wait(3);",
        "random { 1 => { EXIT } 1 => wait(2); } wait(3);",
        "match context.debug_mode { 0 => { EXIT } } wait(3);",
        "match self.species { 0 => { EXIT } } wait(3);",
    ] {
        for declaration in ["", "@slot(table = 22, index = 1)"] {
            let compile = |exit| {
                parse(&format!(
                    "mhf_ai 1; species 6; fn main() {{ helper(); wait(9); }} {declaration} fn helper() {{ {} }}",
                    body.replace("EXIT", exit)
                )).unwrap().compile().unwrap()
            };
            let passing = compile("pass;");
            let explicit = compile("mark_unhandled(); return;");
            assert!(explicit.warnings.is_empty());
            assert_eq!(
                script(&passing.program, 0, 0),
                script(&explicit.program, 0, 0)
            );
            if !declaration.is_empty() {
                assert_eq!(
                    script(&passing.program, 22, 1),
                    script(&explicit.program, 22, 1)
                );
            }
            assert!(
                script(&passing.program, 0, 0)
                    .windows(2)
                    .any(|w| w == [0x48, 9])
            );
        }
    }
}

#[test]
fn source_maps_keep_imported_identity_spans_and_explicit_returns() {
    let entry = "mhf_ai 1;\nspecies 6;\nmap 31;\nimport \"helper.mhai\" as h;\nfn main() {\n    h.act();\n}\n";
    let module = "fn act() {\n    native(0x92, 0x48, 3);\n    return;\n}\n";
    let mut p = project(entry, &[("maps/31/6/helper.mhai", module)]);
    // File order does not define identity or which file owns entry declarations.
    p.files.reverse();
    let compiled = p.compile().unwrap();
    let info = &compiled.debug_info;
    assert_eq!(info.files[0].path, p.entry);
    assert_eq!(info.files.len(), 2);
    let native: Vec<_> = info.positions("maps/31/6/helper.mhai", 2).collect();
    assert_eq!(native.len(), 2);
    for mapping in &native {
        assert_eq!(mapping.source.function, "act");
        assert_eq!((mapping.source.line, mapping.source.column), (2, 5));
        assert_eq!(
            (mapping.source.end_line, mapping.source.end_column),
            (2, 27)
        );
        assert_eq!(
            &module[mapping.source.byte_start..mapping.source.byte_end],
            "native(0x92, 0x48, 3);"
        );
        assert_eq!(info.lookup(mapping.script, mapping.end - 1), Some(*mapping));
    }
    assert_eq!((native[0].start, native[0].end), (0, 1));
    assert_eq!((native[1].start, native[1].end), (1, 3));
    let returned = info.positions("maps/31/6/helper.mhai", 3).next().unwrap();
    let Node::Script(bytes) = &compiled.program.nodes[returned.script] else {
        panic!()
    };
    assert_eq!(&bytes[returned.start..returned.end], [0xff, 1]);
    assert_eq!(
        &module[returned.source.byte_start..returned.source.byte_end],
        "return;"
    );
    assert!(info.lookup(returned.script, returned.end).is_none());
    let call = info.positions(&p.entry, 6).next().unwrap();
    assert_eq!(call.source.function, "main");
    assert_eq!(
        &entry[call.source.byte_start..call.source.byte_end],
        "h.act();"
    );
}

#[test]
fn source_maps_cover_each_emitted_instruction_once_and_distinguish_markers() {
    let p = project(
        "mhf_ai 1; species 6; map 31;\nfn main() {\n    if self.flashed {\n        nop();\n    } else {\n        wait(2);\n    }\n}\n",
        &[],
    );
    let compiled = p.compile().unwrap();
    let info = &compiled.debug_info;
    for (script, node) in compiled.program.nodes.iter().enumerate() {
        let Node::Script(bytes) = node else { continue };
        let instructions = crate::ai::bytecode::decode(bytes).unwrap();
        for instruction in instructions {
            let mapping = info.lookup(script, instruction.offset).unwrap();
            assert_eq!(mapping.start, instruction.offset);
            assert_eq!(mapping.end, instruction.offset + instruction.bytes.len());
            if matches!(instruction.bytes.as_slice(), [0x35, 1 | 2] | [0xff, 0]) {
                assert!(mapping.generated);
            }
        }
        let mappings: Vec<_> = info
            .mappings
            .iter()
            .filter(|mapping| mapping.script == script)
            .collect();
        assert_eq!(mappings.first().unwrap().start, 0);
        assert_eq!(mappings.last().unwrap().end, bytes.len());
        assert!(mappings.windows(2).all(|pair| pair[0].end == pair[1].start));
    }
    assert_eq!(info.positions(&p.entry, 3).count(), 1);
    assert_eq!(info.positions(&p.entry, 4).count(), 1);
    assert_eq!(info.positions(&p.entry, 6).count(), 1);
}

#[test]
fn source_maps_bind_every_copy_of_a_lowered_continuation() {
    let p = project(
        "mhf_ai 1; species 6; map 31;\nfn main() {\n    if self.flashed {\n        if self.active { return; }\n        wait(1);\n    }\n    wait(2);\n}\n",
        &[],
    );
    let compiled = p.compile().unwrap();
    let copies: Vec<_> = compiled.debug_info.positions(&p.entry, 7).collect();
    assert_eq!(copies.len(), 2);
    assert_ne!(copies[0].start, copies[1].start);
    assert_eq!(copies[0].source, copies[1].source);
    for mapping in copies {
        let Node::Script(bytes) = &compiled.program.nodes[mapping.script] else {
            panic!()
        };
        assert_eq!(&bytes[mapping.start..mapping.end], [0x48, 2]);
    }
}

#[test]
fn source_map_keeps_only_compiled_files_and_does_not_change_program_bytes() {
    let source = "mhf_ai 1; species 6; map 31; fn main() { native(0x92, 0xff, 0); }";
    let p = project(
        source,
        &[("maps/31/6/unused.mhai", "fn unused() { wait(9); }")],
    );
    let compiled = p.compile().unwrap();
    assert_eq!(
        compiled.program,
        parse(source).unwrap().compile().unwrap().program
    );
    assert_eq!(compiled.debug_info.files, [p.files[0].clone()]);
    assert_eq!(compiled.debug_info.positions(&p.entry, 1).count(), 2);
    assert!(
        compiled
            .debug_info
            .mappings
            .iter()
            .all(|mapping| !mapping.generated)
    );
}
