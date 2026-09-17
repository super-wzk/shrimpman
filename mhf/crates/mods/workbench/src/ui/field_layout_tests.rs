use super::fields;
use crate::field::{Binding, FieldType, ScalarType, TextEncoding};
use egui::{Event, Pos2, Rect, Shape};
use mhf_resource::binary::Endian;

struct Case {
    name: &'static str,
    binding: Binding,
    original: Vec<u8>,
    text: String,
    focused: Option<egui::Id>,
}

impl Case {
    fn new(name: &'static str, format: FieldType, original: Vec<u8>) -> Self {
        let binding = Binding {
            buffer: 0,
            range: 0..original.len(),
            format,
            endian: Endian::Little,
        };
        let text = binding.decode(&original).unwrap();
        Self {
            name,
            binding,
            original,
            text,
            focused: None,
        }
    }
}

fn cases() -> Vec<Case> {
    let mut numeric = Case::new(
        "long numeric input",
        FieldType::Scalar(ScalarType::U64),
        vec![0; 8],
    );
    numeric.text = "9".repeat(512);
    vec![
        numeric,
        Case::new("flags", FieldType::Flags(ScalarType::U64), vec![0; 8]),
        Case::new(
            "color",
            FieldType::Color { alpha: true },
            vec![10, 20, 30, 255],
        ),
        Case::new("three raw bytes", FieldType::Bytes, vec![0x41, 0x7f, 0xff]),
        Case::new(
            "short vector",
            FieldType::Array(ScalarType::F32),
            [-1_234_567.3_f32; 3]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        ),
        Case::new(
            "multiline text",
            FieldType::Text {
                encoding: TextEncoding::Utf8,
                terminated: false,
            },
            format!(
                "{}\n{}",
                "wide resource name ".repeat(30),
                "second line ".repeat(30)
            )
            .into_bytes(),
        ),
        Case::new("large raw", FieldType::Bytes, vec![0xa5; 256]),
        Case::new(
            "large array",
            FieldType::Array(ScalarType::U32),
            (0..128_u32).flat_map(u32::to_le_bytes).collect(),
        ),
    ]
}

struct Frame {
    main: Rect,
    available: Rect,
    changed: bool,
    popups: Vec<Rect>,
    modal: Option<Rect>,
    output: Option<egui::FullOutput>,
}

impl Frame {
    fn texts(&self) -> Vec<(String, Rect, bool)> {
        fn collect(shape: &Shape, clip: Rect, result: &mut Vec<(String, Rect, bool)>) {
            match shape {
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, result);
                    }
                }
                Shape::Text(text) => {
                    let rect = text
                        .galley
                        .rect
                        .translate(text.pos.to_vec2())
                        .intersect(clip);
                    if rect.is_positive() {
                        result.push((text.galley.text().to_owned(), rect, text.galley.elided));
                    }
                }
                _ => {}
            }
        }
        let mut result = Vec::new();
        for shape in &self.output.as_ref().unwrap().shapes {
            collect(&shape.shape, shape.clip_rect, &mut result);
        }
        result
    }

    fn text_center(&self, label: &str) -> Pos2 {
        self.texts()
            .into_iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("missing visible control {label:?}"))
            .1
            .center()
    }

    fn fits(&self, label: &str) {
        assert!(
            self.main.right() <= self.available.right() + 1.0,
            "{label} expanded the row horizontally: {:?}",
            self.main
        );
        assert!(
            self.main.bottom() <= self.available.bottom() + 1.0,
            "{label} expanded the row vertically: {:?}",
            self.main
        );
    }

    fn same_main_as(&self, previous: &Self) {
        assert!((self.main.width() - previous.main.width()).abs() <= 1.0);
        assert!((self.main.height() - previous.main.height()).abs() <= 1.0);
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if let Some(output) = self.output.take() {
            output.drop_without_applying_deltas();
        }
    }
}

struct Harness {
    context: egui::Context,
    width: f32,
    height: f32,
    screen: egui::Vec2,
    time: f64,
}

impl Harness {
    fn new(width: f32, height: f32) -> Self {
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        context.all_styles_mut(|style| style.animation_time = 0.0);
        Self {
            context,
            width,
            height,
            screen: egui::vec2(900.0, 700.0),
            time: 0.0,
        }
    }

    fn frame(&mut self, events: Vec<Event>, mut draw: impl FnMut(&mut egui::Ui) -> bool) -> Frame {
        self.time += 0.016;
        let mut main = Rect::NOTHING;
        let mut available = Rect::NOTHING;
        let mut changed = false;
        let output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen)),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                egui_hunter::Density::Compact.scope(ui, |ui| {
                    available =
                        Rect::from_min_size(ui.cursor().min, egui::vec2(self.width, self.height));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(available), |ui| {
                        ui.set_width(self.width);
                        changed |= draw(ui);
                        main = ui.min_rect();
                    });
                });
            },
        );
        let layers = self
            .context
            .memory(|memory| memory.areas().visible_layer_ids());
        let popups = layers
            .into_iter()
            .filter(|layer| layer.order == egui::Order::Foreground)
            .filter_map(|layer| {
                egui::AreaState::load(&self.context, layer.id).map(|state| state.rect())
            })
            .collect();
        let modal = self
            .context
            .memory(|memory| memory.top_modal_layer())
            .and_then(|layer| egui::AreaState::load(&self.context, layer.id))
            .map(|state| state.rect());
        Frame {
            main,
            available,
            changed,
            popups,
            modal,
            output: Some(output),
        }
    }

    fn input(&mut self, case: &mut Case, events: Vec<Event>) -> Frame {
        self.frame(events, |ui| {
            fields::input(
                ui,
                &case.binding,
                &case.original,
                &mut case.text,
                &mut case.focused,
            )
        })
    }
}

