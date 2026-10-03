use super::*;
use crate::{preview::Control, settings::ViewSettings, worker::Worker};
use mhf_resource::binary::Reader;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

fn document(path: &Path, byte: u8) -> Arc<Document> {
    let bytes = [byte];
    let mut document = crate::inspect::inspect(path, bytes.to_vec().into());
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

fn native_sdt() -> Vec<u8> {
    let mut bytes = vec![0_u8; 144];
    bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&64_u32.to_le_bytes());
    bytes[30..32].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[108..110].copy_from_slice(&58_u16.to_le_bytes());
    bytes
}

#[test]
fn attack_resource_reference_uses_the_current_sdt_draft_and_keeps_the_numeric_editor() {
    with_workbench(|workbench, _| {
        use crate::field::FieldReference;
        use mhf_resource::action_definition::AttackReference;
        let mut bytes = vec![0u8; 176];
        for index in 0..2 {
            let at = index * 28;
            bytes[at..at + 2].copy_from_slice(&(index as u16).to_le_bytes());
            bytes[at + 4..at + 6].copy_from_slice(&2u16.to_le_bytes());
            bytes[at + 8..at + 12].copy_from_slice(&96u32.to_le_bytes());
        }
        bytes[58..60].copy_from_slice(&u16::MAX.to_le_bytes());
        let path = workbench.editing.source_root.join("mhfsdt.bin");
        let mut original = crate::inspect::inspect(&path, Arc::from(bytes.clone()));
        original.attack_directory = Some(Arc::new(original.parsed_attack_directory()));
        let mut current = workbench.document.as_ref().unwrap().as_ref().clone();
        current.attack_directory = original.attack_directory.clone();
        current.nodes[current.root].fields[0].reference =
            Some(FieldReference::Attack(AttackReference {
                category: 0,
                subtype: None,
                record: 1,
            }));
        let current = Arc::new(current);
        workbench
            .editing
            .sessions
            .get_mut(workbench.path.as_ref().unwrap())
            .unwrap()
            .document = current.clone();
        workbench.loaded_document(current);
        let context = egui::Context::default();
        let draw = |workbench: &mut Workbench| {
            let document = workbench.document.clone().unwrap();
            let output = crate::ui::tests::test_frame(
                &context,
                egui::vec2(600.0, 240.0),
                None,
                vec![],
                |ui| workbench.inspector_fields(ui, &document, &document.nodes[workbench.node]),
            );
            let texts = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            output.drop_without_applying_deltas();
            texts
        };
        assert!(
            draw(workbench)
                .iter()
                .any(|text| text == "mhfsdt.bin#0/attacks/1")
        );
        let mut session = Session::new(Arc::new(original));
        bytes[..2].copy_from_slice(&2u16.to_le_bytes());
        let mut draft = crate::inspect::inspect(&path, Arc::from(bytes));
        draft.attack_directory = Some(Arc::new(draft.parsed_attack_directory()));
        session.apply(Arc::new(draft));
        assert!(session.dirty());
        workbench.editing.sessions.insert(path, session);
        let texts = draw(workbench);
        assert!(texts.iter().any(|text| text == "mhfsdt.bin#1/attacks/1"));
        let input = &workbench.editing.inputs[workbench.path.as_ref().unwrap()][0];
        assert_eq!(input.binding.range, 0..1);
        assert_eq!(input.text, "0");
        assert!(
            workbench.control.commands().is_empty(),
            "showing an attack target must not load or run a resource"
        );
    });
}

