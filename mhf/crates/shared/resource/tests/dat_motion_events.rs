pub mod support;

use mhf_resource::{
    container::open_layers,
    dat::{
        self, Dat,
        motion_events::{ChoiceEvent, CommandEvent, Directory, EventKind, WeightedChoice},
    },
};
use support::{dword as set_u32, word as set_u16};

fn image(size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[..4].copy_from_slice(dat::MAGIC);
    set_u32(&mut bytes, 4, dat::VERSION);
    set_u32(&mut bytes, 12, dat::HEADER_SIZE as u32);
    bytes
}

fn fixture(kind: EventKind, base: u32) -> Vec<u8> {
    let mut bytes = image(3500);
    set_u32(&mut bytes, kind.root() as usize * 4, base + 3040);
    set_u32(&mut bytes, 665 * 4, 1);
    // A 19-group directory whose last group preserves its opaque high word.
    set_u16(&mut bytes, 3040 + 18 * 8, 2);
    set_u16(&mut bytes, 3042 + 18 * 8, 0xbeef);
    set_u32(&mut bytes, 3044 + 18 * 8, base + 3240);
    set_u16(&mut bytes, 3240, 1307);
    set_u16(&mut bytes, 3242, 2);
    set_u32(&mut bytes, 3244, base + 3400);
    set_u16(&mut bytes, 3248, 2001);
    // Empty entry and groups deliberately contain pointers that cannot resolve.
    set_u32(&mut bytes, 3252, u32::MAX);
    set_u32(&mut bytes, 3044, 12);
    bytes
}

#[test]
fn event_records_preserve_raw_parameters_unknown_values_and_signed_bit_patterns() {
    let command = CommandEvent {
        frame: u16::MAX,
        operation: 0xabcd,
        arg_a: (-2i16) as u16,
        arg_b: 0x8000,
        arg_c: 0xa5,
        arg_d: 0xd7,
        arg_e: (-100i16) as u16,
        arg_f: 0x8001,
        arg_g: 0x7fff,
    };
    let raw_command = command.to_bytes();
    assert_eq!(raw_command[8..10], [0xa5, 0xd7]);
    assert_eq!(CommandEvent::parse(&raw_command).unwrap(), command);
    let choice = ChoiceEvent {
        frame: 1234,
        dispatch_kind: 0xffff,
        condition: 0x9a,
        unknown_05: 0xc3,
        choices: [
            WeightedChoice {
                id: 0xffff,
                weight: 0,
            },
            WeightedChoice { id: 420, weight: 1 },
            WeightedChoice {
                id: 0x8000,
                weight: u16::MAX,
            },
            WeightedChoice {
                id: 255,
                weight: 321,
            },
        ],
    };
    let raw_choice = choice.to_bytes();
    assert_eq!(raw_choice[4..6], [0x9a, 0xc3]);
    assert_eq!(ChoiceEvent::parse(&raw_choice).unwrap(), choice);
    for size in [0, 15, 17, 32] {
        assert!(CommandEvent::parse(&vec![0; size]).is_err());
    }
    for size in [0, 21, 23, 44] {
        assert!(ChoiceEvent::parse(&vec![0; size]).is_err());
    }
}

#[test]
fn group_and_event_ranges_are_identical_after_native_relocation() {
    for kind in [EventKind::Command, EventKind::Choice] {
        let bytes = fixture(kind, 0);
        let directory = Directory::parse(&bytes, 0, kind).unwrap();
        let mut loaded = fixture(kind, 0x1234_0000);
        // The native initializer changes the final magic byte after relocation.
        loaded[3] = 0;
        let relocated = Directory::parse(&loaded, 0x1234_0000, kind).unwrap();
        assert_eq!(directory.count, 19);
        assert_eq!(directory.additional_groups, 1);
        assert_eq!(directory.range, 3040..3192);
        assert_eq!(directory.range, relocated.range);
        let group = directory.group(18).unwrap();
        assert_eq!(group.offset, 3184);
        assert_eq!(group.count, 2);
        assert_eq!(group.unknown_02, 0xbeef);
        assert_eq!(group.records, 3240..3256);
        assert_eq!(group, relocated.group(18).unwrap());
        let entry = directory.entry(18, 0).unwrap();
        assert_eq!(entry.offset, 3240);
        assert_eq!(entry.key, 1307);
        assert_eq!(entry.count, 2);
        assert_eq!(entry.events, 3400..3400 + 2 * kind.record_size());
        assert_eq!(entry, relocated.entry(18, 0).unwrap());
        assert_eq!(directory.entry(18, 1).unwrap().events, 0..0);
        assert_eq!(directory.group(0).unwrap().records, 0..0);
        assert!(directory.group(19).is_err());
        assert!(directory.entry(18, 2).is_err());
        assert!(directory.entry(0, 0).is_err());
    }
}

