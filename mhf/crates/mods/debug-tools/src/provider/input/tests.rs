use super::*;
use crate::provider::ui::DebugWindow;
use egui::{Event, Modifiers, RawInput};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn control() -> Arc<DebugControl> {
    DebugControl::new()
}

fn new_context() -> Context {
    let context = Context::default();
    mhf_font::install(&context);
    context
}

fn frame(context: &Context, raw: RawInput, mut show: impl FnMut(&Context)) {
    context
        .run_ui(raw, |ui| show(ui.ctx()))
        .drop_without_applying_deltas();
}

fn snapshot() -> DebugSnapshot {
    DebugSnapshot {
        monster: Some(94),
        controlling_monster: true,
        ..Default::default()
    }
}

fn keys(keys: &[Key], modifiers: Modifiers) -> RawInput {
    RawInput {
        focused: true,
        events: std::iter::once(Event::ModifiersChanged(modifiers))
            .chain(keys.iter().map(|key| Event::Key {
                key: *key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }))
            .collect(),
        ..Default::default()
    }
}

fn axes(input: MonsterInput) -> [f32; 4] {
    [input.forward, input.sideways, input.vertical, input.speed]
}

#[test]
fn movement_uses_existing_axes_speed_boost_and_cancelling_keys() {
    let context = new_context();
    let control = control();
    let mut input = InputController {
        speed: 450.0,
        ..Default::default()
    };
    frame(
        &context,
        keys(&[Key::W, Key::D, Key::E], Modifiers::SHIFT),
        |context| {
            input.update(context, &control, &snapshot(), false);
        },
    );
    assert_eq!(axes(control.monster_input()), [1.0, 1.0, 1.0, 1350.0]);
    frame(
        &context,
        keys(&[Key::S, Key::A, Key::Q], Modifiers::NONE),
        |context| {
            input.update(context, &control, &snapshot(), false);
        },
    );
    assert_eq!(axes(control.monster_input()), [0.0, 0.0, 0.0, 450.0]);
    assert!(control.commands().is_empty());
}

#[test]
fn shortcuts_use_the_selected_species_and_clear_only_when_selection_changes() {
    let mut input = InputController::default();
    let actions = [
        MonsterAction { group: 1, id: 11 },
        MonsterAction { group: 2, id: 22 },
        MonsterAction { group: 3, id: 33 },
        MonsterAction { group: 4, id: 44 },
    ];
    input.shortcuts = actions.map(Some);
    input.select_species(94);
    assert!(
        input
            .shortcuts
            .iter()
            .zip(actions)
            .all(|(binding, action)| *binding == Some(action))
    );
    let control = control();
    let context = new_context();
    frame(
        &context,
        keys(
            &[
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::R,
                Key::Backspace,
            ],
            Modifiers::NONE,
        ),
        |context| {
            input.update(context, &control, &snapshot(), false);
        },
    );
    let commands = control.commands();
    assert_eq!(commands.len(), 6);
    for (command, expected) in commands[..4].iter().zip(actions) {
        assert!(matches!(command, DebugCommand::MonsterAction(action) if *action == expected));
    }
    assert!(matches!(commands[4], DebugCommand::NextMonsterAction));
    assert!(matches!(commands[5], DebugCommand::RestoreHunter));

    for monster in [None, Some(1)] {
        let context = new_context();
        let snapshot = DebugSnapshot {
            monster,
            ..snapshot()
        };
        frame(
            &context,
            keys(
                &[Key::Num1, Key::Num2, Key::Num3, Key::Num4],
                Modifiers::NONE,
            ),
            |context| {
                input.update(context, &control, &snapshot, false);
            },
        );
        assert!(
            control.commands().is_empty(),
            "never apply a different species' bindings"
        );
    }
    input.select_species(1);
    assert!(input.shortcuts.iter().all(Option::is_none));
    let context = new_context();
    let selected = DebugSnapshot {
        monster: Some(1),
        ..snapshot()
    };
    frame(&context, keys(&[Key::Num1], Modifiers::NONE), |context| {
        input.update(context, &control, &selected, false);
    });
    assert!(
        control.commands().is_empty(),
        "unbound slots do not send commands"
    );
}

#[test]
fn variant_changes_clear_bindings_and_species_changes_restore_the_normal_variant() {
    let action = MonsterAction { group: 2, id: 7 };
    let mut input = InputController::default();
    for variant in [1, 16] {
        input.shortcuts[0] = Some(action);
        input.select_variant(variant);
        assert_eq!(input.variant(), variant);
        assert!(input.shortcuts.iter().all(Option::is_none));

        input.shortcuts[0] = Some(action);
        input.select_variant(variant);
        input.select_species(input.species());
        assert_eq!(input.variant(), variant);
        assert!(input.shortcuts[0] == Some(action));
    }
    input.select_species(1);
    assert_eq!(input.species(), 1);
    assert_eq!(input.variant(), 0);
    assert!(input.shortcuts.iter().all(Option::is_none));
}

