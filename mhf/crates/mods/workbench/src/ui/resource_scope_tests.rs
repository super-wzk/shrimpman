use super::*;
use crate::{inspect::Node, metadata::ModelResources, preview::AssetBundle};
use egui::{Event, Pos2, Rect, Shape};

#[derive(Debug, PartialEq, Eq)]
struct Tag(u8);

fn document(extra_nodes: usize) -> Arc<Document> {
    let node = |name: &str, kind, children| Node {
        name: name.into(),
        kind,
        buffer: 0,
        range: 0..16,
        fields: Vec::new(),
        metadata: Default::default(),
        children,
        action: None,
        deferred: false,
        error: None,
    };
    let mut document = Document {
        root: 0,
        buffers: vec![Arc::from([0_u8; 16])],
        nodes: vec![
            node("scopes.bin", Kind::Archive, vec![1, 2]),
            node("owner", Kind::Archive, vec![3]),
            node("caller", Kind::Archive, vec![4]),
            node("payload", Kind::Archive, vec![5, 7, 8]),
            node("reference", Kind::StageResourceReference, vec![3]),
            node("encoded-model", Kind::Ecd, vec![6]),
            node("model", Kind::Fmod, vec![]),
            node("skeleton", Kind::Fskl, vec![]),
            node("textures", Kind::Txb, vec![9]),
            node("image", Kind::Dds, vec![]),
        ],
    };
    document.nodes[1].metadata.insert(Tag(7));
    document.nodes[2].metadata.insert(Tag(9));
    document.nodes[6].metadata.insert(ModelResources {
        skeleton: Some(7 + extra_nodes),
        textures: vec![8 + extra_nodes],
    });
    if extra_nodes != 0 {
        for node in &mut document.nodes {
            for child in &mut node.children {
                *child += extra_nodes;
            }
        }
        document.nodes.splice(
            1..1,
            (0..extra_nodes).map(|_| node("extra", Kind::Empty, vec![])),
        );
    }
    Arc::new(document)
}

struct Frame(Vec<(String, Rect)>);

impl Frame {
    fn label(&self, prefix: &str) -> Pos2 {
        self.0
            .iter()
            .find(|(text, _)| text.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing {prefix:?} in {:?}", self.0))
            .1
            .center()
    }

    fn load_button(&self, prefix: &str) -> Pos2 {
        let row = self.label(prefix).y;
        self.0
            .iter()
            .find(|(text, rect)| text == "加载" && (rect.center().y - row).abs() < 4.0)
            .expect("load button beside the selected row")
            .1
            .center()
    }
}

struct Harness {
    workbench: Workbench,
    context: egui::Context,
    time: f64,
}

impl Harness {
    fn new(show_encoding_layers: bool) -> Self {
        let root = std::env::temp_dir();
        let worker = Arc::new(Worker::start(root.clone(), root.join("exports")).unwrap());
        let mut workbench = Workbench::new(
            Arc::new(Control::default()),
            worker,
            root,
            ViewSettings {
                show_encoding_layers,
                ..ViewSettings::default()
            },
            None,
        );
        workbench.loaded_document(document(0));
        let context = egui::Context::default();
        context.all_styles_mut(|style| style.animation_time = 0.0);
        Self {
            workbench,
            context,
            time: 0.0,
        }
    }

