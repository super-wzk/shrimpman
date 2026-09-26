use super::*;
use crate::inspect::Kind;
use std::fs;
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
