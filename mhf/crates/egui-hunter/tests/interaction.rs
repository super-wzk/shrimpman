mod events;

use egui::{Context, Event, Id, Key, Modifiers, RawInput, Rect, pos2, vec2};
use egui_hunter::Theme;
use events::{key, pointer};

fn frame<R>(ctx: &Context, events: Vec<Event>, mut show: impl FnMut(&mut egui::Ui) -> R) -> R {
    let mut result = None;
    let mut output = ctx.run_ui(
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
    // These interaction tests do not upload font textures to a renderer.
    output.textures_delta.clear();
    result.expect("UI should be drawn")
}

#[test]
fn checkbox_changes_on_release_and_reports_changed() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let mut checked = false;
    let response = frame(&ctx, vec![], |ui| {
        ui.add(theme.checkbox(&mut checked, "提示"))
    });
    let pos = response.rect.center();
    frame(&ctx, pointer(pos, true), |ui| {
        ui.add(theme.checkbox(&mut checked, "提示"))
    });
    assert!(!checked, "pressing alone must not change the value");
    let response = frame(&ctx, pointer(pos, false), |ui| {
        ui.add(theme.checkbox(&mut checked, "提示"))
    });
    assert!(checked);
    assert!(response.changed());
}

#[test]
fn disabled_toggle_rejects_pointer_and_keyboard_activation() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut checked = false;
    let response = frame(&ctx, vec![], |ui| {
        ui.add_enabled(false, theme.toggle(&mut checked, "整理"))
    });
    let pos = response.rect.center();
    ctx.memory_mut(|m| m.request_focus(response.id));
    frame(&ctx, pointer(pos, true), |ui| {
        ui.add_enabled(false, theme.toggle(&mut checked, "整理"))
    });
    let mut events = pointer(pos, false);
    events.push(key(Key::Space));
    let response = frame(&ctx, events, |ui| {
        ui.add_enabled(false, theme.toggle(&mut checked, "整理"))
    });
    assert!(!checked);
    assert!(!response.changed());
    assert!(!response.clicked());
}

#[test]
fn focused_controls_support_space_and_enter() {
    let ctx = Context::default();
    let theme = Theme::default();
    let mut checked = false;
    let response = frame(&ctx, vec![], |ui| {
        ui.add(theme.toggle(&mut checked, "整理"))
    });
    ctx.memory_mut(|m| m.request_focus(response.id));
    let response = frame(&ctx, vec![key(Key::Space)], |ui| {
        ui.add(theme.toggle(&mut checked, "整理"))
    });
    assert!(checked);
    assert!(response.changed());

    let response = frame(&ctx, vec![], |ui| ui.add(theme.button("确认")));
    ctx.memory_mut(|m| m.request_focus(response.id));
    let response = frame(&ctx, vec![key(Key::Enter)], |ui| {
        ui.add(theme.button("确认"))
    });
    assert!(response.clicked());
}

#[test]
fn tab_navigation_skips_disabled_controls() {
    let ctx = Context::default();
    let theme = Theme::default();
    let draw = |ui: &mut egui::Ui| {
        let first = ui.add(theme.button("第一项"));
        let disabled = ui.add_enabled(false, theme.button("禁用项"));
        let last = ui.add(theme.button("第三项"));
        [first.id, disabled.id, last.id]
    };
    let ids = frame(&ctx, vec![], draw);
    // Tab order starts from no focus, then reaches the two enabled widgets.
    frame(&ctx, vec![key(Key::Tab)], draw);
    let first_focus = ctx.memory(|m| m.focused());
    frame(
        &ctx,
        vec![Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        draw,
    );
    frame(&ctx, vec![key(Key::Tab)], draw);
    assert_eq!(first_focus, Some(ids[0]));
    assert_eq!(ctx.memory(|m| m.focused()), Some(ids[2]));
    assert_ne!(ctx.memory(|m| m.focused()), Some(ids[1]));
}

#[test]
fn styled_text_input_retains_unicode_editing_and_stable_identity() {
    let ctx = Context::default();
    let theme = Theme::default();
    theme.apply(&ctx);
    let mut first = String::new();
    let mut second = String::new();
    let target = Id::new("second-search");
    let mut draw = |ui: &mut egui::Ui| {
        theme.text_edit(ui, Id::new("first-search"), &mut first, "搜索");
        theme.text_edit(ui, target, &mut second, "搜索")
    };
    frame(&ctx, vec![], &mut draw);
    ctx.memory_mut(|m| m.request_focus(target));
    frame(&ctx, vec![Event::Text("回复药".into())], &mut draw);
    assert!(first.is_empty());
    assert_eq!(second, "回复药");
}
