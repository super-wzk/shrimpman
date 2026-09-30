use super::*;
use crate::inspect::Kind;
use std::fs;

#[test]
fn background_attack_directory_uses_source_root_and_preserves_edits() {
    use mhf_resource::{action_definition::AttackReference, dat, sdt};
    use std::time::{Duration, Instant};

    let directory = std::env::temp_dir().join(format!(
        "mhf-workbench-attack-directory-{}",
        std::process::id()
    ));
    let data = directory.join("dat");
    let scan = data.join("stage");
    fs::create_dir_all(&scan).unwrap();
    let source = data.join("mhfdat.bin");
    let mut bytes = vec![0; dat::HEADER_SIZE];
    bytes[..4].copy_from_slice(dat::MAGIC);
    bytes[4..8].copy_from_slice(&dat::VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(dat::HEADER_SIZE as u32).to_le_bytes());
    fs::write(&source, bytes).unwrap();
    let mut sdt = vec![0; 64 + 24 * sdt::ATTACK_STRIDE];
    for (index, subtype) in [2_u16, 1].into_iter().enumerate() {
        let offset = index * sdt::DIRECTORY_STRIDE;
        sdt[offset..offset + 2].copy_from_slice(&subtype.to_le_bytes());
        sdt[offset + 4..offset + 6].copy_from_slice(&24_u16.to_le_bytes());
        sdt[offset + 8..offset + 12].copy_from_slice(&64_u32.to_le_bytes());
    }
    sdt[58..60].copy_from_slice(&u16::MAX.to_le_bytes());
    let sdt_path = data.join("mhfsdt.bin");
    fs::write(&sdt_path, sdt).unwrap();
    let reference = AttackReference {
        category: 0,
        subtype: None,
        record: 23,
    };
    let mut worker = Worker::start(scan, directory.join("exports")).unwrap();
    worker.load(17, source.clone(), data);
    let deadline = Instant::now() + Duration::from_secs(3);
    let loaded = loop {
        if let Some(loaded) = worker.updates().loaded {
            break loaded;
        }
        assert!(Instant::now() < deadline, "background load did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    worker.stop();
    assert_eq!(loaded.request, 17);
    assert_eq!(loaded.path, source);
    let document = loaded.document.unwrap();
    let cached = document.attack_directory.as_ref().unwrap();
    assert_eq!(
        cached
            .as_ref()
            .as_ref()
            .unwrap()
            .resolve(reference)
            .unwrap()
            .unwrap()
            .to_string(),
        "mhfsdt.bin#1/attacks/23"
    );
    let edited = edit::apply(&document, 0, 8..12, &1_u32.to_le_bytes()).unwrap();
    assert!(Arc::ptr_eq(
        edited.attack_directory.as_ref().unwrap(),
        cached
    ));

    let sdt_document = read_document(&sdt_path).unwrap();
    let edited = edit::apply(&sdt_document, 0, 0..2, &0_u16.to_le_bytes()).unwrap();
    assert_eq!(
        edited
            .attack_directory
            .as_ref()
            .unwrap()
            .as_ref()
            .as_ref()
            .unwrap()
            .resolve(reference)
            .unwrap()
            .unwrap()
            .to_string(),
        "mhfsdt.bin#0/attacks/23"
    );
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn stopping_drains_an_accepted_save_including_pending_field_input() {
    let directory =
        std::env::temp_dir().join(format!("mhf-workbench-save-stop-{}", std::process::id()));
    let data = directory.join("dat");
    let output = directory.join("redirect");
    fs::create_dir_all(&data).unwrap();
    let source = data.join("value.bin");
    fs::write(&source, [1, 2]).unwrap();
    let document = Arc::new(read_document(&source).unwrap());
    let binding = crate::field::Binding {
        buffer: 0,
        range: 0..1,
        format: crate::field::FieldType::Scalar(crate::field::ScalarType::U8),
        endian: mhf_resource::binary::Endian::Little,
    };
    let patch = binding.write(&document.buffers, "9").unwrap().unwrap();
    let mut worker = Worker::start(data.clone(), directory.join("exports")).unwrap();
    worker.pack(source.clone(), data, output.clone(), document, vec![patch]);
    worker.stop();
    assert_eq!(fs::read(output.join("value.bin")).unwrap(), [9, 2]);
    assert_eq!(fs::read(source).unwrap(), [1, 2]);
    assert!(worker.updates().packed.unwrap().result.is_ok());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn packing_name_repairs_return_the_written_document_and_keep_raw_exports_verbatim() {
    use crate::session::Session;
    use mhf_resource::crypto::{Ecd, Exf};

    let directory =
        std::env::temp_dir().join(format!("mhf-workbench-name-repair-{}", std::process::id()));
    let data = directory.join("dat");
    let output = directory.join("redirect");
    fs::create_dir_all(&data).unwrap();
    let ecd = Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
        .unwrap()
        .encode(b"payload", Some(b"donor.bin"))
        .unwrap();
    let exf = Exf::parse(b"exf\x1a\x04\0\0\0\0\0\0\0\x12\x34\x56\x78")
        .unwrap()
        .encode(b"audio", Some(b"donor.mus"))
        .unwrap();
    for (name, bytes) in [("target.bin", ecd), ("target.mus", exf)] {
        let source = data.join(name);
        fs::write(&source, &bytes).unwrap();
        let document = Arc::new(read_document(&source).unwrap());
        let raw = Export::from_node(&document, document.root).unwrap();
        assert_eq!(&raw.bytes[raw.range], bytes);
        let mut session = Session::new(document.clone());
        let mut worker = Worker::start(data.clone(), directory.join("exports")).unwrap();
        worker.pack(
            source.clone(),
            data.clone(),
            output.clone(),
            document.clone(),
            Vec::new(),
        );
        worker.stop();
        let packed = worker.updates().packed.unwrap();
        let (target, saved) = packed.result.unwrap();
        assert!(Arc::ptr_eq(&packed.requested, &document));
        let written = fs::read(target).unwrap();
        assert_eq!(written.as_slice(), saved.buffers[0].as_ref());
        assert_ne!(written, bytes);
        assert_eq!(&written[..6], &bytes[..6]);
        assert_eq!(&written[8..], &bytes[8..]);
        assert_eq!(fs::read(source).unwrap(), bytes);
        match saved.nodes[saved.root].kind {
            Kind::Ecd => Ecd::parse(&written)
                .unwrap()
                .validate_filename(name.as_bytes())
                .unwrap(),
            Kind::Exf => Exf::parse(&written)
                .unwrap()
                .validate_filename(name.as_bytes())
                .unwrap(),
            _ => panic!("packing must preserve the wrapper kind"),
        }
        assert!(session.saved(&packed.requested, &saved));
        assert!(!session.dirty());
        let raw = Export::from_node(&session.document, session.document.root).unwrap();
        assert_eq!(&raw.bytes[raw.range], written);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn worker_can_stop_while_scan_is_starting_or_waiting() {
    let directory = std::env::temp_dir().join(format!("mhf-workbench-stop-{}", std::process::id()));
    for _ in 0..8 {
        let mut worker = Worker::start(directory.clone(), directory.clone()).unwrap();
        worker.stop();
        worker.stop();
        assert!(worker.thread.is_none());
    }
}