#[test]
fn resource_path_enter_copy_and_field_click_keep_the_address_and_inspector_in_sync() {
    with_workbench(|workbench, _| {
        use egui::{Event, Key, Modifiers, Shape};
        let context = egui::Context::default();
        context.all_styles_mut(|style| style.animation_time = 0.0);
        let draw = |workbench: &mut Workbench, events: Vec<Event>, fields: bool| {
            let output = crate::ui::tests::test_frame(
                &context,
                egui::vec2(480.0, 240.0),
                None,
                events,
                |ui| {
                    if fields {
                        let document = workbench.document.clone().unwrap();
                        workbench.inspector_fields(ui, &document, &document.nodes[workbench.node]);
                    } else {
                        workbench.address_bar(ui);
                    }
                },
            );
            let labels = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.pos + text.galley.rect.center().to_vec2(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let commands = output.platform_output.commands.clone();
            output.drop_without_applying_deltas();
            (labels, commands)
        };
        workbench.address_input.clear();
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("workbench-resource-path")));
        draw(
            workbench,
            vec![Event::Text("value.bin#value".into())],
            false,
        );
        draw(
            workbench,
            vec![Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }],
            false,
        );
        assert_eq!(workbench.address_field, Some(0));
        assert_eq!(workbench.address_input, "value.bin#value");
        assert_eq!(workbench.hex_selection, Some(0..1));
        assert!(workbench.control.commands().is_empty());
        draw(workbench, Vec::new(), false);
        let copy = context
            .read_response(egui::Id::new("workbench-resource-path").with("copy"))
            .unwrap()
            .rect
            .center();
        let pointer = |position, pressed| {
            vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::default(),
                },
            ]
        };
        draw(workbench, pointer(copy, true), false);
        let (_, commands) = draw(workbench, pointer(copy, false), false);
        assert!(commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "value.bin#value")));
        workbench.address_field = None;
        workbench.sync_address();
        let (labels, _) = draw(workbench, Vec::new(), true);
        let label = crate::inspect::field_label(Kind::Unknown, "value");
        let field = labels
            .iter()
            .find(|(text, _)| text == label.as_ref())
            .unwrap()
            .1;
        draw(workbench, pointer(field, true), true);
        draw(workbench, pointer(field, false), true);
        assert_eq!(workbench.address_input, "value.bin#value");
        assert_eq!(workbench.address_field, Some(0));
        assert!(workbench.control.commands().is_empty());
    });
}

#[test]
fn resource_path_navigation_flushes_pending_inputs_and_retains_dirty_file_sessions() {
    with_workbench(|workbench, directory| {
        let root = directory.join("dat");
        let source = root.join("mhfsdt.bin");
        fs::write(&source, native_sdt()).unwrap();
        let (document, node, context, field) = crate::inspect::resource_path::tests::resolve(
            crate::inspect::inspect(&source, Arc::from(native_sdt())),
            &root,
            "mhfsdt.bin#0/attacks/1/power",
        );
        let field = field.unwrap();
        let document = Arc::new(document);
        workbench.path = Some(source.clone());
        workbench
            .editing
            .sessions
            .insert(source.clone(), Session::new(document.clone()));
        workbench.loaded_document(document.clone());
        workbench.select_source(crate::preview::ResourceRef::at_context(
            document.clone(),
            node,
            context,
        ));
        let value = &document.nodes[node].fields[field];
        let mut draft = Input::new(
            Target::Field {
                node: edit::node_key(&document, node).unwrap(),
                index: field,
                name: value.name.clone(),
            },
            value.binding.clone(),
            &document,
        )
        .unwrap();
        draft.text = "66".into();
        draft.change();
        workbench.editing.inputs.insert(source.clone(), vec![draft]);
        let other = root.join("other/mhfsdt.bin");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, native_sdt()).unwrap();
        workbench.address_input = "other/mhfsdt.bin#0/attacks/0/power".into();
        workbench.navigate_address();
        assert_eq!(workbench.path.as_ref(), Some(&source));
        assert_eq!(workbench.editing.next_path.as_ref(), Some(&other));
        assert!(workbench.editing.inputs[&source][0].pending);
        let context = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            workbench.poll();
            workbench.flush_edits(&context);
            if workbench.navigation.is_none()
                && workbench.path.as_ref() == Some(&other)
                && !workbench.loading
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(
            workbench.navigation.is_none(),
            "{} / {}",
            workbench.address_error,
            workbench.editing.error
        );
        assert_eq!(
            workbench.address_input,
            "other/mhfsdt.bin#0/attacks/0/power"
        );
        assert_eq!(workbench.hex_selection, Some(68..70));
        let session = &workbench.editing.sessions[&source];
        assert!(session.dirty());
        assert_eq!(
            &session.document.buffers[0][108..110],
            &66_u16.to_le_bytes()
        );
        assert_eq!(&fs::read(&source).unwrap()[108..110], &58_u16.to_le_bytes());
        assert!(
            workbench
                .catalog
                .entries
                .iter()
                .any(|entry| entry.path == other)
        );
        let output =
            crate::ui::tests::test_frame(&context, egui::vec2(500.0, 500.0), None, vec![], |ui| {
                workbench.resources(ui);
            });
        let revealed = output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains("记录 00000"))
        });
        output.drop_without_applying_deltas();
        assert!(
            revealed,
            "navigation must open the file folder and selected native record"
        );
        assert!(
            workbench
                .control
                .commands()
                .iter()
                .all(|command| !matches!(command, Command::LoadResource(_)))
        );
    });
}

