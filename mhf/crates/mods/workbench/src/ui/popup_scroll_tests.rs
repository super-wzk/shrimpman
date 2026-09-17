use super::Workbench;
use crate::{
    field::{Binding, Field, FieldType, ScalarType},
    inspect::{Document, Kind},
    preview::Control,
    settings::ViewSettings,
    worker::Worker,
};
use egui::{Event, Pos2, Rect, Shape};
use mhf_resource::binary::Endian;
use std::{fs, path::PathBuf, sync::Arc};

const TARGET: usize = 24;
const START_OFFSET: f32 = 600.0;

#[derive(Clone, Copy, Debug)]
enum Editor {
    Flags,
    Color,
    Scalar,
}

#[derive(Clone, Copy, Debug)]
enum Dismiss {
    Anchor,
    Value,
    FieldName,
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document(editor: Editor) -> Document {
    let mut bytes = Vec::new();
    let mut fields = Vec::new();
    for index in 0..64 {
        let start = bytes.len();
        let format = if index == TARGET {
            match editor {
                Editor::Flags => {
                    bytes.extend_from_slice(&0_u64.to_le_bytes());
                    FieldType::Flags(ScalarType::U64)
                }
                Editor::Color => {
                    bytes.extend_from_slice(&[255, 0, 0, 255]);
                    FieldType::Color { alpha: true }
                }
                Editor::Scalar => {
                    bytes.extend_from_slice(&123_u16.to_le_bytes());
                    FieldType::Scalar(ScalarType::U16)
                }
            }
        } else {
            bytes.extend_from_slice(&(10_000 + index as u64).to_le_bytes());
            FieldType::Scalar(ScalarType::U64)
        };
        let binding = Binding {
            buffer: 0,
            range: start..bytes.len(),
            format,
            endian: Endian::Little,
        };
        fields.push(Field {
            name: format!("scroll-field-{index}"),
            value: binding.decode(&bytes[binding.range.clone()]).unwrap(),
            note: None,
            binding,
            writable: true,
        });
    }
    let mut document = crate::inspect::inspect("scroll.bin", bytes.into());
    let node = &mut document.nodes[document.root];
    node.kind = Kind::Block;
    node.children.clear();
    node.error = None;
    node.fields = fields;
    document
}

struct Frame {
    offset: egui::Vec2,
    viewport: Rect,
    content_height: f32,
    texts: Vec<(String, Rect)>,
    red_swatch: Option<Rect>,
    popups: Vec<Rect>,
}

impl Frame {
    fn text(&self, label: &str) -> Pos2 {
        self.texts
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("missing visible {label:?}; offset {:?}", self.offset))
            .1
            .center()
    }

    fn anchor(&self, editor: Editor) -> Pos2 {
        match editor {
            Editor::Flags => self.text("位"),
            Editor::Color => self.red_swatch.expect("visible red color button").center(),
            Editor::Scalar => self.text("123"),
        }
    }

    fn unchanged(&self, previous: &Self, context: impl std::fmt::Debug) {
        assert!(
            (self.offset.y - previous.offset.y).abs() <= 1.0,
            "{context:?}: outer ScrollArea offset moved {:?} -> {:?}, viewport {:?}",
            previous.offset,
            self.offset,
            self.viewport
        );
        assert!((self.offset.x - previous.offset.x).abs() <= 1.0);
    }
}

struct Harness {
    context: egui::Context,
    workbench: Workbench,
    document: Document,
    time: f64,
    first_frame: bool,
    _directory: Directory,
}

