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
        states { idle -> idle; fight = 4 -> fight; recovery -> recovery; }
        events { dung_reaction -> combat.attack; invalid_ground -> combat.attack; }
        fn idle() { combat.attack(); transition fight; }
        fn fight() { transition recovery; }
        fn recovery() { transition idle; }";
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
    assert_eq!(script(&c.program, 0, 5), [7, 0]);
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
        events { bait_detected -> end; group_signal -> end; rage_entered -> end;
                 awareness -> end; player_detected -> end; invalid_ground -> end; dung_reaction -> end; }
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
        parse("mhf_ai 1; species 6; events { group_signal -> end; dung_reaction -> end; rage_entered -> end; } fn end() {}")
            .unwrap();
    assert_eq!(
        doc.events.iter().map(|e| e.slot).collect::<Vec<_>>(),
        [5, 0, 4]
    );
}

#[test]
fn rejects_invalid_calls_and_bindings() {
    for body in [
        "states { a -> absent; } fn end() {}",
        "states { a -> end; } fn end() { end(); }",
        "states { a -> end; } fn end() { other(); } fn other() { end(); }",
        "states { a -> end; } fn end() { other(1); transition a; } fn other() {}",
        "events { bait_detected = 6 -> end; } fn end() {}",
        "events { 0 -> end; } fn end() {}",
        "events { event_0 -> end; } fn end() {}",
        "events { kehai -> end; } fn end() {}",
        "events { awareness -> end; } fn end() { kehai_end(); }",
        "events { invalid_ground -> end; } fn end() { no_floor_end(); }",
        "events { player_detected -> end; } fn end() { find_end(); }",
        "events { awareness -> end; awareness -> end; } fn end() {}",
        "events { awareness -> end; } fn end() {} fn end() {}",
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
        ("transition a; wait(9);", vec![0x07, 0]),
        ("restart;", vec![0x04]),
        ("native(0xff, 0x00);", vec![0xff, 0x00]),
        ("stop();", vec![0x68]),
    ] {
        let source = format!(
            "mhf_ai 1; species 6; base native; states {{ a -> main; }}
             fn main() {{ {body} }} fn helper() {{ wait(1); return; wait(9); }}"
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
        states { idle -> start; } fn start() { a.end(); b.end(); transition idle; }",
        &[("common/6/a.mhai", "fn end() { nop(); }")],
    );
    assert_eq!(
        script(&p.compile().unwrap().program, 0, 0),
        [0x92, 0x92, 7, 0]
    );
    assert!(p.check_target(32, 6).is_err());
    assert!(p.check_target(31, 7).is_err());
    let mut mismatch = p.clone();
    mismatch.files[0].source = mismatch.files[0].source.replace("species 6", "species 7");
    assert!(mismatch.compile().is_err());
}

#[test]
fn native_conditional_transitions_survive_function_compilation() {
    let text = "mhf_ai 1; species 6; base native;
        states { idle -> body; } fn body() {
            native(0x0b,0,0); transition idle; native(0x0b,2); native(0xff,0);
        }";
    assert_eq!(
        script(&parse(text).unwrap().compile().unwrap().program, 0, 0),
        [0x0b, 0, 0, 7, 0, 0x0b, 2, 0xff, 0]
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
    dir.write("common/6/main.mhai", "mhf_ai 1; species 6; base native; import \"helper.mhai\" as h; events { dung_reaction -> h.finish; }");
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