#[test]
fn shortcuts_require_the_selected_variant_to_match_the_controlled_monster() {
    let control = control();
    let action = MonsterAction { group: 2, id: 7 };
    let mut input = InputController::default();
    input.select_variant(1);
    input.shortcuts[0] = Some(action);
    for variant in [0, 16, 1] {
        let snapshot = DebugSnapshot {
            monster_variant: variant,
            ..snapshot()
        };
        frame(
            &new_context(),
            keys(&[Key::Num1, Key::W], Modifiers::NONE),
            |context| input.update(context, &control, &snapshot, false),
        );
        let commands = control.commands();
        if variant == input.variant() {
            assert!(matches!(
                commands.as_slice(),
                [DebugCommand::MonsterAction(bound)] if *bound == action
            ));
        } else {
            assert!(commands.is_empty());
        }
        assert_eq!(control.monster_input().forward, 1.0);
    }
}

#[test]
fn window_capture_application_focus_and_stopped_control_publish_neutral_input() {
    let control = control();
    let mut input = InputController::default();
    for (window_capture, focused, controlling) in [
        (true, true, true),
        (false, false, true),
        (false, true, false),
    ] {
        control.set_monster_input(MonsterInput {
            forward: 1.0,
            speed: 300.0,
            ..Default::default()
        });
        let context = new_context();
        let mut raw = keys(&[Key::W, Key::R, Key::Backspace], Modifiers::NONE);
        raw.focused = focused;
        let snapshot = DebugSnapshot {
            controlling_monster: controlling,
            ..snapshot()
        };
        frame(&context, raw, |context| {
            input.update(context, &control, &snapshot, window_capture)
        });
        assert_eq!(axes(control.monster_input()), [0.0; 4]);
        assert!(control.commands().is_empty());
    }
}

#[test]
fn focused_egui_editor_blocks_gameplay_even_without_window_capture() {
    let context = new_context();
    let control = control();
    let mut input = InputController::default();
    let mut text = String::new();
    context
        .run_ui(keys(&[Key::W, Key::R], Modifiers::NONE), |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
            assert!(ui.ctx().egui_wants_keyboard_input());
            input.update(ui.ctx(), &control, &snapshot(), false);
        })
        .drop_without_applying_deltas();
    assert_eq!(axes(control.monster_input()), [0.0; 4]);
    assert!(control.commands().is_empty());
}

#[test]
fn closing_the_debug_window_releases_gameplay_input_and_keeps_controller_settings() {
    let context = new_context();
    let control = control();
    let mut window = DebugWindow::new(control.clone());
    let mut input = InputController {
        speed: 500.0,
        ..Default::default()
    };
    input.shortcuts[0] = Some(MonsterAction { group: 2, id: 7 });
    frame(&context, keys(&[Key::W], Modifiers::NONE), |context| {
        let capture = window.show(context, &snapshot(), &mut input);
        assert!(capture);
        input.update(context, &control, &snapshot(), capture);
    });
    assert_eq!(axes(control.monster_input()), [0.0; 4]);
    frame(
        &context,
        keys(&[Key::F7, Key::Num1], Modifiers::NONE),
        |context| {
            let capture = window.show(context, &snapshot(), &mut input);
            assert!(!capture);
            assert!(!context.input(|input| input.key_pressed(Key::F7)));
            input.update(context, &control, &snapshot(), capture);
        },
    );
    assert_eq!(axes(control.monster_input()), [1.0, 0.0, 0.0, 500.0]);
    assert!(matches!(
        control.commands().as_slice(),
        [DebugCommand::MonsterAction(MonsterAction {
            group: 2,
            id: 7
        })]
    ));
    // Sampling does not need to render the hidden window at all.
    frame(&context, keys(&[Key::R], Modifiers::NONE), |context| {
        input.update(context, &control, &snapshot(), false);
    });
    assert_eq!(axes(control.monster_input()), [1.0, 0.0, 0.0, 500.0]);
    assert!(matches!(
        control.commands().as_slice(),
        [DebugCommand::NextMonsterAction]
    ));
}

#[test]
fn missing_frames_expire_movement_and_command_errors_are_delivered_once() {
    let context = new_context();
    let control = control();
    let mut input = InputController::default();
    frame(&context, keys(&[Key::W], Modifiers::NONE), |context| {
        input.update(context, &control, &snapshot(), false);
    });
    assert_eq!(control.monster_input().forward, 1.0);
    control.shared.lock().unwrap().input_at = Some(Instant::now() - Duration::from_millis(201));
    assert_eq!(axes(control.monster_input()), [0.0; 4]);

    for _ in 0..16 {
        control.send(DebugCommand::NextMonsterAction).unwrap();
    }
    frame(&context, keys(&[Key::R], Modifiers::NONE), |context| {
        input.update(context, &control, &snapshot(), false);
    });
    assert_eq!(
        input.take_command_error().as_deref(),
        Some("等待上一个调试操作完成")
    );
    assert!(input.take_command_error().is_none());
    control.commands();
    frame(
        &new_context(),
        keys(&[Key::R], Modifiers::NONE),
        |context| {
            input.update(context, &control, &snapshot(), false);
        },
    );
    assert_eq!(input.take_command_error().as_deref(), Some(""));
}
