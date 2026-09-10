use super::*;
use crate::provider::{Catalog, Equipment, Monster, MonsterAction};
use egui::{Event, Id, RawInput, Rect, pos2, vec2};

fn context() -> Context {
    let context = Context::default();
    egui_hunter::Theme::default().apply(&context);
    mhf_font::install(&context);
    context
}

#[test]
fn result_rows_fit_controls_and_two_lines_without_consuming_the_viewport() {
    for viewport_height in [120.0, 480.0, 1200.0] {
        let context = context();
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(430.0, viewport_height),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let height = result_row_height(ui);
                    let mut controls = Vec::new();
                    let mut rows = Vec::new();
                    for selected in [false, true] {
                        let row = result_row(ui, height, selected, |ui| {
                            controls.push(ui.add(Button::new("换装")).rect);
                            ui.allocate_ui_with_layout(
                                vec2(ui.available_width(), height - 4.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label("猎人武器");
                                    ui.small("编号 100");
                                },
                            );
                        });
                        assert!((row.rect.height() - height).abs() <= 1.0, "{row:?}");
                        rows.push(row.rect);
                    }
                    assert!(rows[0].bottom() <= rows[1].top());
                    for (row, control) in rows.iter().zip(controls) {
                        assert!(control.height() >= ui.spacing().interact_size.y);
                        assert!(row.contains_rect(control));
                    }
                });
            },
        );
        output.drop_without_applying_deltas();
    }
}

fn populated_snapshot(item_count: u16) -> DebugSnapshot {
    let mut catalog = Catalog {
        equipment: (0..item_count)
            .map(|id| Equipment {
                kind: 6,
                id,
                weapon: Some(0),
                name: format!("猎人武器 {id}"),
            })
            .collect(),
        ..Default::default()
    };
    let action_count = item_count.min(200) as u8;
    catalog.actions[0] = (0..action_count)
        .map(|id| Action {
            group: 1,
            id,
            weapon: 0,
        })
        .collect();
    catalog.monsters = vec![Monster {
        id: 94,
        name: "测试怪物",
        actions: (0..action_count)
            .map(|id| MonsterAction { group: 1, id })
            .collect(),
    }];
    DebugSnapshot {
        quest_id: 1,
        ready: true,
        area: 245,
        map: 44,
        areas: vec![245, 246],
        equipment: vec![(6, 0)],
        catalog: Arc::new(catalog),
        ..Default::default()
    }
}

#[test]
fn each_page_reveals_and_activates_the_footer_in_a_short_window() {
    verify_footer_accessibility(false);
}

#[test]
fn tab_navigation_reaches_and_activates_the_footer_on_every_page() {
    verify_footer_accessibility(true);
}

fn verify_footer_accessibility(navigate_with_tabs: bool) {
    for height in [380.0, 900.0] {
        for page in 0..3 {
            let context = context();
            let control = DebugControl::new();
            let mut window = DebugWindow::new(control.clone());
            window.page = page;
            let mut input = InputController::default();
            // Keep a large catalog for programmatic focus and geometry. For
            // the full Tab route, use a list longer than the visible viewport.
            let snapshot = populated_snapshot(if navigate_with_tabs { 8 } else { 2000 });
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(480.0, height));
            let mut time = 0.0;
            let mut frame = |events: Vec<Event>, focus_footer: bool| {
                let output = context.run_ui(
                    RawInput {
                        screen_rect: Some(screen),
                        time: Some(time),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        if focus_footer {
                            // Focus requests belong inside the egui pass so
                            // gained_focus can observe the transition.
                            ui.memory_mut(|memory| memory.request_focus(Id::new("debug-exit")));
                        }
                        window.show(ui.ctx(), &snapshot, &mut input);
                    },
                );
                let visible_equipment = output.shapes.iter().filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Text(text)
                        if text.galley.job.text.starts_with("猎人武器 ")
                            && clipped.clip_rect.contains_rect(clipped.shape.visual_bounding_rect()))
                }).count();
                output.drop_without_applying_deltas();
                time += 0.2;
                visible_equipment
            };
            frame(vec![], false);
            let visible_equipment = frame(vec![], false);
            let tab = context
                .read_response(Id::new("debug-pages").with(("header", Id::new("equipment"))))
                .unwrap();
            assert!(
                tab.rect.top() < 160.0,
                "compact header is too tall: {tab:?}"
            );
            if height == 900.0 && page == 0 {
                assert!(
                    visible_equipment >= 6,
                    "only {visible_equipment} equipment rows are fully visible"
                );
            }
            if navigate_with_tabs {
                for _ in 0..80 {
                    frame(
                        [true, false]
                            .map(|pressed| Event::Key {
                                key: Key::Tab,
                                physical_key: None,
                                pressed,
                                repeat: false,
                                modifiers: Modifiers::NONE,
                            })
                            .into(),
                        false,
                    );
                    for _ in 0..4 {
                        frame(vec![], false);
                    }
                    if let Some(focused) = context
                        .memory(|memory| memory.focused())
                        .and_then(|id| context.read_response(id))
                    {
                        assert!(
                            screen.contains_rect(focused.rect)
                                && focused.interact_rect.height() >= focused.rect.height() - 1.0
                                && focused.interact_rect.width() >= focused.rect.width() - 1.0,
                            "Tab focus is clipped on page {page}, height {height}: {focused:?}"
                        );
                    }
                    if context.memory(|memory| memory.focused()) == Some(Id::new("debug-exit")) {
                        break;
                    }
                }
            } else {
                frame(vec![], true);
            }
            for _ in 0..8 {
                frame(vec![], false);
            }
            let exit = context.read_response(Id::new("debug-exit")).unwrap();
            assert!(exit.has_focus(), "page {page}, height {height}");
            assert!(
                screen.contains_rect(exit.rect),
                "page {page}, height {height}: {exit:?}"
            );
            assert!(
                exit.interact_rect.height() >= exit.rect.height() - 1.0
                    && exit.interact_rect.width() >= exit.rect.width() - 1.0,
                "footer is clipped on page {page}, height {height}: {exit:?}"
            );
            frame(
                vec![Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
                false,
            );
            assert!(matches!(
                control.commands().as_slice(),
                [DebugCommand::Exit]
            ));
        }
    }
}