fn pointer(position: Pos2, pressed: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(position),
        Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn open_popup(harness: &mut Harness, case: &mut Case, label: &str) -> (Frame, Frame) {
    let mut closed = harness.input(case, vec![]);
    for _ in 0..3 {
        closed = harness.input(case, vec![]);
    }
    closed.fits(case.name);
    let anchor = closed.text_center(label);
    assert!(
        closed.available.contains(anchor),
        "popup control must be reachable inside its row"
    );
    harness.input(case, pointer(anchor, true));
    harness.input(case, pointer(anchor, false));
    let mut opened = harness.input(case, vec![]);
    for _ in 0..3 {
        opened = harness.input(case, vec![]);
    }
    assert!(egui::Popup::is_any_open(&harness.context));
    opened.fits(case.name);
    opened.same_main_as(&closed);
    assert!(
        !opened.popups.is_empty(),
        "the actual popup Area must be visible"
    );
    for popup in &opened.popups {
        assert!(
            popup.width() <= 321.0,
            "popup width includes its frame: {popup:?}"
        );
        assert!(
            popup.height() <= 241.0,
            "popup height includes its frame: {popup:?}"
        );
    }
    (closed, opened)
}

#[test]
fn every_field_control_keeps_a_single_row_at_narrow_and_wide_panel_sizes() {
    for width in [220.0, 320.0, 500.0] {
        for mut case in cases() {
            let mut harness = Harness::new(width, 30.0);
            for _ in 0..3 {
                harness.input(&mut case, vec![]).fits(case.name);
            }
        }
    }
}

#[test]
fn flags_popup_stays_bounded_and_open_while_multiple_bits_change() {
    for width in [220.0, 320.0, 500.0] {
        let mut case = cases()
            .into_iter()
            .find(|case| case.name == "flags")
            .unwrap();
        let mut harness = Harness::new(width, 30.0);
        let (closed, mut popup) = open_popup(&mut harness, &mut case, "位");
        for bit in ["0", "1"] {
            let target = popup.text_center(bit);
            let pressed = harness.input(&mut case, pointer(target, true));
            let released = harness.input(&mut case, pointer(target, false));
            assert!(
                pressed.changed || released.changed,
                "bit {bit} did not report an edit"
            );
            assert!(
                egui::Popup::is_any_open(&harness.context),
                "editing one bit must keep the other bits available"
            );
            popup = harness.input(&mut case, vec![]);
            popup.same_main_as(&closed);
        }
        let encoded = case.binding.encode(&case.original, &case.text).unwrap();
        assert_eq!(u64::from_le_bytes(encoded.try_into().unwrap()), 3);
    }
}

#[test]
fn large_array_and_text_popups_do_not_resize_the_inspector_row() {
    for width in [220.0, 320.0, 500.0] {
        for (name, button, title) in [
            ("large array", "展开", "数组元素"),
            ("multiline text", "编辑", "文本内容"),
        ] {
            let mut case = cases().into_iter().find(|case| case.name == name).unwrap();
            let mut harness = Harness::new(width, 30.0);
            let (_, opened) = open_popup(&mut harness, &mut case, button);
            opened.text_center(title);
        }
    }
}

fn binary_frame(
    harness: &mut Harness,
    case: &mut Case,
    events: Vec<Event>,
    background_clicked: &mut bool,
) -> Frame {
    let background = Rect::from_min_size(
        egui::pos2(12.0, harness.screen.y - 40.0),
        egui::vec2(140.0, 24.0),
    );
    harness.frame(events, |ui| {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("background")
                .max_rect(background),
        );
        *background_clicked |= child
            .add_sized(background.size(), egui::Button::new("background button"))
            .clicked();
        fields::input(
            ui,
            &case.binding,
            &case.original,
            &mut case.text,
            &mut case.focused,
        )
    })
}

fn open_binary(
    harness: &mut Harness,
    case: &mut Case,
    background_clicked: &mut bool,
) -> (Frame, Frame) {
    let mut closed = binary_frame(harness, case, vec![], background_clicked);
    for _ in 0..3 {
        closed = binary_frame(harness, case, vec![], background_clicked);
    }
    let anchor = closed.text_center("编辑");
    assert!(closed.available.contains(anchor));
    for pressed in [true, false] {
        binary_frame(harness, case, pointer(anchor, pressed), background_clicked);
    }
    let mut opened = binary_frame(harness, case, vec![], background_clicked);
    for _ in 0..3 {
        opened = binary_frame(harness, case, vec![], background_clicked);
    }
    opened.fits(case.name);
    opened.same_main_as(&closed);
    opened.text_center("二进制编辑");
    let modal = opened
        .modal
        .expect("the editor must be a modal, not an inline expansion");
    assert!(
        modal.width() <= 700.0_f32.min(harness.screen.x * 0.9) + 1.0,
        "modal width: {modal:?}"
    );
    assert!(
        modal.height() <= 480.0_f32.min(harness.screen.y * 0.9) + 1.0,
        "modal height: {modal:?}"
    );
    (closed, opened)
}

fn replace_text(text: &str) -> Vec<Event> {
    vec![
        Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
        Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
        Event::Text(text.into()),
    ]
}