impl Harness {
    fn new(editor: Editor, suffix: impl std::fmt::Debug) -> Self {
        let suffix = format!("{suffix:?}").replace('"', "");
        let directory = Directory(std::env::temp_dir().join(format!(
            "mhf-popup-scroll-{}-{editor:?}-{suffix}",
            std::process::id()
        )));
        fs::create_dir_all(&directory.0).unwrap();
        let worker =
            Arc::new(Worker::start(directory.0.clone(), directory.0.join("exports")).unwrap());
        let mut workbench = Workbench::new(
            Arc::new(Control::default()),
            worker,
            directory.0.clone(),
            ViewSettings::default(),
            None,
        );
        workbench.path = Some(directory.0.join("scroll.bin"));
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        context.all_styles_mut(|style| style.animation_time = 0.0);
        Self {
            context,
            workbench,
            document: document(editor),
            time: 0.0,
            first_frame: true,
            _directory: directory,
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Frame {
        self.time += 0.016;
        let mut scroll = None;
        let output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                egui_hunter::Density::Compact.scope(ui, |ui| {
                    ui.set_width(320.0);
                    let mut area = egui::ScrollArea::vertical()
                        .id_salt("inspector-scroll")
                        .max_height(360.0)
                        .auto_shrink([false, false]);
                    // Seed only the initial frame. Later frames must use persisted
                    // state, otherwise this test would hide an unexpected jump.
                    if self.first_frame {
                        area = area.vertical_scroll_offset(START_OFFSET);
                    }
                    let shown = area.show(ui, |ui| {
                        self.workbench.inspector(
                            ui,
                            &self.document,
                            &self.document.nodes[self.document.root],
                        );
                    });
                    scroll = Some((shown.id, shown.inner_rect, shown.content_size.y));
                });
            },
        );
        self.first_frame = false;
        let (id, viewport, content_height) = scroll.unwrap();
        let state = egui::scroll_area::State::load(&self.context, id)
            .expect("persisted outer ScrollArea state");
        let mut frame = Frame {
            offset: state.offset,
            viewport,
            content_height,
            texts: Vec::new(),
            red_swatch: None,
            popups: Vec::new(),
        };
        fn collect(shape: &Shape, clip: Rect, frame: &mut Frame) {
            match shape {
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, frame);
                    }
                }
                Shape::Text(text) => {
                    let rect = text
                        .galley
                        .rect
                        .translate(text.pos.to_vec2())
                        .intersect(clip);
                    if rect.is_positive() {
                        frame.texts.push((text.galley.text().into(), rect));
                    }
                }
                Shape::Rect(rect) if rect.fill == egui::Color32::RED => {
                    let visible = rect.rect.intersect(clip);
                    if visible.is_positive() && frame.red_swatch.is_none() {
                        frame.red_swatch = Some(visible);
                    }
                }
                _ => {}
            }
        }
        for clipped in &output.shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut frame);
        }
        output.drop_without_applying_deltas();
        frame.popups = self
            .context
            .memory(|memory| memory.areas().visible_layer_ids())
            .into_iter()
            .filter(|layer| layer.order == egui::Order::Foreground)
            .filter_map(|layer| {
                egui::AreaState::load(&self.context, layer.id).map(|state| state.rect())
            })
            .collect();
        frame
    }

    fn click(&mut self, position: Pos2) {
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
    }

    fn settle(&mut self) -> Frame {
        let mut frame = self.frame(vec![]);
        for _ in 0..10 {
            frame = self.frame(vec![]);
        }
        frame
    }
}

