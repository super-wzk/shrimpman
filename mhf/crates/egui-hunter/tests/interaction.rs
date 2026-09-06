mod events;

use egui::{Context, Event, Id, Key, RawInput, Rect, pos2, vec2};
use egui_hunter::{Button, Checkbox, ItemSlot, Theme, Toggle};
use events::{key, pointer};

fn frame<R>(ctx: &Context, events: Vec<Event>, mut show: impl FnMut(&mut egui::Ui) -> R) -> R {
    let mut result = None;
    let output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                result = Some(show(ui));
            });
        },
    );
    output.drop_without_applying_deltas();
    result.expect("UI should be drawn")
}

#[test]
fn checkbox_changes_on_release_and_reports_changed() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut checked = false;
    let response = frame(&ctx, vec![], |ui| {
        ui.add(Checkbox::new(&mut checked, "提示"))
    });
    let pos = response.rect.center();
    frame(&ctx, pointer(pos, true), |ui| {
        ui.add(Checkbox::new(&mut checked, "提示"))
    });
    assert!(!checked, "pressing alone must not change the value");
    let response = frame(&ctx, pointer(pos, false), |ui| {
        ui.add(Checkbox::new(&mut checked, "提示"))
    });
    assert!(checked);
    assert!(response.changed());
}

#[test]
fn disabled_toggle_rejects_pointer_and_keyboard_activation() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut checked = false;
    let response = frame(&ctx, vec![], |ui| {
        ui.add_enabled(false, Toggle::new(&mut checked, "整理"))
    });
    let pos = response.rect.center();
    ctx.memory_mut(|m| m.request_focus(response.id));
    frame(&ctx, pointer(pos, true), |ui| {
        ui.add_enabled(false, Toggle::new(&mut checked, "整理"))
    });
    let mut events = pointer(pos, false);
    events.push(key(Key::Space));
    let response = frame(&ctx, events, |ui| {
        ui.add_enabled(false, Toggle::new(&mut checked, "整理"))
    });
    assert!(!checked);
    assert!(!response.changed());
    assert!(!response.clicked());
}

#[test]
fn focused_controls_support_space_and_enter() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut checked = false;
    let response = frame(&ctx, vec![], |ui| ui.add(Toggle::new(&mut checked, "整理")));
    ctx.memory_mut(|m| m.request_focus(response.id));
    let response = frame(&ctx, vec![key(Key::Space)], |ui| {
        ui.add(Toggle::new(&mut checked, "整理"))
    });
    assert!(checked);
    assert!(response.changed());

    let response = frame(&ctx, vec![], |ui| ui.add(Button::new("确认")));
    ctx.memory_mut(|m| m.request_focus(response.id));
    let response = frame(&ctx, vec![key(Key::Enter)], |ui| {
        ui.add(Button::new("确认"))
    });
    assert!(response.clicked());
}

#[test]
fn mouse_clicks_focus_all_styled_controls_without_hover_stealing_focus() {
    for kind in 0..4 {
        let ctx = Context::default();
        Theme::default().apply(&ctx);
        let mut checked = false;
        let mut draw = |ui: &mut egui::Ui| {
            let first = ui.add(Button::new("first").id(Id::new("first")));
            let target = match kind {
                0 => ui.add(Button::new("target")),
                1 => ui.add(Checkbox::new(&mut checked, "target")),
                2 => ui.add(Toggle::new(&mut checked, "target")),
                _ => ui.add(ItemSlot::new("target")),
            };
            (first, target)
        };
        let (first, target) = frame(&ctx, vec![], &mut draw);
        first.request_focus();
        let pos = target.rect.center();
        frame(&ctx, vec![Event::PointerMoved(pos)], &mut draw);
        assert_eq!(ctx.memory(|m| m.focused()), Some(first.id));
        frame(&ctx, pointer(pos, true), &mut draw);
        frame(&ctx, pointer(pos, false), &mut draw);
        assert_eq!(ctx.memory(|m| m.focused()), Some(target.id));
        let (_, response) = frame(&ctx, vec![key(Key::Enter)], &mut draw);
        assert!(
            response.clicked(),
            "mouse focus must also accept keyboard input"
        );
    }
}
