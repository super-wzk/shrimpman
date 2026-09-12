use super::*;
use crate::{preview::Control, settings::ViewSettings, worker::Worker};
use mhf_resource::binary::Reader;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

fn document(path: &Path, byte: u8) -> Arc<Document> {
    let bytes = [byte];
    let mut document = crate::inspect::inspect(&path.to_string_lossy(), bytes.to_vec().into());
    document.nodes[document.root]
        .fields
        .push(Field::from_binary(
            "value",
            0,
            Reader::new(&bytes).read::<u8>().unwrap(),
        ));
    Arc::new(document)
}

fn input(document: &Document) -> Input {
    let field = &document.nodes[document.root].fields[0];
    Input::new(
        Target::Field {
            node: edit::node_key(document, document.root).unwrap(),
            index: 0,
            name: field.name.clone(),
        },
        field.binding.clone(),
        document,
    )
    .unwrap()
}

fn with_workbench(test: impl FnOnce(&mut Workbench, &Path)) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(std::env::temp_dir().join(format!(
        "mhf-workbench-editing-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )));
    let root = directory.0.join("dat");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("value.bin");
    fs::write(&path, [0]).unwrap();
    let worker = Arc::new(Worker::start(root.clone(), directory.0.join("exports")).unwrap());
    let mut workbench = Workbench::new(
        Arc::new(Control::default()),
        worker,
        root.clone(),
        ViewSettings::default(),
        None,
    );
    workbench.set_redirect_paths(root, directory.0.join("redirect"));
    let document = document(&path, 0);
    workbench.path = Some(path.clone());
    workbench
        .editing
        .sessions
        .insert(path, Session::new(document.clone()));
    workbench.loaded_document(document);
    test(&mut workbench, &directory.0);
    // Workbench and its worker drop before the directory guard, including on
    // assertion failure. Accepted saves finish before their destination leaves.
}

fn submit_first_revision(workbench: &mut Workbench) {
    let path = workbench.path.clone().unwrap();
    let mut input = input(workbench.document.as_ref().unwrap());
    input.text = "1".into();
    input.change();
    workbench.editing.submitted = vec![(0, input.revision)];
    workbench.editing.inputs.insert(path, vec![input]);
    workbench.editing.busy = true;
    workbench.request = 7;
}

fn complete_first_revision(workbench: &mut Workbench) {
    let path = workbench.path.clone().unwrap();
    workbench.finish_edit(Expanded {
        request: 7,
        document: Ok(document(&path, 1)),
    });
}

#[test]
fn a_completed_revision_rebases_newer_text_without_consuming_it() {
    with_workbench(|workbench, _| {
        submit_first_revision(workbench);
        let path = workbench.path.clone().unwrap();
        let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[0];
        input.text = "2".into();
        input.change();
        complete_first_revision(workbench);
        let input = &workbench.editing.inputs[&path][0];
        assert!(input.pending);
        assert_eq!(input.text, "2");
        assert_eq!(input.original, [1]);
        let patch = input
            .binding
            .write(&workbench.document.as_ref().unwrap().buffers, &input.text)
            .unwrap()
            .unwrap();
        assert_eq!(patch.before, [1]);
        assert_eq!(patch.after, [2]);
    });
}

#[test]
fn packing_a_return_to_the_old_value_during_rebuild_keeps_that_new_input() {
    with_workbench(|workbench, directory| {
        submit_first_revision(workbench);
        let path = workbench.path.clone().unwrap();
        let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[0];
        input.text = "0".into();
        input.change();
        workbench.editing.pack_after_edits = true;
        workbench.flush_edits(&egui::Context::default());
        assert!(workbench.editing.inputs[&path][0].pending);
        complete_first_revision(workbench);
        let input = &workbench.editing.inputs[&path][0];
        assert!(input.pending);
        assert_eq!(input.text, "0");
        assert_eq!(input.original, [1]);
        Arc::get_mut(&mut workbench.worker).unwrap().stop();
        assert_eq!(fs::read(directory.join("redirect/value.bin")).unwrap(), [0]);
    });
}

#[test]
fn reverting_input_never_reuses_an_inflight_revision_number() {
    let document = document(Path::new("value.bin"), 0);
    let mut input = input(&document);
    input.text = "1".into();
    input.change();
    let submitted = input.revision;
    input.text = "invalid".into();
    input.change();
    input.reset(&document);
    input.text = "2".into();
    input.change();
    assert_ne!(input.revision, submitted);
    assert!(input.pending);
}

fn members(path: &Path, first_length: usize, second_length: usize) -> Arc<Document> {
    let mut document = crate::inspect::inspect(
        &path.to_string_lossy(),
        vec![0; first_length + second_length].into(),
    );
    let mut first = document.nodes[document.root].clone();
    first.name = "first".into();
    first.kind = Kind::Block;
    first.range = 0..first_length;
    first.fields.clear();
    let mut second = first.clone();
    second.name = "second".into();
    second.range = first_length..first_length + second_length;
    document.nodes[document.root].kind = Kind::Archive;
    document.nodes[document.root].children = vec![1, 2];
    document.nodes.extend([first, second]);
    Arc::new(document)
}

fn pending_raw(workbench: &mut Workbench, owner: usize, range: Range<usize>) {
    let path = workbench.path.clone().unwrap();
    let document = members(&path, 2, 2);
    workbench
        .editing
        .sessions
        .insert(path.clone(), Session::new(document.clone()));
    workbench.loaded_document(document.clone());
    workbench.node = owner;
    workbench.select_bytes(&document, 0, range);
    let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[0];
    input.text = "01".into();
    input.change();
    // Model a file replacement already being rebuilt when this raw edit was
    // typed. It has no submitted field input revision to acknowledge.
    workbench.editing.busy = true;
    workbench.request = 9;
}

#[test]
fn pending_raw_input_follows_its_member_when_an_earlier_member_grows() {
    with_workbench(|workbench, _| {
        pending_raw(workbench, 2, 2..3);
        let path = workbench.path.clone().unwrap();
        workbench.finish_edit(Expanded {
            request: 9,
            document: Ok(members(&path, 4, 2)),
        });
        let input = &workbench.editing.inputs[&path][0];
        assert!(input.pending);
        assert!(input.error.is_empty());
        assert_eq!(input.binding.range, 4..5);
        let patch = input
            .binding
            .write(&workbench.document.as_ref().unwrap().buffers, &input.text)
            .unwrap()
            .unwrap();
        assert_eq!(patch.binding.range, 4..5);
        assert_eq!(patch.after, [1]);
    });
}

#[test]
fn typing_again_cannot_reenable_a_raw_target_invalidated_by_replacement() {
    with_workbench(|workbench, _| {
        pending_raw(workbench, 1, 0..1);
        let path = workbench.path.clone().unwrap();
        workbench.finish_edit(Expanded {
            request: 9,
            document: Ok(members(&path, 4, 2)),
        });
        let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[0];
        assert!(!input.error.is_empty());
        input.text = "02".into();
        input.change();
        workbench.flush_edits(&egui::Context::default());
        assert!(
            !workbench.editing.busy,
            "an invalidated owner cannot submit another write"
        );
        assert!(!workbench.editing.inputs[&path][0].error.is_empty());
        assert_eq!(
            workbench.document.as_ref().unwrap().buffers[0].as_ref(),
            &[0; 6]
        );
    });
}

#[test]
fn two_field_rows_for_one_binding_share_the_pending_value() {
    with_workbench(|workbench, _| {
        let path = workbench.path.clone().unwrap();
        let mut document = (*workbench.document.as_ref().unwrap().as_ref()).clone();
        let mut alias = document.nodes[document.root].fields[0].clone();
        alias.name = "alternate label".into();
        document.nodes[document.root].fields.push(alias);
        let key = edit::node_key(&document, document.root).unwrap();
        let context = egui::Context::default();
        let output = context.run_ui(Default::default(), |ui| {
            let mut status = ui.new_child(egui::UiBuilder::new().id_salt("field-status"));
            workbench.field_row(
                ui,
                &mut status,
                &document,
                Some(&key),
                0,
                &document.nodes[document.root].fields[0],
            );
            let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[0];
            input.text = "7".into();
            input.change();
            workbench.field_row(
                ui,
                &mut status,
                &document,
                Some(&key),
                1,
                &document.nodes[document.root].fields[1],
            );
        });
        output.drop_without_applying_deltas();
        let inputs = &workbench.editing.inputs[&path];
        assert_eq!(
            inputs.len(),
            1,
            "aliases must not produce overlapping independent patches"
        );
        assert_eq!(inputs[0].text, "7");
        assert!(inputs[0].pending);
    });
}

#[test]
fn editing_again_does_not_clear_a_conflict_and_overwrite_other_field_changes() {
    with_workbench(|workbench, _| {
        let path = workbench.path.clone().unwrap();
        let image = |bytes: Vec<u8>| {
            Arc::new(crate::inspect::inspect(
                &path.to_string_lossy(),
                bytes.into(),
            ))
        };
        let original = image(vec![1, 2, 3, 4]);
        workbench.loaded_document(original.clone());
        workbench.select_bytes(&original, 0, 0..4);
        let index = workbench.editing.raw.unwrap();
        let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[index];
        input.text = "?A 02 03 04".into();
        input.change();
        workbench.request = 9;
        workbench.editing.busy = true;
        workbench.finish_edit(Expanded {
            request: 9,
            document: Ok(image(vec![1, 2, 3, 5])),
        });
        let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[index];
        assert!(input.conflicted);
        input.text = "AA 02 03 04".into();
        input.change();
        assert!(!input.error.is_empty());
        workbench.flush_edits(&egui::Context::default());
        assert!(!workbench.editing.busy);
        assert_eq!(
            &*workbench.document.as_ref().unwrap().buffers[0],
            &[1, 2, 3, 5]
        );
        let document = workbench.document.clone().unwrap();
        let input = &mut workbench.editing.inputs.get_mut(&path).unwrap()[index];
        input.reset(&document);
        assert!(!input.conflicted);
        assert_eq!(input.text, "01 02 03 05");
    });
}
