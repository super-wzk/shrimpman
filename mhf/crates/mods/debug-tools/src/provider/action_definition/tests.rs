use super::*;
use mhf_resource::action_definition::ActionEvent;
use std::fs;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mhf-debug-attack-resources-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        Self(directory.join("mhfsdt.bin"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn sdt_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x100 + 10 * sdt::ATTACK_STRIDE];
    for (index, subtype, category, pointer) in [
        (0, 0_u16, 999_u16, u32::MAX),
        (1, 2, 100, 0x100),
        (2, 1, 100, 0x100),
    ] {
        let offset = index * sdt::DIRECTORY_STRIDE;
        bytes[offset..offset + 2].copy_from_slice(&subtype.to_le_bytes());
        bytes[offset + 2..offset + 4].copy_from_slice(&category.to_le_bytes());
        bytes[offset + 4..offset + 6].copy_from_slice(&10_u16.to_le_bytes());
        bytes[offset + 8..offset + 12].copy_from_slice(&pointer.to_le_bytes());
    }
    let end = 3 * sdt::DIRECTORY_STRIDE;
    bytes[end + 2..end + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes
}

fn encoded_sdt(payload: &[u8], filename: &[u8]) -> Vec<u8> {
    let mut jkr = b"JKR\x1a\x08\x01\x00\x00".to_vec();
    jkr.extend_from_slice(&16_u32.to_le_bytes());
    jkr.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    jkr.extend_from_slice(payload);
    let mut envelope = b"ecd\x1a\x04\x00\x00\x00".to_vec();
    envelope.extend_from_slice(&[0; 8]);
    Ecd::parse(&envelope)
        .unwrap()
        .encode(&jkr, Some(filename))
        .unwrap()
}

fn definition() -> Definition {
    let event = ActionEvent {
        step: 0,
        timing: 1,
        phase: 0,
        frame: 0,
        count: 0,
        operation: 4,
        argument: 8,
    };
    Definition {
        weapon: 11,
        action: 0,
        offset: 0,
        steps_range: 0..0,
        events_range: 0..36,
        steps: Vec::new(),
        events: vec![
            ActionEvent {
                operation: u16::MAX,
                ..event
            },
            event,
            ActionEvent {
                argument: 10,
                ..event
            },
        ],
    }
}

#[test]
fn unsupported_action_domains_do_not_enter_the_weapon_directory() {
    for (group, weapon) in [(0, 7), (1, 14)] {
        let error = read(
            &[],
            0,
            super::super::Action {
                group,
                weapon,
                id: 0,
            },
        )
        .unwrap_err();
        assert!(error.contains("不使用 DAT[389]"));
    }
}

#[test]
fn encoded_attack_resources_resolve_original_indices_once_and_keep_local_errors() {
    let fixture = Fixture::new("cache");
    fs::write(&fixture.0, encoded_sdt(&sdt_fixture(), b"mhfsdt.bin")).unwrap();
    let resources = AttackResources::new(fixture.0.clone());
    assert!(resources.directory.get().is_none());
    let definition = definition();
    let snapshot = resources.snapshot(&definition).unwrap();
    let directory = snapshot.as_ref().as_ref().unwrap();
    let reference = definition.events[1].attack_reference(11).unwrap();
    assert_eq!(
        directory.resolve(reference).unwrap().unwrap().to_string(),
        "mhfsdt.bin#2/attacks/8"
    );
    assert!(
        directory
            .resolve(definition.events[2].attack_reference(11).unwrap())
            .unwrap_err()
            .to_string()
            .contains("record index out of range")
    );
    fs::remove_file(&fixture.0).unwrap();
    assert!(
        Arc::ptr_eq(&resources.snapshot(&definition).unwrap(), &snapshot),
        "cached file indices remain immutable"
    );
}

#[test]
fn absent_or_invalid_attack_resources_stay_unresolved_with_specific_diagnostics() {
    let fixture = Fixture::new("errors");
    let definition = definition();
    for (bytes, error) in [
        (None, "读取攻击资源"),
        (Some(b"bad".to_vec()), "解析 mhfsdt.bin 原文件目录"),
        (Some(encoded_sdt(&sdt_fixture(), b"other.bin")), "文件名"),
    ] {
        if let Some(bytes) = bytes {
            fs::write(&fixture.0, bytes).unwrap();
        }
        let snapshot = AttackResources::new(fixture.0.clone())
            .snapshot(&definition)
            .unwrap();
        assert!(matches!(snapshot.as_ref(), Err(detail) if detail.contains(error)));
    }
}

#[test]
fn unsupported_callbacks_do_not_read_attack_resources() {
    let mut definition = definition();
    definition.events.truncate(1);
    let resources = AttackResources::new("missing/mhfsdt.bin".into());
    assert!(resources.snapshot(&definition).is_none());
    assert!(resources.directory.get().is_none());
}
