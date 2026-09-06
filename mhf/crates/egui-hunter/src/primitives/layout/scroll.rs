use crate::primitives::focus::navigation_allowed;
use egui::{Id, Key, Modifiers, Ui};

/// Add held Up/Down and PageUp/PageDown scrolling to a native ScrollArea's content.
/// The caller supplies the viewport's native focus target, for example a
/// UiBuilder::sense background. Native egui owns clipping and scroll state.
pub fn scroll_keyboard(ui: &mut Ui, focus: Id) {
    if !navigation_allowed(ui) || !ui.memory(|memory| memory.has_focus(focus)) {
        return;
    }
    let rect = ui.clip_rect().intersect(ui.max_rect());
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            focus,
            egui::EventFilter {
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
    let (up, down, step_up, step_down, page_up, page_down, dt) = ui.input_mut(|i| {
        (
            i.key_down(Key::ArrowUp),
            i.key_down(Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::PageUp),
            i.consume_key(Modifiers::NONE, Key::PageDown),
            i.stable_dt.min(0.05),
        )
    });
    if up || down || step_up || step_down {
        ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    }
    let distance = |held, pressed| {
        if held {
            480.0 * dt
        } else if pressed {
            36.0
        } else {
            0.0
        }
    };
    let delta = distance(up, step_up) - distance(down, step_down)
        + (i32::from(page_up) - i32::from(page_down)) as f32 * rect.height() * 0.9;
    if delta != 0.0 {
        ui.scroll_with_delta_animation(
            egui::vec2(0.0, delta),
            egui::style::ScrollAnimation::none(),
        );
    }
    if up || down {
        ui.ctx().request_repaint();
    }
}
