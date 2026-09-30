use super::*;
use egui::{Context, Event, FullOutput, OutputCommand, Pos2, RawInput, Rect, Shape};

fn context() -> Context {
    let context = Context::default();
    egui_hunter::Theme::default().apply(&context);
    context.all_styles_mut(|style| style.animation_time = 0.0);
    context
}

fn frame(
    context: &Context,
    id: Id,
    width: f32,
    events: Vec<Event>,
    mut draw: impl FnMut(&mut Ui) -> ResourceReferenceResponse,
) -> (ResourceReferenceResponse, Option<Response>, FullOutput) {
    let mut result = None;
    let mut copy = None;
    let mut output = context.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 400.0))),
            events,
            ..Default::default()
        },
        |ui| {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 350.0))),
                |ui| {
                    ui.set_width(width);
                    result = Some(draw(ui));
                    copy = context.read_response(id.with("copy"));
                },
            );
        },
    );
    output.textures_delta.clear();
    (result.unwrap(), copy, output)
}

fn pointer(point: Pos2, pressed: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(point),
        Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn texts(output: &FullOutput) -> Vec<String> {
    fn visit(shape: &Shape, values: &mut Vec<String>) {
        match shape {
            Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, values);
                }
            }
            Shape::Text(text) => values.push(text.galley.text().into()),
            _ => {}
        }
    }
    let mut values = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, &mut values);
    }
    values
}