    fn frame(&mut self, inspector: bool, events: Vec<Event>) -> Frame {
        self.time += 0.1;
        let mut load = None;
        let mut details = None;
        let output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 720.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                let document = self.workbench.document.as_ref().unwrap().clone();
                if inspector {
                    self.workbench.resource_actions(ui, &document);
                } else {
                    tree(
                        ui,
                        &ResourceRef::new(document.clone(), document.root),
                        &self.workbench.resource_counts,
                        self.workbench.view.show_encoding_layers,
                        &mut self.workbench.node,
                        &mut self.workbench.selection,
                        &mut load,
                        &mut details,
                    );
                }
            },
        );
        fn texts(shape: &Shape, output: &mut Vec<(String, Rect)>) {
            match shape {
                Shape::Text(text) => output.push((
                    text.galley.text().to_owned(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, output);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut labels);
        }
        output.drop_without_applying_deltas();
        if let Some(source) = load {
            self.workbench.load_source(source);
        }
        Frame(labels)
    }

    fn click(&mut self, inspector: bool, position: Pos2) {
        for pressed in [true, false] {
            self.frame(
                inspector,
                vec![
                    Event::PointerMoved(position),
                    Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    fn open_caller_model(&mut self) -> Frame {
        for label in ["caller ·", "reference ·", "payload ·"] {
            let position = self.frame(false, vec![]).label(label);
            self.click(false, position);
        }
        if self.workbench.view.show_encoding_layers {
            let position = self.frame(false, vec![]).label("encoded-model ·");
            self.click(false, position);
        }
        self.frame(false, vec![])
    }

    fn loaded(&self) -> ResourceRef {
        let mut commands = self.workbench.control.commands();
        assert_eq!(commands.len(), 1);
        let Command::LoadResource(source) = commands.pop().unwrap() else {
            panic!("tree operation must dispatch LoadResource")
        };
        source
    }
}

#[test]
fn reference_child_buttons_load_the_same_context_as_the_parent_directory() {
    for layers in [false, true] {
        let mut harness = Harness::new(layers);
        let frame = harness.open_caller_model();
        let model_label = if layers {
            "model ·"
        } else {
            "encoded-model ·"
        };
        harness.click(false, frame.load_button(model_label));
        let source = harness.loaded();
        assert_eq!(source.node, 6);
        assert_eq!(source.scope().get::<Tag>().unwrap().value, &Tag(9));
        let document = harness.workbench.document.as_ref().unwrap();
        let grouped = ResourceRef::new(document.clone(), 2).loadable_resources();
        assert!(source.same_instance(&grouped[0]));
        assert!(!source.same_instance(&ResourceRef::new(document.clone(), 6)));
        let named = AssetBundle::find_with_nodes(document.clone()).0;
        let bundle = AssetBundle::from_source(source, &named);
        assert!(bundle.skeleton.unwrap().same_instance(&grouped[1]));
        assert!(bundle.textures[0].texture_images()[0].same_instance(&grouped[2]));
    }
}

#[test]
fn inspector_loading_and_document_refresh_retain_the_selected_reference_child() {
    let mut harness = Harness::new(false);
    let frame = harness.open_caller_model();
    harness.click(false, frame.label("encoded-model ·"));
    assert!(harness.workbench.control.commands().is_empty());
    for extra_nodes in [0, 3] {
        if extra_nodes != 0 {
            harness.workbench.refresh_document(document(extra_nodes));
        }
        let button = harness.frame(true, vec![]).label("加载");
        harness.click(true, button);
        let source = harness.loaded();
        assert_eq!(source.node, 6 + extra_nodes);
        assert_eq!(source.scope().get::<Tag>().unwrap().value, &Tag(9));
    }
    harness.workbench.loaded_document(document(0));
    harness.workbench.load_node(6);
    assert_eq!(
        harness.loaded().scope().get::<Tag>().unwrap().value,
        &Tag(7)
    );
}

#[test]
fn reference_selection_survives_resource_type_changes_and_inspection_errors() {
    let mut harness = Harness::new(false);
    let frame = harness.open_caller_model();
    harness.click(false, frame.label("encoded-model ·"));
    for error in [None, Some("edited image is incomplete".into())] {
        let mut updated = (*document(3)).clone();
        updated.nodes[9].kind = Kind::Dds;
        updated.nodes[9].error = error;
        harness.workbench.refresh_document(Arc::new(updated));
        let source = harness.workbench.selected_source().unwrap();
        assert_eq!(source.node, 9);
        assert_eq!(source.document.nodes[source.node].kind, Kind::Dds);
        assert_eq!(source.scope().get::<Tag>().unwrap().value, &Tag(9));
        assert!(harness.workbench.control.commands().is_empty());
    }
}

#[test]
fn shared_package_rows_keep_independent_expansion_state_in_each_branch() {
    let mut harness = Harness::new(false);
    harness.open_caller_model();
    let frame = harness.frame(false, vec![]);
    harness.click(false, frame.label("owner ·"));
    let frame = harness.frame(false, vec![]);
    assert_eq!(
        frame
            .0
            .iter()
            .filter(|(text, _)| text.starts_with("encoded-model ·"))
            .count(),
        1
    );
    harness.click(false, frame.label("payload ·"));
    let frame = harness.frame(false, vec![]);
    assert_eq!(
        frame
            .0
            .iter()
            .filter(|(text, _)| text.starts_with("encoded-model ·"))
            .count(),
        2
    );
    let caller_package = frame
        .0
        .iter()
        .filter(|(text, _)| text.starts_with("payload ·"))
        .nth(1)
        .unwrap()
        .1
        .center();
    harness.click(false, caller_package);
    let frame = harness.frame(false, vec![]);
    assert_eq!(
        frame
            .0
            .iter()
            .filter(|(text, _)| text.starts_with("encoded-model ·"))
            .count(),
        1
    );
}
