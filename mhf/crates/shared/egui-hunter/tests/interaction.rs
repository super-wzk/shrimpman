pub mod events;

use egui::{Context, Event, Id, Key, vec2};
use egui_hunter::{Button, Checkbox, Icon, IconButton, ItemSlot, Toggle};
use events::{pointer, pulse};

fn frame<R>(ctx: &Context, events: Vec<Event>, mut show: impl FnMut(&mut egui::Ui) -> R) -> R {
    events::frame(ctx, events::input(vec2(640.0, 480.0), None, events), |ui| {
        egui::CentralPanel::default().show(ui, &mut show).inner
    })
    .0
}

#[test]
fn styled_controls_activate_on_release_and_keyboard_without_stealing_hover_focus() {
    for kind in 0..5 {
        for enabled in [true, false] {
            let ctx = events::themed_context();
            let mut checked = false;
            let mut draw = |ui: &mut egui::Ui| {
                let first = ui.add(Button::new("first").id(Id::new("first")));
                let target = match kind {
                    0 => ui.add_enabled(enabled, Button::new("target")),
                    1 => ui.add_enabled(enabled, Checkbox::new(&mut checked, "target")),
                    2 => ui.add_enabled(enabled, Toggle::new(&mut checked, "target")),
                    3 => ui.add_enabled(enabled, ItemSlot::new("target")),
                    _ => ui.add_enabled(enabled, IconButton::new(Icon::Apply, "target")),
                };
                (first, target, checked)
            };
            let (first, target, _) = frame(&ctx, vec![], &mut draw);
            first.request_focus();
            let pos = target.rect.center();
            frame(&ctx, vec![Event::PointerMoved(pos)], &mut draw);
            assert_eq!(ctx.memory(|m| m.focused()), Some(first.id));
            let (_, pressed, value) = frame(&ctx, pointer(pos, true), &mut draw);
            assert!(!pressed.clicked());
            assert!(!pressed.changed());
            assert!(!value, "pressing alone must not change the value");
            let (first, released, value) = frame(&ctx, pointer(pos, false), &mut draw);
            let choice = kind == 1 || kind == 2;
            assert_eq!(released.clicked(), enabled);
            assert_eq!(released.changed(), enabled && choice);
            assert_eq!(value, enabled && choice);
            assert!(!first.clicked());
            assert_eq!(released.rect, target.rect);
            if enabled {
                assert!(released.has_focus());
            } else {
                target.request_focus();
            }
            let mut previous = value;
            for key in [Key::Space, Key::Enter] {
                let (first, activated, value) = frame(&ctx, pulse(key), &mut draw);
                assert_eq!(activated.clicked(), enabled);
                assert_eq!(activated.changed(), enabled && choice);
                assert_eq!(
                    value,
                    if enabled && choice {
                        !previous
                    } else {
                        previous
                    }
                );
                assert_eq!(activated.has_focus(), enabled);
                assert!(!first.clicked());
                previous = value;
            }
        }
    }
}