#[test]
fn loader_count_uses_wrapping_low_word_and_preserves_full_scalar() {
    let mut bytes = image(3400);
    set_u32(&mut bytes, 390 * 4, 3040);
    set_u32(&mut bytes, 665 * 4, 0xabcd_0002);
    let directory = Directory::parse(&bytes, 0, EventKind::Command).unwrap();
    assert_eq!(directory.additional_groups, 0xabcd_0002);
    assert_eq!(directory.count, 20);
    assert_eq!(directory.range, 3040..3200);
    set_u32(&mut bytes, 665 * 4, 0xffee);
    set_u32(&mut bytes, 390 * 4, u32::MAX);
    let directory = Directory::parse(&bytes, 0, EventKind::Command).unwrap();
    assert_eq!(directory.count, 0);
    assert_eq!(directory.range, 0..0);
    assert!(directory.group(0).is_err());
    set_u32(&mut bytes, 665 * 4, u32::MAX);
    set_u32(&mut bytes, 390 * 4, 3040);
    let directory = Directory::parse(&bytes, 0, EventKind::Command).unwrap();
    assert_eq!(directory.additional_groups, u32::MAX);
    assert_eq!(directory.count, 17);
}

#[test]
fn invalid_nested_pointers_are_rejected_only_when_their_table_is_read() {
    for kind in [EventKind::Command, EventKind::Choice] {
        let mut bytes = fixture(kind, 0);
        // Group 1 is damaged; group 18 still decodes without reading it.
        set_u16(&mut bytes, 3048, 1);
        set_u32(&mut bytes, 3052, 0);
        // Entry 1 in the valid group is damaged independently of entry 0.
        set_u16(&mut bytes, 3250, 1);
        set_u32(&mut bytes, 3252, 0);
        let directory = Directory::parse(&bytes, 0, kind).unwrap();
        assert_eq!(directory.group(1).unwrap_err().offset, 3052);
        assert!(directory.entry(18, 0).is_ok());
        assert_eq!(directory.entry(18, 1).unwrap_err().offset, 3252);
        for pointer in [0, 12, 3499, u32::MAX] {
            set_u32(&mut bytes, 3244, pointer);
            let directory = Directory::parse(&bytes, 0, kind).unwrap();
            assert_eq!(directory.entry(18, 0).unwrap_err().offset, 3244);
        }
        for pointer in [0, 12, 3499, u32::MAX] {
            set_u32(&mut bytes, 3188, pointer);
            let directory = Directory::parse(&bytes, 0, kind).unwrap();
            assert_eq!(directory.group(18).unwrap_err().offset, 3188);
        }
        for pointer in [0, 12, 3499, u32::MAX] {
            set_u32(&mut bytes, kind.root() as usize * 4, pointer);
            assert_eq!(
                Directory::parse(&bytes, 0, kind).unwrap_err().offset,
                kind.root() as usize * 4,
            );
        }
    }
}

#[test]
fn relocated_pointers_below_base_or_inside_the_header_are_rejected() {
    let base = 0x1234_0000;
    for kind in [EventKind::Command, EventKind::Choice] {
        let mut bytes = fixture(kind, base);
        for pointer in [base - 1, base, base + dat::HEADER_SIZE as u32 - 1] {
            set_u32(&mut bytes, 3244, pointer);
            let directory = Directory::parse(&bytes, base, kind).unwrap();
            assert_eq!(directory.entry(18, 0).unwrap_err().offset, 3244);
        }
        set_u32(&mut bytes, 3244, base + 3400);
        set_u16(&mut bytes, 3242, u16::MAX);
        let directory = Directory::parse(&bytes, base, kind).unwrap();
        assert_eq!(directory.entry(18, 0).unwrap_err().offset, 3244);
        set_u16(&mut bytes, 3184, u16::MAX);
        let directory = Directory::parse(&bytes, base, kind).unwrap();
        assert_eq!(directory.group(18).unwrap_err().offset, 3188);
    }
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original mhfdat.bin only"]
fn original_dat_motion_event_directories_preserve_every_event() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
    let opened = open_layers(&source, usize::MAX, 16).unwrap();
    let dat = Dat::parse(opened.payload()).unwrap();
    let bytes = dat.as_bytes();
    let additional = dat.u32(665 * 4).unwrap();
    assert!(additional <= u32::from(u16::MAX));
    for kind in [EventKind::Command, EventKind::Choice] {
        let directory = Directory::parse(bytes, 0, kind).unwrap();
        assert_eq!(directory.additional_groups, additional);
        assert_eq!(directory.count, additional as usize + 18);
        let mut entries = 0usize;
        let mut events = 0usize;
        for group_index in 0..directory.count {
            let group = directory.group(group_index).unwrap();
            let mut previous = None;
            for index in 0..usize::from(group.count) {
                let entry = directory.entry(group_index, index).unwrap();
                if let Some(previous) = previous {
                    assert!(
                        previous <= entry.key,
                        "DAT[{}] group {group_index}",
                        kind.root()
                    );
                }
                previous = Some(entry.key);
                let raw_events = &bytes[entry.events.clone()];
                for raw in raw_events.chunks_exact(kind.record_size()) {
                    match kind {
                        EventKind::Command => {
                            assert_eq!(CommandEvent::parse(raw).unwrap().to_bytes(), raw)
                        }
                        EventKind::Choice => {
                            assert_eq!(ChoiceEvent::parse(raw).unwrap().to_bytes(), raw)
                        }
                    }
                }
                entries += 1;
                events += usize::from(entry.count);
            }
            println!(
                "DAT[{}] group {group_index}: {} entries",
                kind.root(),
                group.count
            );
        }
        println!(
            "DAT[{}]: {} groups, {entries} entries, {events} events",
            kind.root(),
            directory.count
        );
    }
}