#[test]
fn two_and_three_byte_fields_edit_in_a_modal_without_apply_or_background_clicks() {
    for width in [220.0, 320.0, 500.0] {
        for count in [2, 3] {
            let mut case = Case::new(
                "short bytes",
                FieldType::Bytes,
                vec![0x41, 0x42, 0x43][..count].to_vec(),
            );
            let mut harness = Harness::new(width, 30.0);
            let mut background_clicked = false;
            let (closed, opened) = open_binary(&mut harness, &mut case, &mut background_clicked);
            let byte = opened.text_center("41");
            for pressed in [true, false] {
                binary_frame(
                    &mut harness,
                    &mut case,
                    pointer(byte, pressed),
                    &mut background_clicked,
                );
            }
            let before = case.original.clone();
            let invalid = binary_frame(
                &mut harness,
                &mut case,
                replace_text("GG"),
                &mut background_clicked,
            );
            assert!(
                invalid.changed,
                "the coordinator must learn that an invalid draft exists"
            );
            assert!(case.binding.encode(&case.original, &case.text).is_err());
            assert_eq!(case.original, before);
            invalid.text_center("GG");
            let valid = binary_frame(
                &mut harness,
                &mut case,
                replace_text("7F"),
                &mut background_clicked,
            );
            assert!(
                valid.changed,
                "valid hex must update without an Apply button"
            );
            let encoded = case.binding.encode(&case.original, &case.text).unwrap();
            assert_eq!(encoded[0], 0x7f);
            assert_eq!(&encoded[1..], &case.original[1..]);
            case.original = encoded.clone();
            let current = binary_frame(&mut harness, &mut case, vec![], &mut background_clicked);
            current.same_main_as(&closed);
            let close = current.text_center("关闭");
            for pressed in [true, false] {
                binary_frame(
                    &mut harness,
                    &mut case,
                    pointer(close, pressed),
                    &mut background_clicked,
                );
            }
            binary_frame(&mut harness, &mut case, vec![], &mut background_clicked);
            let dismissed = binary_frame(&mut harness, &mut case, vec![], &mut background_clicked);
            assert!(dismissed.modal.is_none());
            assert_eq!(
                case.binding.encode(&case.original, &case.text).unwrap(),
                encoded
            );
            let (_, reopened) = open_binary(&mut harness, &mut case, &mut background_clicked);
            reopened.text_center("7F");
            let background = reopened.text_center("background button");
            for pressed in [true, false] {
                binary_frame(
                    &mut harness,
                    &mut case,
                    pointer(background, pressed),
                    &mut background_clicked,
                );
            }
            assert!(
                !background_clicked,
                "the modal backdrop must block the underlying UI"
            );
        }
    }
}

#[test]
fn large_binary_modal_fits_normal_and_small_screens_without_sizing_the_row() {
    for screen in [egui::vec2(900.0, 700.0), egui::vec2(420.0, 300.0)] {
        let mut harness = Harness::new(220.0, 30.0);
        harness.screen = screen;
        let mut case = Case::new("large binary", FieldType::Bytes, vec![0x41; 256]);
        let mut background_clicked = false;
        let (_, opened) = open_binary(&mut harness, &mut case, &mut background_clicked);
        let texts = opened.texts();
        assert!(
            texts.iter().any(|(text, _, _)| text.contains("ASCII")),
            "ASCII column is visible"
        );
        assert!(
            texts.iter().any(|(text, _, _)| text.contains("00000000")),
            "offset column is visible"
        );
        opened.text_center("41");
    }
}

#[test]
fn clicking_a_byte_selects_its_value_and_escape_cancels_only_the_invalid_cell() {
    let mut harness = Harness::new(220.0, 30.0);
    let mut case = Case::new("short bytes", FieldType::Bytes, vec![0x41, 0x42]);
    let mut background_clicked = false;
    let (_, mut shown) = open_binary(&mut harness, &mut case, &mut background_clicked);
    for draft in ["G", "GG"] {
        let byte = shown.text_center("41");
        for pressed in [true, false] {
            binary_frame(
                &mut harness,
                &mut case,
                pointer(byte, pressed),
                &mut background_clicked,
            );
        }
        // The click itself selects the existing pair; no Ctrl+A is involved.
        let invalid = binary_frame(
            &mut harness,
            &mut case,
            vec![Event::Text(draft.into())],
            &mut background_clicked,
        );
        assert!(invalid.changed);
        invalid.text_center(draft);
        assert!(case.binding.encode(&case.original, &case.text).is_err());
        let escaped = binary_frame(
            &mut harness,
            &mut case,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut background_clicked,
        );
        assert!(
            escaped.changed,
            "cancelling an invalid draft must notify the coordinator"
        );
        shown = binary_frame(&mut harness, &mut case, vec![], &mut background_clicked);
        assert!(
            shown.modal.is_some(),
            "the first Escape belongs to the invalid cell"
        );
        shown.text_center("41");
        assert_eq!(
            case.binding.encode(&case.original, &case.text).unwrap(),
            case.original
        );
    }
}

fn type_byte(
    harness: &mut Harness,
    case: &mut Case,
    frame: &Frame,
    value: &str,
    replacement: &str,
    background_clicked: &mut bool,
) -> Frame {
    let point = frame.text_center(value);
    for pressed in [true, false] {
        binary_frame(harness, case, pointer(point, pressed), background_clicked);
    }
    binary_frame(
        harness,
        case,
        vec![Event::Text(replacement.into())],
        background_clicked,
    )
}