#[test]
fn resource_path_navigation_does_not_apply_an_old_load_or_expansion_after_switching_files() {
    with_workbench(|workbench, directory| {
        let root = directory.join("dat");
        let first = root.join("mhfsdt.bin");
        let second = root.join("second.sdt");
        fs::write(&first, native_sdt()).unwrap();
        fs::write(&second, native_sdt()).unwrap();
        workbench.open_document(first.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while workbench.loading && Instant::now() < deadline {
            workbench.poll();
            std::thread::sleep(Duration::from_millis(3));
        }
        workbench.address_input = "mhfsdt.bin#0/attacks/1/power".into();
        workbench.navigate_address();
        assert!(workbench.expanding.is_some());
        let old_request = workbench.request;
        workbench.open_document(second.clone());
        assert!(workbench.request != old_request);
        assert!(workbench.navigation.is_none());
        while Instant::now() < deadline {
            workbench.poll();
            if !workbench.loading
                && workbench
                    .document
                    .as_ref()
                    .is_some_and(|document| document.source == second)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(3));
        }
        assert_eq!(workbench.document.as_ref().unwrap().source, second);
        assert_eq!(workbench.address_input, "second.sdt");
        assert_eq!(workbench.node, workbench.document.as_ref().unwrap().root);
        assert!(workbench.address_field.is_none());
        assert!(workbench.control.commands().is_empty());
    });
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
    let mut document = crate::inspect::inspect(path, vec![0; first_length + second_length].into());
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
        let image = |bytes: Vec<u8>| Arc::new(crate::inspect::inspect(&path, bytes.into()));
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

#[test]
fn focused_drafts_commit_on_blur_pack_or_document_switch() {
    for action in ["blur", "pack", "switch"] {
        with_workbench(|workbench, directory| {
            let context = egui::Context::default();
            let id = egui::Id::new("draft");
            context.memory_mut(|memory| memory.request_focus(id));
            let path = workbench.path.clone().unwrap();
            let mut draft = input(workbench.document.as_ref().unwrap());
            draft.text = "12".into();
            draft.change();
            draft.focused = Some(id);
            workbench.editing.inputs.insert(path.clone(), vec![draft]);
            match action {
                "blur" => {
                    workbench.flush_edits(&context);
                    assert!(!workbench.editing.busy);
                    assert!(workbench.editing.inputs[&path][0].pending);
                    assert!(workbench.editing.submitted.is_empty());
                    context.memory_mut(|memory| memory.surrender_focus(id));
                }
                "pack" => workbench.editing.pack_after_edits = true,
                "switch" => workbench.open_document(directory.join("other.bin")),
                _ => unreachable!(),
            }
            workbench.flush_edits(&context);
            if action == "pack" {
                Arc::get_mut(&mut workbench.worker).unwrap().stop();
                assert_eq!(
                    fs::read(directory.join("redirect/value.bin")).unwrap(),
                    [12]
                );
            } else {
                assert!(workbench.editing.busy);
                assert_eq!(workbench.editing.submitted.len(), 1);
                if action == "switch" {
                    assert_eq!(workbench.path.as_ref(), Some(&path));
                }
            }
        });
    }
}
