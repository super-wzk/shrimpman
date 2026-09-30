use super::*;
use crate::{
    edit,
    field::{FieldType, ScalarType},
    inspect::{self, Document, resource_path::Location},
};
use mhf_resource::{
    ResourcePath,
    dat::motion_events::{ChoiceEvent, CommandEvent, WeightedChoice},
    dat::{HEADER_SIZE, MAGIC, VERSION},
};
use std::path::Path;

#[derive(Clone, Copy)]
struct Offsets {
    commands: usize,
    choices: usize,
    command_entries: usize,
    choice_entries: usize,
    command_events: usize,
    choice_events: usize,
}

fn word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn dword(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn header(bytes: &mut [u8]) {
    bytes[..4].copy_from_slice(MAGIC);
    dword(bytes, 4, VERSION);
    dword(bytes, 12, HEADER_SIZE as u32);
}

fn image() -> (Vec<u8>, Offsets) {
    let commands = HEADER_SIZE;
    let choices = commands + 19 * 8;
    let command_entries = choices + 19 * 8;
    let choice_entries = command_entries + 2 * 8;
    let command_events = choice_entries + 2 * 8;
    let choice_events = command_events + 2 * CommandEvent::SIZE;
    let offsets = Offsets {
        commands,
        choices,
        command_entries,
        choice_entries,
        command_events,
        choice_events,
    };
    let mut bytes = vec![0; choice_events + 2 * ChoiceEvent::SIZE];
    header(&mut bytes);
    dword(&mut bytes, 665 * 4, 1);
    dword(&mut bytes, 390 * 4, commands as u32);
    dword(&mut bytes, 391 * 4, choices as u32);
    for (at, entries) in [
        (commands + 18 * 8, command_entries),
        (choices + 3 * 8, choice_entries),
    ] {
        word(&mut bytes, at, 2);
        word(&mut bytes, at + 2, 0xfffe);
        dword(&mut bytes, at + 4, entries as u32);
    }
    // The key is a native lookup value. Paths retain each original row ordinal.
    word(&mut bytes, command_entries, 7);
    dword(&mut bytes, command_entries + 4, u32::MAX);
    word(&mut bytes, command_entries + 8, 0xffff);
    word(&mut bytes, command_entries + 10, 2);
    dword(&mut bytes, command_entries + 12, command_events as u32);
    word(&mut bytes, choice_entries, 99);
    word(&mut bytes, choice_entries + 2, 2);
    dword(&mut bytes, choice_entries + 4, choice_events as u32);
    word(&mut bytes, choice_entries + 8, 0xffff);
    dword(&mut bytes, choice_entries + 12, 1);
    let command = CommandEvent {
        frame: 0xfffe,
        operation: 0xffff,
        arg_a: 0xabcd,
        arg_b: 0x8000,
        arg_c: 0xff,
        arg_d: 0x80,
        arg_e: 0x0123,
        arg_f: 0xfffd,
        arg_g: 0xffff,
    };
    for index in 0..2 {
        let at = command_events + index * CommandEvent::SIZE;
        bytes[at..at + CommandEvent::SIZE].copy_from_slice(&command.to_bytes());
    }
    let choice = ChoiceEvent {
        frame: 0xfffe,
        dispatch_kind: 0xffff,
        condition: 0x81,
        unknown_05: 0xff,
        choices: [
            WeightedChoice {
                id: 0xffff,
                weight: 0x8000,
            },
            WeightedChoice {
                id: 0xabcd,
                weight: 0xffff,
            },
            WeightedChoice {
                id: 0x1234,
                weight: 0xfffe,
            },
            WeightedChoice { id: 0, weight: 0 },
        ],
    };
    for index in 0..2 {
        let at = choice_events + index * ChoiceEvent::SIZE;
        bytes[at..at + ChoiceEvent::SIZE].copy_from_slice(&choice.to_bytes());
    }
    (bytes, offsets)
}

fn source(bytes: &[u8], base: usize) -> (Vec<u8>, &'static str) {
    let mut source = vec![0; base];
    let prefix = if base == 0 {
        "mhfdat.bin#"
    } else {
        dword(&mut source, 0, 1);
        dword(&mut source, 4, base as u32);
        dword(&mut source, 8, bytes.len() as u32);
        "mhfdat.bin#0/"
    };
    source.extend_from_slice(bytes);
    (source, prefix)
}

fn locate_field(
    document: &mut Document,
    root: &Path,
    path: &ResourcePath,
) -> (usize, Vec<usize>, usize) {
    loop {
        match document.locate_resource(root, path) {
            Location::Expand(node) => *document = inspect::expand(document, node).unwrap(),
            Location::Resolved {
                node,
                context,
                field: Some(field),
            } => return (node, context, field),
            location => panic!("could not locate motion event field {path}: {location:?}"),
        }
    }
}

fn check_field(
    document: &mut Document,
    root: &Path,
    path: String,
    at: usize,
    scalar: ScalarType,
    expected: u32,
) {
    let path = path.parse().unwrap();
    let (node, context, field) = locate_field(document, root, &path);
    let value = &document.nodes[node].fields[field];
    assert!(value.writable, "{path}");
    assert_eq!(value.binding.range, at..at + scalar.size(), "{path}");
    assert_eq!(value.binding.format, FieldType::Scalar(scalar), "{path}");
    assert_eq!(value.read(&document.buffers).unwrap(), expected.to_string());
    assert!(
        value
            .write(&document.buffers, &expected.to_string())
            .unwrap()
            .is_none(),
        "{path}"
    );
    document.nodes[node].name = "重命名节点".into();
    document.nodes[node].fields[field].name = "重命名字段".into();
    let address = document
        .resource_address(root, &context, Some(field))
        .unwrap();
    assert!(address.exact, "{path}");
    assert_eq!(address.path, path);
}

#[test]
fn paths_keep_native_widths_and_ordinals_inside_containers() {
    let (bytes, offsets) = image();
    let root = Path::new("/fixtures/dat");
    for base in [0, 32] {
        let (source, prefix) = source(&bytes, base);
        let mut document = inspect::inspect(root.join("mhfdat.bin"), source.into());
        for (kind, group, at, entries) in [
            (390, 18, offsets.commands + 18 * 8, offsets.command_entries),
            (391, 3, offsets.choices + 3 * 8, offsets.choice_entries),
        ] {
            for (key, offset, scalar, value) in [
                ("count", 0, ScalarType::U16, 2),
                ("unknown_02", 2, ScalarType::U16, 0xfffe),
                ("records_offset", 4, ScalarType::U32, entries as u32),
            ] {
                check_field(
                    &mut document,
                    root,
                    format!("{prefix}{kind}/{group}/{key}"),
                    base + at + offset,
                    scalar,
                    value,
                );
            }
        }
        for (kind, group, entry, at, key_value, events) in [
            (
                390,
                18,
                1,
                offsets.command_entries + 8,
                0xffff,
                offsets.command_events,
            ),
            (391, 3, 0, offsets.choice_entries, 99, offsets.choice_events),
        ] {
            for (key, offset, scalar, value) in [
                ("key", 0, ScalarType::U16, key_value),
                ("count", 2, ScalarType::U16, 2),
                ("events_offset", 4, ScalarType::U32, events as u32),
            ] {
                check_field(
                    &mut document,
                    root,
                    format!("{prefix}{kind}/{group}/{entry}/{key}"),
                    base + at + offset,
                    scalar,
                    value,
                );
            }
        }
        for (key, offset, scalar, value) in [
            ("frame", 0, ScalarType::U16, 0xfffe),
            ("operation", 2, ScalarType::U16, 0xffff),
            ("arg_a", 4, ScalarType::U16, 0xabcd),
            ("arg_b", 6, ScalarType::U16, 0x8000),
            ("arg_c", 8, ScalarType::U8, 0xff),
            ("arg_d", 9, ScalarType::U8, 0x80),
            ("arg_e", 10, ScalarType::U16, 0x0123),
            ("arg_f", 12, ScalarType::U16, 0xfffd),
            ("arg_g", 14, ScalarType::U16, 0xffff),
        ] {
            check_field(
                &mut document,
                root,
                format!("{prefix}390/18/1/events/1/{key}"),
                base + offsets.command_events + CommandEvent::SIZE + offset,
                scalar,
                value,
            );
        }
        for (key, offset, scalar, value) in [
            ("frame", 0, ScalarType::U16, 0xfffe),
            ("dispatch_kind", 2, ScalarType::U16, 0xffff),
            ("condition", 4, ScalarType::U8, 0x81),
            ("unknown_05", 5, ScalarType::U8, 0xff),
        ] {
            check_field(
                &mut document,
                root,
                format!("{prefix}391/3/0/events/1/{key}"),
                base + offsets.choice_events + ChoiceEvent::SIZE + offset,
                scalar,
                value,
            );
        }
        for (choice, id, weight) in [
            (0, 0xffff, 0x8000),
            (1, 0xabcd, 0xffff),
            (2, 0x1234, 0xfffe),
            (3, 0, 0),
        ] {
            for (key, offset, value) in [("id", 0, id), ("weight", 2, weight)] {
                check_field(
                    &mut document,
                    root,
                    format!("{prefix}391/3/0/events/1/choice_{choice}/{key}"),
                    base + offsets.choice_events + ChoiceEvent::SIZE + 6 + choice * 4 + offset,
                    ScalarType::U16,
                    value,
                );
            }
        }
    }
}

#[test]
fn typed_edits_change_only_the_bound_bytes_and_preserve_unknown_values() {
    let (bytes, offsets) = image();
    let root = Path::new("/fixtures/dat");
    let base = 32;
    let (mut expected, prefix) = source(&bytes, base);
    let mut document = inspect::inspect(root.join("mhfdat.bin"), expected.clone().into());
    for (suffix, input, at, after) in [
        (
            "390/18/1/events/1/arg_c",
            "18",
            offsets.command_events + CommandEvent::SIZE + 8,
            vec![18],
        ),
        (
            "390/18/1/events/1/arg_g",
            "32769",
            offsets.command_events + CommandEvent::SIZE + 14,
            32769_u16.to_le_bytes().to_vec(),
        ),
        (
            "391/3/0/events/1/unknown_05",
            "254",
            offsets.choice_events + ChoiceEvent::SIZE + 5,
            vec![254],
        ),
        (
            "391/3/0/events/1/choice_2/weight",
            "4660",
            offsets.choice_events + ChoiceEvent::SIZE + 16,
            4660_u16.to_le_bytes().to_vec(),
        ),
    ] {
        let path = format!("{prefix}{suffix}").parse().unwrap();
        let (node, _, field) = locate_field(&mut document, root, &path);
        let value = &document.nodes[node].fields[field];
        let patch = value.write(&document.buffers, input).unwrap().unwrap();
        assert_eq!(patch.binding.range, base + at..base + at + after.len());
        assert_eq!(patch.after, after);
        expected[patch.binding.range.clone()].copy_from_slice(&after);
        document = edit::apply_many(&document, &[patch]).unwrap();
        assert_eq!(&*document.buffers[0], expected);
    }
    let dat = &document.buffers[0][base..];
    let first_command = CommandEvent::parse(
        &dat[offsets.command_events..offsets.command_events + CommandEvent::SIZE],
    )
    .unwrap();
    let second_command = CommandEvent::parse(
        &dat[offsets.command_events + CommandEvent::SIZE..offsets.choice_events],
    )
    .unwrap();
    assert_eq!(second_command.operation, 0xffff);
    assert_eq!(second_command.arg_a, first_command.arg_a);
    assert_eq!(second_command.arg_c, 18);
    assert_eq!(second_command.arg_g, 32769);
    let second_choice = ChoiceEvent::parse(
        &dat[offsets.choice_events + ChoiceEvent::SIZE
            ..offsets.choice_events + 2 * ChoiceEvent::SIZE],
    )
    .unwrap();
    assert_eq!(second_choice.dispatch_kind, 0xffff);
    assert_eq!(second_choice.condition, 0x81);
    assert_eq!(second_choice.unknown_05, 254);
    assert_eq!(second_choice.choices[2].id, 0x1234);
    assert_eq!(second_choice.choices[2].weight, 4660);
}

#[test]
fn empty_roots_groups_and_entries_keep_raw_headers_without_expansion() {
    let root = Path::new("/fixtures/dat");
    let mut bytes = vec![0; HEADER_SIZE];
    header(&mut bytes);
    let document = inspect::inspect(root.join("mhfdat.bin"), bytes.into());
    assert!(!document.nodes.iter().any(|node| matches!(
        node.kind,
        Kind::DatMotionEventGroup(..) | Kind::DatMotionEventEntry(..)
    )));
    let (bytes, offsets) = image();
    let mut document = inspect::inspect(root.join("mhfdat.bin"), bytes.into());
    for (suffix, at, scalar, expected) in [
        ("390/0/count", offsets.commands, ScalarType::U16, 0),
        ("390/18/0/key", offsets.command_entries, ScalarType::U16, 7),
        (
            "390/18/0/events_offset",
            offsets.command_entries + 4,
            ScalarType::U32,
            u32::MAX,
        ),
        (
            "391/3/1/events_offset",
            offsets.choice_entries + 12,
            ScalarType::U32,
            1,
        ),
    ] {
        let path: ResourcePath = format!("mhfdat.bin#{suffix}").parse().unwrap();
        let (node, _, _) = locate_field(&mut document, root, &path);
        assert!(!document.nodes[node].deferred);
        assert!(document.nodes[node].children.is_empty());
        assert!(document.nodes[node].error.is_none());
        check_field(&mut document, root, path.to_string(), at, scalar, expected);
    }
}

#[test]
fn damaged_groups_and_entries_do_not_hide_other_motion_events() {
    let (mut bytes, offsets) = image();
    // A nonempty group with no record pointer remains visible with a local error.
    word(&mut bytes, offsets.commands + 8, 1);
    // The valid choice group has one broken entry and one working sibling.
    word(&mut bytes, offsets.choice_entries + 10, 1);
    dword(&mut bytes, offsets.choice_entries + 12, u32::MAX);
    let root = Path::new("/fixtures/dat");
    let mut document = inspect::inspect(root.join("mhfdat.bin"), bytes.into());
    let bad_group = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatMotionEventGroup(EventKind::Command, 1))
        .unwrap();
    assert!(document.nodes[bad_group].error.is_some());
    assert!(!document.nodes[bad_group].deferred);
    for suffix in ["390/18/1/events/1/arg_c", "391/3/0/events/1/choice_0/id"] {
        let path = format!("mhfdat.bin#{suffix}").parse().unwrap();
        let (node, _, _) = locate_field(&mut document, root, &path);
        assert!(document.nodes[node].error.is_none());
    }
    let bad_entry = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatMotionEventEntry(EventKind::Choice, 3, 1))
        .unwrap();
    assert!(document.nodes[bad_entry].error.is_some());
    assert!(!document.nodes[bad_entry].deferred);
    assert_eq!(document.nodes[bad_entry].fields.len(), 3);
}