#[test]
fn cancelling_one_invalid_cell_keeps_other_cells_latest_complete_values() {
    let mut harness = Harness::new(220.0, 30.0);
    let mut case = Case::new("short bytes", FieldType::Bytes, vec![0x41, 0x42]);
    let mut background_clicked = false;
    let (_, opened) = open_binary(&mut harness, &mut case, &mut background_clicked);
    let first = type_byte(
        &mut harness,
        &mut case,
        &opened,
        "41",
        "G",
        &mut background_clicked,
    );
    let second = type_byte(
        &mut harness,
        &mut case,
        &first,
        "42",
        "FF",
        &mut background_clicked,
    );
    assert!(case.binding.encode(&case.original, &case.text).is_err());
    type_byte(
        &mut harness,
        &mut case,
        &second,
        "FF",
        "F",
        &mut background_clicked,
    );
    binary_frame(
        &mut harness,
        &mut case,
        vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        &mut background_clicked,
    );
    let resumed = binary_frame(&mut harness, &mut case, vec![], &mut background_clicked);
    assert!(resumed.modal.is_some());
    resumed.text_center("FF");
    assert!(
        case.binding.encode(&case.original, &case.text).is_err(),
        "the first cell is still incomplete"
    );
    type_byte(
        &mut harness,
        &mut case,
        &resumed,
        "G",
        "41",
        &mut background_clicked,
    );
    assert_eq!(
        case.binding.encode(&case.original, &case.text).unwrap(),
        [0x41, 0xff]
    );
}

#[derive(Debug)]
struct TextPosition {
    text: String,
    rect: Rect,
    left: f32,
    baseline: f32,
}

fn text_positions(frame: &Frame) -> Vec<TextPosition> {
    fn collect(shape: &Shape, clip: Rect, output: &mut Vec<TextPosition>) {
        match shape {
            Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, output);
                }
            }
            Shape::Text(text) => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                if rect.intersects(clip)
                    && let Some(row) = text.galley.rows.first()
                    && let Some(glyph) = row.glyphs.first()
                {
                    output.push(TextPosition {
                        text: text.galley.text().into(),
                        rect,
                        left: text.pos.x + row.pos.x + glyph.pos.x,
                        baseline: text.pos.y + row.pos.y + glyph.pos.y,
                    });
                }
            }
            _ => {}
        }
    }
    let mut positions = Vec::new();
    for shape in &frame.output.as_ref().unwrap().shapes {
        collect(&shape.shape, shape.clip_rect, &mut positions);
    }
    positions
}

#[test]
fn inline_text_stays_vertically_centered_before_and_after_focus_and_typing() {
    for (mut case, replacement) in [
        (
            Case::new("flags", FieldType::Flags(ScalarType::U32), vec![0; 4]),
            "0x1",
        ),
        (
            Case::new(
                "color",
                FieldType::Color { alpha: true },
                vec![10, 20, 30, 255],
            ),
            "1, 2, 3, 255",
        ),
        (
            Case::new(
                "integer",
                FieldType::Scalar(ScalarType::U64),
                123_u64.to_le_bytes().to_vec(),
            ),
            "456",
        ),
        (
            Case::new(
                "text",
                FieldType::Text {
                    encoding: TextEncoding::Utf8,
                    terminated: false,
                },
                b"hello text".to_vec(),
            ),
            "world text",
        ),
    ] {
        let mut harness = Harness::new(260.0, 30.0);
        mhf_font::install(&harness.context);
        let check = |frame: &Frame, value: &str| {
            let positions = text_positions(frame);
            let text = positions
                .iter()
                .find(|position| position.text == value)
                .expect("visible inline text");
            assert!(
                (text.rect.center().y - frame.main.center().y).abs() <= 1.0,
                "{value:?}: text={:?}, control={:?}",
                text.rect,
                frame.main
            );
        };
        let idle = harness.input(&mut case, vec![]);
        check(&idle, &case.text);
        let point = idle.text_center(&case.text);
        harness.input(&mut case, pointer(point, true));
        let focused = harness.input(&mut case, pointer(point, false));
        check(&focused, &case.text);
        let edited = harness.input(&mut case, replace_text(replacement));
        assert!(edited.changed);
        check(&edited, &case.text);
    }
}

