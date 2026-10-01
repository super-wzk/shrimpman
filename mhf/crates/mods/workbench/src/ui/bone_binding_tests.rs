use super::{
    Command, ResourceRef, resource_reference,
    tests::{pointer, preview_fixture},
};
use crate::{
    field::ReferenceCollection,
    preview::{Bone, LoadedSkeleton},
};
use egui::{Pos2, Rect, Shape};
use std::{cell::Cell, sync::Arc};

fn skeleton_source(path: &std::path::Path) -> ResourceRef {
    use mhf_resource::fskl::{BONE_HD, BONE_RECORD_SIZE, ROOT_INDICES, SKELETON};

    let count = 8;
    let record = 12 + BONE_RECORD_SIZE;
    let mut words = vec![
        SKELETON,
        count as u32 + 1,
        0,
        ROOT_INDICES,
        count as u32,
        (12 + count * 4) as u32,
    ];
    words.extend(0..count as u32);
    for index in 0..count {
        let start = words.len();
        words.extend([
            BONE_HD,
            1,
            record as u32,
            index as u32,
            u32::MAX,
            u32::MAX,
            u32::MAX,
        ]);
        words.resize(start + record / 4, 0);
        words[start + 7..start + 10].fill(1.0f32.to_bits());
    }
    words[2] = (words.len() * 4) as u32;
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    let document = Arc::new(crate::inspect::inspect(path, bytes.into()));
    assert!(document.nodes[document.root].error.is_none());
    ResourceRef::new(document.clone(), document.root)
}

fn caption(output: &egui::FullOutput, label: &str) -> Option<Pos2> {
    output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
}

fn select_sources(canonical: bool) {
    let mut workbench = preview_fixture();
    let path = if canonical {
        workbench.editing.source_root.join("bone-binding.fskl")
    } else {
        std::path::PathBuf::from("bone-binding.fskl")
    };
    let source = skeleton_source(&path);
    let resolved =
        resource_reference::indexed_source(&source, ReferenceCollection::SkeletonNodes, 5)
            .expect("the parsed FSKL exposes each indexed bone")
            .resource_address(&workbench.editing.source_root, None)
            .filter(|address| address.exact);
    assert_eq!(resolved.is_some(), canonical);
    let mut skeleton = LoadedSkeleton {
        id: 92,
        bones: Arc::new(
            (0..8)
                .map(|index| Bone {
                    index,
                    parent: None,
                    position: [0.0; 3],
                })
                .collect(),
        ),
        bone_bindings: Arc::new(vec![None; 8]),
        error: None,
    };
    let context = egui::Context::default();
    context.all_styles_mut(|style| style.animation_time = 0.0);
    let time = Cell::new(0.0);
    let parent = Cell::new(Rect::NOTHING);
    let draw = |workbench: &mut super::Workbench, skeleton: &LoadedSkeleton, events| {
        time.set(time.get() + 0.25);
        context.run_ui(
            egui::RawInput {
                time: Some(time.get()),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(380.0, 500.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let scroll = egui::ScrollArea::vertical()
                    .max_height(72.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        workbench.bone_binding_controls(ui, skeleton, &source, 0)
                    });
                parent.set(scroll.inner_rect);
            },
        )
    };
    for wanted in [Some(5), Some(2), None] {
        let output = draw(&mut workbench, &skeleton, vec![]);
        let selected = if skeleton.bone_bindings[0].is_some() {
            "跟随来源节点"
        } else {
            "原始骨架姿态"
        };
        let anchor = caption(&output, selected).expect("binding chooser is visible");
        output.drop_without_applying_deltas();
        for pressed in [true, false] {
            draw(&mut workbench, &skeleton, pointer(anchor, pressed))
                .drop_without_applying_deltas();
        }
        let output = draw(&mut workbench, &skeleton, vec![]);
        assert!(
            caption(&output, "节点 0").is_none(),
            "the target cannot select itself"
        );
        let label = wanted.map_or_else(
            || "原始骨架姿态".to_owned(),
            |index| format!("节点 {index}"),
        );
        let mut option = caption(&output, &label).expect("the source option is visible");
        if wanted == Some(5) {
            assert!(
                option.y > parent.get().bottom(),
                "exercise popup outside the parent scroll area"
            );
            // Select the first source through row space, then the next by text.
            option.x = parent.get().center().x;
        }
        output.drop_without_applying_deltas();
        for pressed in [true, false] {
            draw(&mut workbench, &skeleton, pointer(option, pressed))
                .drop_without_applying_deltas();
        }
        assert!(
            matches!(workbench.control.commands().as_slice(), [Command::BoneBinding { skeleton: 92, node: 0, source }] if *source == wanted),
            "canonical={canonical}, source={wanted:?}: clicking the menu row must enqueue its binding"
        );
        Arc::make_mut(&mut skeleton.bone_bindings)[0] = wanted;
    }
}

#[test]
fn source_rows_without_canonical_paths_select_and_restore_original_pose() {
    select_sources(false);
}

#[test]
fn source_rows_with_canonical_paths_select_and_restore_original_pose() {
    select_sources(true);
}