fn copied(output: &FullOutput) -> Vec<&str> {
    output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn labels_copy_the_canonical_identity_without_becoming_navigation() {
    let context = context();
    let id = Id::new("named-path");
    let path =
        ResourcePath::from_parts("motion\\怪物#%.mot", [mhf_resource::PathSegment::Index(4)])
            .unwrap();
    let draw = |ui: &mut Ui| {
        ResourceReference::new(&path)
            .id(id)
            .label("来源名称")
            .show(ui)
    };
    let (row, copy, output) = frame(&context, id, 240.0, vec![], draw);
    assert!(row.activated.is_none());
    assert!(texts(&output).iter().any(|text| text == "来源名称"));
    let point = copy.unwrap().rect.center();
    let _ = frame(&context, id, 240.0, pointer(point, true), draw);
    let (row, _, output) = frame(&context, id, 240.0, pointer(point, false), draw);
    assert_eq!(copied(&output), ["motion/怪物%23%25.mot#4"]);
    assert!(row.activated.is_none(), "copy is not an activation");
}

#[test]
fn popup_sources_keep_their_full_click_area_after_the_sizing_pass() {
    let context = context();
    let id = Id::new("second-source");
    let selected = std::cell::Cell::new(false);
    let draw = |ui: &mut Ui| {
        let mut target = None;
        let menu = egui::ComboBox::from_id_salt("source-picker")
            .width(240.0)
            .selected_text("Choose source")
            .show_ui(ui, |ui| {
                for (index, file) in ["/external/first.model", "/external/second.model"]
                    .into_iter()
                    .enumerate()
                {
                    let row = ResourceReference::new(ResourceTarget::Source(Path::new(file)))
                        .id(if index == 1 { id } else { Id::new(file) })
                        .compact(true)
                        .show(ui);
                    let response = row.response.interact(egui::Sense::click());
                    if index == 1 {
                        if response.clicked() {
                            selected.set(true);
                            ui.close();
                        }
                        target = Some(ResourceReferenceResponse {
                            response,
                            activated: row.activated,
                        });
                    }
                }
            });
        target.unwrap_or(ResourceReferenceResponse {
            response: menu.response,
            activated: None,
        })
    };
    let (menu, _, _) = frame(&context, id, 240.0, vec![], draw);
    let open = menu.response.rect.center();
    let _ = frame(&context, id, 240.0, pointer(open, true), draw);
    let _ = frame(&context, id, 240.0, pointer(open, false), draw);
    let _ = frame(&context, id, 240.0, vec![], draw);
    let (row, copy, _) = frame(&context, id, 240.0, vec![], draw);
    let point = row.response.rect.center();
    assert!(
        row.response.interact_rect.contains_rect(row.response.rect),
        "popup sizing must include the whole reference row"
    );
    assert!(copy.is_none());
    assert!(row.activated.is_none());
    let _ = frame(&context, id, 240.0, pointer(point, true), draw);
    let (row, _, _) = frame(&context, id, 240.0, pointer(point, false), draw);
    assert!(selected.get(), "the second source must remain selectable");
    assert!(
        row.activated.is_none(),
        "local selection creates no canonical path"
    );
}

#[test]
fn unresolved_queries_keep_raw_keys_and_never_offer_canonical_copy_or_activation() {
    let cases = [
        (
            ResourceTarget::Motion(NativeMotionRef {
                id: 12345,
                weapon: 11,
                style: None,
            }),
            ["12345", "资源库 12", "武器 11"],
        ),
        (
            ResourceTarget::Attack(AttackReference {
                category: 6,
                subtype: Some(65535),
                record: 23,
            }),
            ["mhfsdt.bin", "子类别键[65535]", "记录[23]"],
        ),
        (
            ResourceTarget::NativeScript {
                table: 270,
                index: 255,
            },
            ["DLL AI 原生根引用", "root[270][255]", "未解析"],
        ),
        (
            ResourceTarget::Index {
                collection: "bones",
                index: u32::MAX,
            },
            ["bones", "4294967295", "未解析"],
        ),
    ];
    for (target, expected) in cases {
        let context = context();
        let id = Id::new("unresolved");
        let (row, copy, output) = frame(&context, id, 320.0, vec![], |ui| {
            ResourceReference::new(target.clone())
                .id(id)
                .activate(true)
                .show(ui)
        });
        let text = texts(&output).join("\n");
        for expected in expected {
            assert!(text.contains(expected), "missing {expected:?} in {text:?}");
        }
        assert!(text.contains("未解析"));
        assert!(copy.is_none());
        assert!(row.activated.is_none());
        assert!(copied(&output).is_empty());
    }
}

#[test]
fn activation_is_opt_in_and_returns_the_actual_resolved_path() {
    let context = context();
    let id = Id::new("motion");
    let motion = NativeMotionRef {
        id: 1405,
        weapon: 7,
        style: None,
    };
    let draw = |ui: &mut Ui| {
        ResourceReference::new(motion)
            .id(id)
            .compact(true)
            .activate(true)
            .show(ui)
    };
    let (row, _, _) = frame(&context, id, 240.0, vec![], draw);
    let point = row.response.rect.center();
    assert!(row.activated.is_none());
    let _ = frame(&context, id, 240.0, pointer(point, true), draw);
    let (row, _, _) = frame(&context, id, 240.0, pointer(point, false), draw);
    assert_eq!(row.activated.unwrap().to_string(), "motion/w07.mot#4/5");

    let attack = AttackReference {
        category: 100,
        subtype: Some(2),
        record: 8,
    };
    let resolved: ResourcePath = "mhfsdt.bin#1/attacks/8".parse().unwrap();
    let (row, copy, output) = frame(&context, id, 240.0, vec![], |ui| {
        ResourceReference::new(attack)
            .id(id)
            .resolved_path(&resolved)
            .show(ui)
    });
    assert!(
        texts(&output)
            .iter()
            .any(|text| text == "mhfsdt.bin#1/attacks/8")
    );
    assert!(copy.is_some());
    assert!(row.activated.is_none());
}

#[test]
fn long_references_and_their_copy_buttons_fit_the_available_row() {
    for compact in [false, true] {
        for width in [120.0, 240.0, 320.0] {
            let context = context();
            let id = Id::new("long-path");
            let path =
                ResourcePath::new(format!("motion/{}.mot", "long-source-name".repeat(40))).unwrap();
            let (row, copy, _) = frame(&context, id, width, vec![], |ui| {
                ResourceReference::new(&path)
                    .id(id)
                    .compact(compact)
                    .show(ui)
            });
            let copy = copy.unwrap();
            assert!(
                row.response.rect.left().abs() <= 0.5,
                "the value starts at the row's left edge"
            );
            assert!(copy.rect.right() <= width + 0.5);
            assert!(row.response.rect.right() <= copy.rect.left() + 0.5);
            assert_eq!(copy.rect.height(), if compact { 24.0 } else { 36.0 });
            assert!((row.response.rect.center().y - copy.rect.center().y).abs() < 0.5);
        }
    }
}

#[test]
fn editor_copies_canonical_text_without_submitting_or_replacing_the_draft() {
    let context = context();
    let id = Id::new("editor");
    let mut draft = "motion\\W%23.mot#0004/005".to_owned();
    let original = draft.clone();
    let (_, copy, _) = frame(&context, id, 320.0, vec![], |ui| {
        ResourceReference::editor(id, &mut draft).show(ui)
    });
    let point = copy.unwrap().rect.center();
    let _ = frame(&context, id, 320.0, pointer(point, true), |ui| {
        ResourceReference::editor(id, &mut draft).show(ui)
    });
    let (row, _, output) = frame(&context, id, 320.0, pointer(point, false), |ui| {
        ResourceReference::editor(id, &mut draft).show(ui)
    });
    assert_eq!(copied(&output), ["motion/W%23.mot#4/5"]);
    assert!(row.activated.is_none());
    assert_eq!(draft, original);

    context.memory_mut(|memory| memory.request_focus(id));
    let (row, _, _) = frame(
        &context,
        id,
        320.0,
        vec![Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        |ui| ResourceReference::editor(id, &mut draft).show(ui),
    );
    assert!(
        row.response.has_focus(),
        "the application owns Enter handling"
    );
    assert!(row.activated.is_none());
    assert_eq!(draft, original);

    draft = "../invalid.bin#0".into();
    let invalid_context = self::context();
    let (_, copy, output) = frame(&invalid_context, id, 320.0, vec![], |ui| {
        ResourceReference::editor(id, &mut draft).show(ui)
    });
    assert!(copy.is_none(), "invalid text has no canonical copy");
    assert!(copied(&output).is_empty());
}

#[test]
fn curve_queries_distinguish_native_key_indices_from_data_layer_bytes() {
    let context = context();
    let id = Id::new("curve");
    let lookup = CurveLookup {
        matching_indices: vec![2, 7],
    };
    let (row, copy, output) = frame(&context, id, 320.0, vec![], |ui| {
        ResourceReference::new(ResourceTarget::Curve {
            reference: CurveReference {
                offset: 0x24,
                kind: CurveKind::Integer,
                id: -1,
            },
            lookup: &lookup,
        })
        .id(id)
        .source_range(0x100..0x160)
        .show(ui)
    });
    let text = texts(&output).join("\n");
    for expected in [
        "ID -1",
        "匹配 [2, 7]",
        "原生键索引跨度 2..4",
        "匹配位置与原生跨度不同",
        "数据层偏移 0x100..0x160",
        "96 字节",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text:?}");
    }
    assert!(copy.is_none());
    assert!(row.activated.is_none());
}

#[test]
fn references_start_at_the_grid_value_column_and_stay_inside_its_clip() {
    for zoom in [1.0, 1.5] {
        let context = context();
        context.set_zoom_factor(zoom);
        let id = Id::new("grid-motion");
        let mut left = 0.0;
        let mut clip = Rect::NOTHING;
        for _ in 0..8 {
            let (row, copy, _) = frame(&context, id, 520.0, vec![], |ui| {
                let mut row = None;
                egui::Grid::new("resource-grid")
                    .num_columns(2)
                    .min_col_width(72.0)
                    .max_col_width(436.0)
                    .spacing(egui::vec2(12.0, 4.0))
                    .show(ui, |ui| {
                        ui.weak("动画资源");
                        left = ui.cursor().left();
                        clip = ui.clip_rect();
                        row = Some(
                            ResourceReference::new(NativeMotionRef {
                                id: 1405,
                                weapon: 4,
                                style: Some(0),
                            })
                            .id(id)
                            .show(ui),
                        );
                        ui.end_row();
                        ui.weak("动画参数");
                        ui.label("0, 4");
                        ui.end_row();
                    });
                row.unwrap()
            });
            assert!((row.response.rect.left() - left).abs() <= 0.5);
            assert!(copy.unwrap().rect.right() <= clip.right() + 0.5);
            assert!(row.response.rect.right() <= clip.right() + 0.5);
        }
    }
}

#[test]
fn unaddressable_sources_keep_the_real_file_and_compact_byte_context() {
    let context = context();
    let id = Id::new("source");
    let file = Path::new("/outside-game-root/model.bin");
    let (row, copy, output) = frame(&context, id, 240.0, vec![], |ui| {
        ResourceReference::new(ResourceTarget::Source(file))
            .id(id)
            .compact(true)
            .source_range(0x100..0x160)
            .activate(true)
            .show(ui)
    });
    assert!(texts(&output).iter().any(|text| text == "model.bin"));
    assert!(
        !texts(&output)
            .iter()
            .any(|text| text.contains("数据层偏移")),
        "compact ranges remain in hover"
    );
    assert!(copy.is_none());
    assert!(row.activated.is_none());
}