fn dismiss_without_scrolling(editor: Editor) {
    for dismiss in [Dismiss::Anchor, Dismiss::Value, Dismiss::FieldName] {
        let mut harness = Harness::new(editor, dismiss);
        let baseline = harness.settle();
        assert!((baseline.offset.y - START_OFFSET).abs() <= 1.0);
        assert!(
            baseline.offset.y > 100.0
                && baseline.offset.y + baseline.viewport.height() + 200.0 < baseline.content_height,
            "the test must start in the middle, not at a clamped edge"
        );
        let anchor = baseline.anchor(editor);
        assert!(baseline.viewport.contains(anchor));
        harness.click(anchor);
        let opened = harness.settle();
        opened.unchanged(&baseline, (editor, "open"));
        assert!(
            egui::Popup::is_any_open(&harness.context),
            "{editor:?} popup did not open"
        );
        let target = match dismiss {
            Dismiss::Anchor => anchor,
            Dismiss::Value => opened.text(&format!("{}", 10_000 + TARGET - 1)),
            // A later field executes after the popup closes in the draw loop,
            // so checking is_any_open only at each click site is insufficient.
            Dismiss::FieldName => opened.text(&format!("scroll-field-{}", TARGET + 1)),
        };
        assert!(opened.viewport.contains(target));
        assert!(
            opened.popups.iter().all(|rect| !rect.contains(target)),
            "dismiss target is inside the popup"
        );
        harness.click(target);
        for frame in 0..24 {
            harness
                .frame(vec![])
                .unchanged(&baseline, (editor, dismiss, frame));
        }
        assert!(!egui::Popup::is_any_open(&harness.context));
        assert!(
            harness.workbench.hex_selection.is_none(),
            "dismissal must not select bytes"
        );
    }
}

#[test]
fn closing_flags_popup_preserves_the_outer_inspector_scroll_offset() {
    dismiss_without_scrolling(Editor::Flags);
}

#[test]
fn closing_color_picker_preserves_the_outer_inspector_scroll_offset() {
    dismiss_without_scrolling(Editor::Color);
}

#[test]
fn clicking_a_field_name_without_a_popup_selects_its_bytes_where_the_panel_stands() {
    let mut harness = Harness::new(Editor::Flags, "normal-name-click");
    let before = harness.settle();
    harness.click(before.text(&format!("scroll-field-{}", TARGET + 1)));
    let after = harness.settle();
    assert!(harness.workbench.hex_selection.is_some());
    after.unchanged(&before, "deliberate field-name click");
}

#[test]
fn a_keyboard_focused_field_name_selects_its_bytes_without_moving_the_panel() {
    let mut harness = Harness::new(Editor::Flags, "keyboard-name-click");
    let before = harness.settle();
    let position = before.text(&format!("scroll-field-{}", TARGET + 1));
    harness.frame(vec![Event::PointerMoved(position)]);
    let hovered = harness
        .context
        .interaction_snapshot(|snapshot| snapshot.hovered.iter().copied().collect::<Vec<_>>());
    let response = hovered
        .into_iter()
        .filter_map(|id| harness.context.read_response(id))
        .filter(|response| response.sense.senses_click() && response.rect.contains(position))
        .min_by(|left, right| left.rect.area().total_cmp(&right.rect.area()))
        .expect("hovered field name");
    harness
        .context
        .memory_mut(|memory| memory.request_focus(response.id));
    harness.settle();
    assert!(!harness.context.text_edit_focused());
    harness.frame(vec![Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert!(harness.workbench.hex_selection.is_some());
    harness
        .settle()
        .unchanged(&before, "keyboard field-name activation");
}

#[test]
fn leaving_an_inline_editor_does_not_also_select_bytes() {
    for editor in [Editor::Flags, Editor::Scalar] {
        let mut harness = Harness::new(editor, "inline-focus");
        let before = harness.settle();
        let value = if matches!(editor, Editor::Scalar) {
            before.text("123")
        } else {
            before.text(&(10_000 + TARGET - 1).to_string())
        };
        harness.click(value);
        let focused = harness.settle();
        assert!(
            harness.context.text_edit_focused(),
            "the field must really be editing"
        );
        focused.unchanged(&before, (editor, "focused"));
        let label = focused.text(&format!("scroll-field-{}", TARGET + 1));
        harness.click(label);
        let unfocused = harness.settle();
        unfocused.unchanged(&before, (editor, "blurred"));
        assert!(harness.workbench.hex_selection.is_none());
        harness.click(label);
        assert!(
            harness.workbench.hex_selection.is_some(),
            "a subsequent deliberate label click still selects bytes"
        );
    }
}