#[test]
fn binary_header_and_rows_share_column_origins_and_text_baselines() {
    for (count, screen) in [
        (2, egui::vec2(900.0, 700.0)),
        (7, egui::vec2(420.0, 300.0)),
        (32, egui::vec2(900.0, 700.0)),
    ] {
        let mut harness = Harness::new(220.0, 30.0);
        harness.screen = screen;
        mhf_font::install(&harness.context);
        let mut case = Case::new("grid", FieldType::Bytes, vec![0x41; count]);
        let (_, opened) = open_binary(&mut harness, &mut case, &mut false);
        let positions = text_positions(&opened);
        let header = |label| {
            positions
                .iter()
                .find(|position| position.text == label)
                .unwrap()
        };
        let (offset_header, hex_header, ascii_header) =
            (header("Offset"), header("HEX"), header("ASCII"));
        let close =
            |a: f32, b: f32| assert!((a - b).abs() <= 1.0, "misaligned {a} vs {b}: {positions:?}");
        close(offset_header.baseline, hex_header.baseline);
        close(offset_header.baseline, ascii_header.baseline);
        let offsets = positions
            .iter()
            .filter(|position| {
                position.text.len() == 8
                    && position.text.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .collect::<Vec<_>>();
        assert!(!offsets.is_empty());
        let row_height = (offsets[0].rect.center().y - offset_header.rect.center().y).abs();
        for offset in offsets {
            let same_row = |position: &&TextPosition| {
                (position.rect.center().y - offset.rect.center().y).abs() < row_height / 2.0
            };
            let mut hex = positions
                .iter()
                .filter(same_row)
                .filter(|position| position.text == "41")
                .collect::<Vec<_>>();
            let mut ascii = positions
                .iter()
                .filter(same_row)
                .filter(|position| position.text == "A")
                .collect::<Vec<_>>();
            hex.sort_by(|a, b| a.left.total_cmp(&b.left));
            ascii.sort_by(|a, b| a.left.total_cmp(&b.left));
            assert!(
                !hex.is_empty() && !ascii.is_empty(),
                "missing row cells: {positions:?}"
            );
            close(offset.left, offset_header.left);
            close(hex[0].left, hex_header.left);
            close(ascii[0].left, ascii_header.left);
            for cell in hex.into_iter().chain(ascii) {
                close(cell.baseline, offset.baseline);
                close(cell.rect.center().y, offset.rect.center().y);
            }
        }
    }
}

// Full inspector tests below also exercise fixed name/value columns.

fn inspector_document(long: bool) -> crate::inspect::Document {
    use crate::field::Field;
    let mut bytes = vec![0];
    let mut fields = vec![Field {
        name: "field-0".into(),
        value: if long {
            format!("READONLY {}", "a very long decoded value ".repeat(100))
        } else {
            "ok".into()
        },
        note: None,
        binding: Binding {
            buffer: 0,
            range: 0..1,
            format: FieldType::Bytes,
            endian: Endian::Little,
        },
        writable: false,
    }];
    for (index, case) in cases().into_iter().enumerate() {
        let start = bytes.len();
        bytes.extend_from_slice(&case.original);
        fields.push(Field {
            name: format!("field-{}", index + 1),
            value: case.text,
            note: None,
            binding: Binding {
                range: start..bytes.len(),
                ..case.binding
            },
            writable: true,
        });
    }
    if long {
        for field in &mut fields {
            field.name.push_str(&format!(
                " / {}",
                "very-long-resource-field-name ".repeat(50)
            ));
        }
    }
    let mut document = crate::inspect::inspect("layout.bin", bytes.into());
    let root = &mut document.nodes[document.root];
    root.kind = crate::inspect::Kind::Block;
    root.children.clear();
    root.fields = fields;
    document
}

#[test]
fn inspector_names_readonly_values_and_popups_keep_uniform_rows() {
    use crate::{preview::Control, settings::ViewSettings, worker::Worker};
    use std::{fs, sync::Arc};
    let directory =
        std::env::temp_dir().join(format!("mhf-workbench-field-layout-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let short = inspector_document(false);
    let long = inspector_document(true);
    for width in [220.0, 320.0, 500.0] {
        let worker = Arc::new(Worker::start(directory.clone(), directory.join("exports")).unwrap());
        let mut workbench = super::Workbench::new(
            Arc::new(Control::default()),
            worker,
            directory.clone(),
            ViewSettings::default(),
            None,
        );
        workbench.path = Some(directory.join("layout.bin"));
        let mut harness = Harness::new(width, 650.0);
        let mut baseline = harness.frame(vec![], |ui| {
            workbench.inspector_fields(ui, &short, &short.nodes[short.root]);
            false
        });
        for _ in 0..3 {
            baseline = harness.frame(vec![], |ui| {
                workbench.inspector_fields(ui, &short, &short.nodes[short.root]);
                false
            });
        }
        let mut expanded = harness.frame(vec![], |ui| {
            workbench.inspector_fields(ui, &long, &long.nodes[long.root]);
            false
        });
        for _ in 0..3 {
            expanded = harness.frame(vec![], |ui| {
                workbench.inspector_fields(ui, &long, &long.nodes[long.root]);
                false
            });
        }
        expanded.fits("full inspector");
        expanded.same_main_as(&baseline);
        let texts = expanded.texts();
        let mut names = texts
            .iter()
            .filter(|(text, _, _)| text.starts_with("field-"))
            .collect::<Vec<_>>();
        names.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
        assert_eq!(names.len(), long.nodes[long.root].fields.len());
        assert!(
            names.iter().all(|(_, _, elided)| *elided),
            "long field names should visibly truncate"
        );
        let gaps = names
            .windows(2)
            .map(|pair| pair[1].1.center().y - pair[0].1.center().y)
            .collect::<Vec<_>>();
        assert!(
            gaps.iter().all(|gap| (gap - gaps[0]).abs() <= 1.0),
            "different editors must not change row spacing: {gaps:?}"
        );
        assert!(
            texts
                .iter()
                .any(|(text, _, elided)| text.starts_with("READONLY") && *elided)
        );
        let anchor = expanded.text_center("位");
        assert!(expanded.main.contains(anchor));
        for pressed in [true, false] {
            harness.frame(pointer(anchor, pressed), |ui| {
                workbench.inspector_fields(ui, &long, &long.nodes[long.root]);
                false
            });
        }
        let mut opened = harness.frame(vec![], |ui| {
            workbench.inspector_fields(ui, &long, &long.nodes[long.root]);
            false
        });
        for _ in 0..3 {
            opened = harness.frame(vec![], |ui| {
                workbench.inspector_fields(ui, &long, &long.nodes[long.root]);
                false
            });
        }
        assert!(egui::Popup::is_any_open(&harness.context));
        opened.same_main_as(&expanded);
        opened.fits("inspector with a popup");
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tab_selects_the_next_byte_for_direct_replacement() {
    let mut harness = Harness::new(220.0, 30.0);
    let mut case = Case::new("tab bytes", FieldType::Bytes, vec![0x41, 0x42, 0x43]);
    let mut background_clicked = false;
    let (_, opened) = open_binary(&mut harness, &mut case, &mut background_clicked);
    type_byte(
        &mut harness,
        &mut case,
        &opened,
        "41",
        "AA",
        &mut background_clicked,
    );
    for pressed in [true, false] {
        binary_frame(
            &mut harness,
            &mut case,
            vec![Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            &mut background_clicked,
        );
    }
    let edited = binary_frame(
        &mut harness,
        &mut case,
        vec![Event::Text("BB".into())],
        &mut background_clicked,
    );
    assert!(edited.changed);
    assert_eq!(
        case.binding.encode(&case.original, &case.text).unwrap(),
        [0xaa, 0xbb, 0x43]
    );
}

fn hover_truncated_inspector_field(name: bool) {
    use crate::{preview::Control, settings::ViewSettings, worker::Worker};
    use std::{fs, path::PathBuf, sync::Arc};

    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(std::env::temp_dir().join(format!(
        "mhf-workbench-tooltip-{}-{name}",
        std::process::id(),
    )));
    fs::create_dir_all(&directory.0).unwrap();
    let mut document = inspector_document(true);
    let node = &mut document.nodes[document.root];
    node.fields.truncate(1);
    node.fields[0].name = format!("field-name {}", "long field label ".repeat(20));
    node.fields[0].value = format!("READONLY {}", "complete decoded value ".repeat(24));
    let field = &node.fields[0];
    let complete = if name {
        field.name.clone()
    } else {
        field.value.clone()
    };
    let offset = format!("0x{:08X}", field.binding.range.start);
    let buffer = format!("b{}", field.binding.buffer);

    for width in [220.0, 320.0, 500.0] {
        let worker =
            Arc::new(Worker::start(directory.0.clone(), directory.0.join("exports")).unwrap());
        let mut workbench = super::Workbench::new(
            Arc::new(Control::default()),
            worker,
            directory.0.clone(),
            ViewSettings::default(),
            None,
        );
        workbench.path = Some(directory.0.join("layout.bin"));
        let mut harness = Harness::new(width, 650.0);
        harness.context.all_styles_mut(|style| {
            style.interaction.tooltip_delay = 0.0;
            style.interaction.tooltip_grace_time = 0.0;
            style.interaction.show_tooltips_only_when_still = false;
        });
        let mut closed = harness.frame(vec![], |ui| {
            workbench.inspector_fields(ui, &document, &document.nodes[document.root]);
            false
        });
        for _ in 0..3 {
            closed = harness.frame(vec![], |ui| {
                workbench.inspector_fields(ui, &document, &document.nodes[document.root]);
                false
            });
        }
        let anchor = closed
            .texts()
            .into_iter()
            .find(|(text, _, elided)| text == &complete && *elided)
            .expect("the displayed field must be truncated before hovering")
            .1
            .center();
        let mut hovered = harness.frame(vec![Event::PointerMoved(anchor)], |ui| {
            workbench.inspector_fields(ui, &document, &document.nodes[document.root]);
            false
        });
        for _ in 0..7 {
            hovered = harness.frame(vec![], |ui| {
                workbench.inspector_fields(ui, &document, &document.nodes[document.root]);
                false
            });
        }
        hovered.same_main_as(&closed);
        let tooltips = hovered
            .texts()
            .into_iter()
            .filter(|(text, _, elided)| !*elided && text.contains(&complete))
            .collect::<Vec<_>>();
        assert_eq!(
            tooltips.len(),
            1,
            "{width}px {} should show one complete tooltip",
            if name { "field name" } else { "readonly value" }
        );
        if name {
            assert!(
                tooltips[0].0.contains(&offset),
                "the custom tooltip retains the field offset"
            );
            assert!(
                tooltips[0].0.contains(&buffer),
                "the custom tooltip retains the buffer identity"
            );
        } else {
            assert_eq!(tooltips[0].0, complete);
        }
    }
}

#[test]
fn a_truncated_readonly_value_has_one_complete_hover_tooltip() {
    hover_truncated_inspector_field(false);
}

#[test]
fn a_truncated_field_name_has_one_custom_hover_tooltip_with_its_offset() {
    hover_truncated_inspector_field(true);
}

#[derive(Debug)]
struct DockAction {
    label: String,
    text: Rect,
    clip: Rect,
    elided: bool,
}

struct DockFrame {
    panel: Rect,
    content: Rect,
    actions: Vec<DockAction>,
    bar: Option<egui::Response>,
    buttons: Vec<(&'static str, egui::Response)>,
    font: egui::FontId,
    ppp: f32,
}

impl DockFrame {
    fn assert_actions_fit(&self) {
        let tolerance = 1.0 / self.ppp;
        let diagnostics = || {
            format!(
                "panel={:?} content={:?} bar={:?} font={:?} ppp={} actions={:?} buttons={:?}",
                self.panel,
                self.content,
                self.bar.as_ref().map(|bar| bar.rect),
                self.font,
                self.ppp,
                self.actions,
                self.buttons
                    .iter()
                    .map(|(label, button)| (*label, button.rect, button.interact_rect))
                    .collect::<Vec<_>>(),
            )
        };
        assert!(
            self.actions.iter().any(|action| action.label == "编辑"),
            "missing byte editor: {}",
            diagnostics()
        );
        assert!(
            self.actions.iter().any(|action| action.label == "位"),
            "missing flags editor: {}",
            diagnostics()
        );
        for action in &self.actions {
            // Deliberately use the original galley rect. Intersecting with clip
            // first would hide exactly the missing right-hand text in this bug.
            assert!(
                !action.elided,
                "action label was shortened: {}",
                diagnostics()
            );
            assert!(
                action.text.left() >= action.clip.left() - tolerance
                    && action.text.right() <= action.clip.right() + tolerance,
                "action text is clipped: {}",
                diagnostics()
            );
            assert!(
                action.text.right() <= self.content.right() + tolerance,
                "action text left the panel: {}",
                diagnostics()
            );
        }
        for (_, button) in &self.buttons {
            assert!(
                button.rect.left() >= self.content.left() - tolerance
                    && button.rect.right() <= self.content.right() + tolerance,
                "complete button left the panel: {}",
                diagnostics()
            );
            assert!(
                button.interact_rect.width() + tolerance >= button.rect.width(),
                "the button's clickable area is clipped: {}",
                diagnostics()
            );
            if let Some(bar) = &self.bar {
                assert!(
                    button.rect.right() <= bar.rect.left() + tolerance,
                    "scrollbar covers the button: {}",
                    diagnostics()
                );
            }
        }
    }
}

struct DockHarness {
    context: egui::Context,
    workbench: super::Workbench,
    screen: egui::Vec2,
    ppp: f32,
    time: f64,
    ids: Vec<(&'static str, egui::Id)>,
    directory: std::path::PathBuf,
}

impl DockHarness {
    fn new() -> Self {
        use crate::{
            field::Field, preview::Control, session::Session, settings::ViewSettings,
            worker::Worker,
        };
        use std::{fs, sync::Arc};
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-dock-actions-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let worker = Arc::new(Worker::start(directory.clone(), directory.join("exports")).unwrap());
        let path = directory.join("dock.bin");
        let mut document = crate::inspect::inspect(&path.to_string_lossy(), vec![0; 320].into());
        let node = &mut document.nodes[document.root];
        node.kind = crate::inspect::Kind::Block;
        node.children.clear();
        node.fields = (0..80)
            .map(|index| Field {
                name: format!("字段 {index:02}"),
                value: "0".into(),
                note: None,
                writable: true,
                binding: Binding {
                    buffer: 0,
                    range: index * 4..index * 4 + 4,
                    endian: Endian::Little,
                    format: if index % 2 == 0 {
                        FieldType::Flags(ScalarType::U32)
                    } else {
                        FieldType::Bytes
                    },
                },
            })
            .collect();
        let document = Arc::new(document);
        let mut workbench = super::Workbench::new(
            Arc::new(Control::default()),
            worker,
            directory.clone(),
            ViewSettings::default(),
            None,
        );
        workbench.path = Some(path.clone());
        workbench.tab = super::InspectorTab::Resource;
        workbench
            .editing
            .sessions
            .insert(path, Session::new(document.clone()));
        workbench.loaded_document(document);
        let mut harness = Self {
            context: egui::Context::default(),
            workbench,
            screen: egui::vec2(960.0, 700.0),
            ppp: 0.9,
            time: 0.0,
            ids: Vec::new(),
            directory,
        };
        harness.reset_context();
        harness
    }

    fn reset_context(&mut self) {
        self.context = egui::Context::default();
        egui_hunter::Theme::default().apply(&self.context);
        mhf_font::install(&self.context);
        self.time = 0.0;
    }

    fn frame(&mut self, events: Vec<Event>) -> DockFrame {
        self.time += 0.025;
        let mut panel = Rect::NOTHING;
        let mut content = Rect::NOTHING;
        let mut scroll = egui::Id::NULL;
        let mut font = egui::FontId::default();
        let mut bar = None;
        let mut buttons = Vec::new();
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(self.ppp);
        let snapshot = self.workbench.control.snapshot();
        let output = self.context.run_ui(input, |ui| {
            egui_hunter::Density::Compact.scope(ui, |ui| {
                let side_limit = ((self.screen.x - 300.0) / 2.0).clamp(160.0, 520.0);
                let shown = egui::Panel::right("workbench-inspector")
                    .default_size(320.0)
                    .size_range(220.0_f32.min(side_limit)..=side_limit)
                    .frame(
                        egui::Frame::NONE
                            .fill(ui.visuals().window_fill)
                            .inner_margin(ui.spacing().window_margin),
                    )
                    .show(ui, |ui| {
                        content = ui.max_rect();
                        // ScrollArea stores an IdSalt first, then salts the Ui
                        // with that value; passing the raw string hashes once less.
                        scroll =
                            ui.make_persistent_id(egui::IdSalt::new("workbench-inspector-content"));
                        font = ui
                            .style()
                            .button_style(
                                &Default::default(),
                                egui::widget_style::WidgetState::Inactive,
                            )
                            .text_style
                            .font_id
                            .clone();
                        self.workbench.inspector_panel(ui, &snapshot);
                    });
                panel = shown.response.rect;
                // Responses are read before end_pass rotates the widget buffers.
                // Reading after run_ui can compare this frame's paint with the
                // previous frame's geometry immediately after a resize.
                bar = ui.ctx().read_response(scroll.with(1_usize));
                buttons = self
                    .ids
                    .iter()
                    .filter_map(|(label, id)| {
                        ui.ctx()
                            .read_response(*id)
                            .map(|response| (*label, response))
                    })
                    .collect();
            });
        });
        fn actions(shape: &Shape, clip: Rect, result: &mut Vec<DockAction>) {
            match shape {
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        actions(shape, clip, result);
                    }
                }
                Shape::Text(text) if matches!(text.galley.text(), "编辑" | "位") => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    if rect.intersects(clip) {
                        result.push(DockAction {
                            label: text.galley.text().into(),
                            text: rect,
                            clip,
                            elided: text.galley.elided,
                        });
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in &output.shapes {
            actions(&shape.shape, shape.clip_rect, &mut labels);
        }
        output.drop_without_applying_deltas();
        DockFrame {
            panel,
            content,
            actions: labels,
            bar,
            buttons,
            font,
            ppp: self.context.pixels_per_point(),
        }
    }

    fn discover_buttons(&mut self) {
        let mut frame = self.frame(vec![]);
        for _ in 0..10 {
            frame = self.frame(vec![]);
        }
        for label in ["位", "编辑"] {
            let point = frame
                .actions
                .iter()
                .find(|action| action.label == label)
                .unwrap()
                .text
                .intersect(
                    frame
                        .actions
                        .iter()
                        .find(|action| action.label == label)
                        .unwrap()
                        .clip,
                )
                .center();
            frame = self.frame(vec![Event::PointerMoved(point)]);
            let hovered = self.context.interaction_snapshot(|snapshot| {
                snapshot.hovered.iter().copied().collect::<Vec<_>>()
            });
            let response = hovered
                .into_iter()
                .filter_map(|id| self.context.read_response(id))
                .filter(|response| {
                    response.sense.senses_click() && response.interact_rect.contains(point)
                })
                .min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()))
                .expect("hovered action button");
            self.ids.push((label, response.id));
        }
    }
}

impl Drop for DockHarness {
    fn drop(&mut self) {
        if let Some(worker) = std::sync::Arc::get_mut(&mut self.workbench.worker) {
            worker.stop();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn dock_actions_remain_fully_clickable_beside_scrollbars_after_resize_and_dpi_changes() {
    let mut harness = DockHarness::new();
    harness.discover_buttons();
    // Replay from a fresh Context with the real widget IDs, so the first layout
    // pass is checked too, before cached ScrollArea sizes and bar animations.
    harness.reset_context();
    for (screen, ppp) in [
        (egui::vec2(960.0, 700.0), 0.9),
        (egui::vec2(640.0, 540.0), 0.9),
        (egui::vec2(640.0, 540.0), 2.0),
    ] {
        harness.screen = screen;
        harness.ppp = ppp;
        let mut frame = harness.frame(vec![]);
        frame.assert_actions_fit();
        for _ in 0..10 {
            let point = frame
                .bar
                .as_ref()
                .map(|bar| bar.rect.center())
                .unwrap_or_else(|| {
                    egui::pos2(frame.content.right() - 2.0, frame.content.center().y)
                });
            frame = harness.frame(vec![Event::PointerMoved(point)]);
            frame.assert_actions_fit();
        }
        assert!(frame.bar.is_some(), "the long inspector must really scroll");
        assert_eq!(
            frame.buttons.len(),
            2,
            "both action widget IDs remain registered"
        );
    }
    for label in ["位", "编辑"] {
        let frame = harness.frame(vec![]);
        let button = &frame
            .buttons
            .iter()
            .find(|(name, _)| *name == label)
            .unwrap()
            .1;
        let edge = egui::pos2(
            button.rect.right() - 1.0 / harness.ppp,
            button.rect.center().y,
        );
        harness.frame(pointer(edge, true));
        harness.frame(pointer(edge, false));
        assert_eq!(
            harness
                .context
                .interaction_snapshot(|snapshot| snapshot.clicked),
            Some(button.id),
            "the button edge must not hit the scrollbar"
        );
        if label == "位" {
            assert!(egui::Popup::is_any_open(&harness.context));
        } else {
            assert!(
                harness
                    .context
                    .memory(|memory| memory.top_modal_layer())
                    .is_some()
            );
        }
        harness.frame(vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        harness.frame(vec![]);
        harness.frame(vec![]);
    }
}

#[test]
fn keyboard_fields_keep_focus_until_enter() {
    for scalar in [ScalarType::U32, ScalarType::U64] {
        let mut case = Case::new("number", FieldType::Scalar(scalar), vec![0; scalar.size()]);
        let mut harness = Harness::new(260.0, 30.0);
        let idle = harness.input(&mut case, vec![]);
        let point = idle.text_center("0");
        harness.input(&mut case, pointer(point, true));
        harness.input(&mut case, pointer(point, false));
        harness.input(&mut case, vec![]);
        assert!(case.focused.is_some());
        harness.input(&mut case, replace_text("12"));
        assert_eq!(case.text, "12");
        assert!(case.focused.is_some());
        harness.input(
            &mut case,
            vec![Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(case.focused.is_none());
    }
}

#[test]
fn multiline_ctrl_enter_submits_without_inserting_a_newline() {
    let mut case = Case::new(
        "text",
        FieldType::Text {
            encoding: TextEncoding::Utf8,
            terminated: false,
        },
        b"hello\nworld".to_vec(),
    );
    let mut harness = Harness::new(260.0, 30.0);
    let (_, opened) = open_popup(&mut harness, &mut case, "编辑");
    let point = opened.text_center("hello\nworld");
    harness.input(&mut case, pointer(point, true));
    harness.input(&mut case, pointer(point, false));
    assert!(case.focused.is_some());
    let text = case.text.clone();
    harness.input(
        &mut case,
        vec![Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
    );
    assert_eq!(case.text, text);
    assert!(case.focused.is_none());
}
