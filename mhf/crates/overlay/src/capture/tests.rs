use super::*;
use egui::{Context, Event, Id, RawInput, Rect, pos2, vec2};

#[test]
fn caller_policy_overrides_hover_and_focus_independently() {
    let ctx = Context::default();
    let mut capture = CaptureState::default();
    let editor = Id::new("editor");
    let mut value = String::new();
    let mut draw = |events, policy| {
        let mut rect = Rect::NOTHING;
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::Window::new("Controls").show(ui.ctx(), |ui| {
                    rect = ui
                        .add(egui::TextEdit::singleline(&mut value).id(editor))
                        .rect;
                });
            },
        );
        output.textures_delta.clear();
        capture.update(
            policy,
            ctx.egui_wants_pointer_input(),
            ctx.egui_wants_keyboard_input(),
        );
        (rect, capture.pointer(), capture.keyboard())
    };
    draw(vec![], InputPolicy::default());
    let (rect, _, _) = draw(vec![], InputPolicy::default());
    let (_, pointer, keyboard) = draw(
        vec![Event::PointerMoved(rect.center())],
        InputPolicy::default(),
    );
    assert!(pointer);
    assert!(!keyboard);

    let (_, pointer, keyboard) = draw(
        vec![],
        InputPolicy {
            pointer: InputCapture::PassThrough,
            keyboard: InputCapture::Block,
        },
    );
    assert!(
        !pointer,
        "the caller permits game clicks over an egui window"
    );
    assert!(keyboard, "the caller blocks keys without an egui focus");

    ctx.memory_mut(|memory| memory.request_focus(editor));
    let (_, pointer, keyboard) = draw(
        vec![Event::PointerMoved(pos2(790.0, 590.0))],
        InputPolicy {
            pointer: InputCapture::Block,
            keyboard: InputCapture::PassThrough,
        },
    );
    assert!(pointer);
    assert!(
        !keyboard,
        "keyboard pass-through overrides a focused editor"
    );

    let (_, pointer, keyboard) = draw(vec![], InputPolicy::default());
    assert!(!pointer);
    assert!(keyboard);
}

#[test]
fn ui_owned_drag_and_key_keep_their_release_when_a_window_closes() {
    let mut capture = CaptureState::default();
    capture.update(InputPolicy::default(), true, true);
    assert!(capture.pointer_button(0, true));
    assert!(capture.key(13, true));
    capture.update(
        InputPolicy {
            pointer: InputCapture::PassThrough,
            keyboard: InputCapture::PassThrough,
        },
        false,
        false,
    );
    assert!(capture.pointer_motion());
    assert!(
        capture.key(13, true),
        "a held key repeat keeps its recipient"
    );
    assert!(capture.pointer_button(0, false));
    assert!(capture.key(13, false));
    assert!(!capture.pointer_motion());
    assert!(!capture.pointer_button(0, true));
    assert!(!capture.key(13, true));
}

#[test]
fn game_owned_press_releases_to_game_after_a_modal_opens() {
    let mut capture = CaptureState::default();
    assert!(!capture.pointer_button(1, true));
    assert!(!capture.key(65, true));
    capture.update(
        InputPolicy {
            pointer: InputCapture::Block,
            keyboard: InputCapture::Block,
        },
        false,
        false,
    );
    assert!(!capture.pointer_motion());
    assert!(!capture.key(65, true));
    assert!(!capture.pointer_button(1, false));
    assert!(!capture.key(65, false));
    assert!(
        !capture.key(66, false),
        "release a key pressed before hook installation"
    );
    assert!(capture.pointer_button(1, true));
    assert!(capture.key(65, true));
}

#[test]
fn render_failure_releases_new_input_without_leaking_a_partial_press() {
    let mut capture = CaptureState::default();
    capture.update(InputPolicy::default(), true, true);
    assert!(capture.pointer_button(4, true));
    assert!(capture.key(32, true));
    capture.clear();
    assert!(!capture.pointer());
    assert!(!capture.keyboard());
    assert!(!capture.pointer_button(2, true));
    assert!(!capture.key(66, true));
    assert!(capture.pointer_button(4, false));
    assert!(capture.key(32, false));
    assert!(!capture.pointer_button(2, false));
    assert!(!capture.key(66, false));
}

#[test]
fn current_pointer_position_captures_the_first_click_before_another_ui_frame() {
    let ctx = Context::default();
    let capture = InputCaptureState::default();
    let adapter = capture.clone();
    let mut button = Rect::NOTHING;
    for _ in 0..2 {
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events: vec![Event::PointerMoved(pos2(790.0, 590.0))],
                ..Default::default()
            },
            |ui| {
                egui::Window::new("Controls").show(ui.ctx(), |ui| {
                    button = ui.button("Open").rect;
                });
            },
        );
        output.textures_delta.clear();
    }
    capture.update(InputPolicy::default(), &ctx);
    assert!(!ctx.egui_wants_pointer_input());
    let mut state = adapter.lock();
    assert!(state.pointer_at(Some(button.center())));
    state.move_pointer(button.center());
    assert!(state.pointer_button(0, true));
    drop(state);
    assert_eq!(adapter.pointer_button_owner(0), Some(true));
    assert_eq!(adapter.pointer_button_owner(6), None);
    let mut state = adapter.lock();
    state.move_pointer(pos2(790.0, 590.0));
    assert!(
        state.pointer_motion(),
        "a UI drag remains captured outside the window"
    );
    assert!(state.pointer_button(0, false));
    assert!(!state.pointer_motion());
    drop(state);
    assert_eq!(adapter.pointer_button_owner(0), None);

    capture.update(
        InputPolicy {
            pointer: InputCapture::PassThrough,
            keyboard: InputCapture::Block,
        },
        &ctx,
    );
    assert!(!adapter.lock().pointer_at(Some(button.center())));
    assert!(adapter.captures_keyboard());
    capture.lock().clear();
    assert!(!adapter.captures_keyboard());
    assert!(!adapter.lock().pointer_at(Some(button.center())));
}

#[test]
fn hit_regions_respect_passthrough_areas_and_ui_scale() {
    let ctx = Context::default();
    ctx.set_pixels_per_point(2.0);
    let capture = InputCaptureState::default();
    let mut button = Rect::NOTHING;
    let mut toast = Rect::NOTHING;
    let mut hud = Rect::NOTHING;
    for _ in 0..2 {
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                egui::Window::new("Controls").show(ui.ctx(), |ui| {
                    button = ui.button("Action").rect;
                });
                egui::Area::new(Id::new("toast"))
                    .fixed_pos(pos2(500.0, 450.0))
                    .interactable(false)
                    .show(ui.ctx(), |ui| {
                        toast = ui.button("Notice").rect;
                    });
                egui::Area::new(Id::new("hud"))
                    .order(egui::Order::Background)
                    .fixed_pos(pos2(300.0, 300.0))
                    .show(ui.ctx(), |ui| {
                        hud = ui.button("HUD action").rect;
                    });
            },
        );
        output.textures_delta.clear();
    }
    capture.update(InputPolicy::default(), &ctx);
    let mut state = capture.lock();
    assert!(!state.pointer_at(Some(toast.center())));
    assert!(state.pointer_at(Some(hud.center())));
    state.move_pointer(button.center() * ctx.pixels_per_point());
    assert!(state.pointer_button(0, true));
    assert!(state.pointer_button(0, false));
    state.move_pointer(toast.center() * ctx.pixels_per_point());
    assert!(!state.pointer_button(0, true));
}
