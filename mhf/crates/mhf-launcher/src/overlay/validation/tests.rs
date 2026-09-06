use super::*;
use egui::{PointerButton, RawInput, Rect, pos2, vec2};
use egui_hunter::{Direction, EngagementPlugin, GamepadState, NavigationInput, Theme};

struct Host {
    ctx: Context,
    page: ValidationPage,
    time: f64,
    size: Option<egui::Vec2>,
    background: bool,
    background_clicks: usize,
    navigation: NavigationInput,
    gamepad: GamepadState,
}

impl Default for Host {
    fn default() -> Self {
        let ctx = Context::default();
        ctx.add_plugin(EngagementPlugin::default());
        Self {
            ctx,
            page: ValidationPage::default(),
            time: 0.0,
            size: None,
            background: false,
            background_clicks: 0,
            navigation: NavigationInput::default(),
            gamepad: GamepadState::default(),
        }
    }
}

impl Host {
    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                self.size.unwrap_or(vec2(1280.0, 900.0)),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.navigation.apply(&self.ctx, &mut input, self.gamepad);
        let mut output = self.ctx.run_ui(input, |ui| {
            if self.background {
                let background = ui.put(
                    Rect::from_min_size(pos2(1100.0, 830.0), vec2(150.0, 44.0)),
                    Button::new("背景操作").id(Id::new("validation-background")),
                );
                if background.clicked() {
                    self.background_clicks += 1;
                }
            }
            self.page.show(ui.ctx());
        });
        output.textures_delta.clear();
    }

    fn press(&mut self, key: Key) {
        self.press_with_modifiers(key, Modifiers::NONE);
    }

    fn pad(&mut self, state: GamepadState) {
        self.gamepad = state;
        self.frame(vec![]);
        self.gamepad = GamepadState::default();
        self.frame(vec![]);
    }

    fn press_with_modifiers(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(
            [true, false]
                .map(|pressed| Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .into(),
        );
    }

    fn open(&mut self) {
        Theme::default().apply(&self.ctx);
        self.press(Key::F8);
        self.frame(vec![]);
        self.frame(vec![]);
        assert!(self.page.modal.is_open());
    }

    fn click(&mut self, id: impl egui::AsId) {
        let pos = self.ctx.read_response(Id::new(id)).unwrap().rect.center();
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
        self.frame(vec![]);
    }
}

fn key_event(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn f8_toggles_once_per_press_and_hidden_page_releases_keyboard() {
    let mut host = Host::default();
    host.frame(vec![]);
    assert!(!host.page.modal.is_open());
    assert_eq!(host.page.input_policy().pointer, InputCapture::PassThrough);
    host.frame(vec![key_event(Key::F8, true)]);
    assert!(host.page.modal.is_open());
    assert_eq!(host.page.input_policy().pointer, InputCapture::Block);
    assert_eq!(host.page.input_policy().keyboard, InputCapture::Block);
    for _ in 0..4 {
        host.frame(vec![key_event(Key::F8, true)]);
        assert!(host.page.modal.is_open(), "holding F8 must not flicker");
    }
    host.frame(vec![key_event(Key::F8, false)]);
    host.ctx
        .memory_mut(|memory| memory.request_focus(Id::new("validation-text")));
    host.frame(vec![]);
    assert!(host.ctx.egui_wants_keyboard_input());
    host.press(Key::F8);
    assert!(!host.page.modal.is_open());
    assert!(host.ctx.memory(|memory| memory.focused()).is_none());
    assert!(!host.ctx.egui_wants_keyboard_input());
    assert_eq!(host.page.input_policy().keyboard, InputCapture::PassThrough);
}

#[test]
fn page_modal_blocks_background_pointer_and_tab_and_restores_external_focus() {
    let mut host = Host {
        background: true,
        ..Default::default()
    };
    host.frame(vec![]);
    let background = Id::new("validation-background");
    host.ctx
        .memory_mut(|memory| memory.request_focus(background));
    host.frame(vec![]);
    host.open();
    for _ in 0..16 {
        host.press(Key::Tab);
        host.frame(vec![]);
        assert_ne!(host.ctx.memory(|memory| memory.focused()), Some(background));
    }
    host.click("validation-background");
    assert_eq!(host.background_clicks, 0);
    assert!(host.page.modal.is_open());
    assert_eq!(host.page.input_policy().pointer, InputCapture::Block);
    host.press(Key::Escape);
    assert!(!host.page.modal.is_open());
    for _ in 0..3 {
        host.frame(vec![]);
    }
    assert_eq!(host.ctx.memory(|memory| memory.focused()), Some(background));
    assert_eq!(host.page.input_policy().pointer, InputCapture::PassThrough);
    host.click("validation-background");
    assert_eq!(host.background_clicks, 1);
}

#[test]
fn opening_focuses_one_list_tab_stop_and_preserves_selection_across_reopening() {
    let mut host = Host::default();
    host.open();
    let list = host.ctx.memory(|memory| memory.focused()).unwrap();
    host.press(Key::End);
    assert_eq!(host.ctx.memory(|memory| memory.focused()), Some(list));
    assert_eq!(host.page.selected, 0);
    let list_rect = host.ctx.read_response(list).unwrap().rect;
    host.press(Key::Tab);
    host.frame(vec![]);
    let next = host.ctx.memory(|memory| memory.focused()).unwrap();
    assert!(!list_rect.intersects(host.ctx.read_response(next).unwrap().rect));
    host.press_with_modifiers(Key::Tab, Modifiers::SHIFT);
    host.frame(vec![]);
    assert_eq!(host.ctx.memory(|memory| memory.focused()), Some(list));
    let last_row = host
        .ctx
        .read_response(Id::new(("validation-record", RECORD_COUNT - 1)))
        .unwrap();
    assert!(last_row.interact_rect.contains_rect(last_row.rect));
    host.press(Key::Enter);
    assert_eq!(host.page.selected, RECORD_COUNT - 1);
    host.press(Key::F8);
    host.press(Key::F8);
    assert_eq!(host.page.selected, RECORD_COUNT - 1);
    assert_eq!(host.ctx.memory(|memory| memory.focused()), Some(list));
    host.press_with_modifiers(Key::Tab, Modifiers::SHIFT);
    host.frame(vec![]);
    assert_eq!(
        host.ctx.memory(|memory| memory.focused()),
        Some(Id::new("validation-close"))
    );
    host.press(Key::Enter);
    assert!(!host.page.modal.is_open());
}

#[test]
fn confirmation_emits_one_notice_and_restores_the_opener() {
    let mut host = Host::default();
    host.open();
    host.click("validation-open-dialog");
    assert!(host.page.dialog.is_open());
    assert_eq!(
        host.page.input_policy(),
        InputPolicy {
            pointer: InputCapture::Block,
            keyboard: InputCapture::Block,
        }
    );
    assert_eq!(
        host.ctx.memory(|m| m.focused()),
        Some(Id::new("validation-confirm"))
    );
    host.press(Key::Enter);
    assert!(!host.page.dialog.is_open());
    assert_eq!(host.page.input_policy().pointer, InputCapture::Block);
    assert_eq!(host.page.input_policy().keyboard, InputCapture::Block);
    host.frame(vec![]);
    assert_eq!(host.page.notices.len(), 1);
    assert_eq!(
        host.ctx.memory(|m| m.focused()),
        Some(Id::new("validation-open-dialog"))
    );
}

#[test]
fn hiding_dismisses_native_popup_modal_and_notifications() {
    let mut host = Host::default();
    host.open();
    host.click("validation-open-popup");
    assert!(egui::Popup::is_any_open(&host.ctx));
    host.press(Key::F8);
    assert!(!egui::Popup::is_any_open(&host.ctx));
    host.press(Key::F8);
    assert!(!egui::Popup::is_any_open(&host.ctx));
    host.frame(vec![]);
    host.click("validation-notify");
    assert_eq!(host.page.notices.len(), 2);
    host.click("validation-open-dialog");
    assert!(host.page.dialog.is_open());
    host.press(Key::F8);
    assert!(!host.page.dialog.is_open());
    assert!(host.page.notices.is_empty());
    assert!(host.ctx.memory(|m| m.focused()).is_none());
    host.press(Key::F8);
    host.frame(vec![]);
    assert!(!host.page.dialog.is_open());
    assert!(!egui::Popup::is_any_open(&host.ctx));
    assert_eq!(
        host.ctx.memory(|m| m.top_modal_layer()),
        Some(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("validation-page")
        ))
    );
}

#[test]
fn pointer_activates_controls_in_different_panels_with_one_click() {
    let mut host = Host::default();
    host.open();
    let list = host.ctx.memory(|memory| memory.focused()).unwrap();
    host.click(("validation-record", 3_usize));
    assert_eq!(host.page.selected, 3);
    assert_eq!(host.ctx.memory(|memory| memory.focused()), Some(list));
    host.click("validation-notify");
    assert_eq!(host.page.notices.len(), 2);
    assert_eq!(
        host.ctx.memory(|memory| memory.focused()),
        Some(Id::new("validation-notify"))
    );
    host.click("validation-text");
    host.frame(vec![Event::Text("直接编辑".into())]);
    assert!(host.page.text.contains("直接编辑"));
    let edited = host.page.text.clone();
    host.click("validation-open-dialog");
    assert!(host.page.dialog.is_open());
    host.press(Key::F8);
    host.press(Key::F8);
    assert_eq!(host.page.text, edited);
}

#[test]
fn native_text_cancel_ends_editing_before_the_next_press_closes_the_page() {
    let mut host = Host::default();
    host.open();
    host.click("validation-text");
    host.frame(vec![Event::Text("保留内容".into())]);
    let text = host.page.text.clone();
    host.frame(vec![key_event(Key::Escape, true)]);
    assert!(host.page.modal.is_open());
    assert!(host.ctx.memory(|memory| memory.focused()).is_none());
    host.frame(vec![key_event(Key::Escape, false)]);
    host.press(Key::Escape);
    assert!(!host.page.modal.is_open());
    assert_eq!(host.page.text, text);
    assert_eq!(host.page.input_policy().keyboard, InputCapture::PassThrough);
}

#[test]
fn escape_closes_child_overlays_before_the_page_and_does_not_repeat() {
    for opener in ["validation-open-popup", "validation-open-dialog"] {
        let mut host = Host::default();
        host.open();
        host.click(opener);
        host.frame(vec![key_event(Key::Escape, true)]);
        assert!(!egui::Popup::is_any_open(&host.ctx));
        assert!(!host.page.dialog.is_open());
        assert!(host.page.modal.is_open());
        assert_eq!(host.page.input_policy().keyboard, InputCapture::Block);
        for _ in 0..4 {
            host.frame(vec![key_event(Key::Escape, true)]);
            assert!(
                host.page.modal.is_open(),
                "held Escape must not dismiss the page"
            );
        }
        host.frame(vec![key_event(Key::Escape, false)]);
        host.press(Key::Escape);
        assert!(!host.page.modal.is_open());
    }
}

#[test]
fn narrow_viewport_stacks_panels_and_scrolls_to_feedback() {
    let mut host = Host {
        size: Some(vec2(640.0, 560.0)),
        ..Default::default()
    };
    host.open();
    let row = host
        .ctx
        .read_response(Id::new(("validation-record", 0_usize)))
        .unwrap();
    let text = host.ctx.read_response(Id::new("validation-text")).unwrap();
    assert!(text.rect.top() > row.rect.bottom());
    assert!(text.rect.right() <= 640.0);
    let window = host.ctx.read_response(Id::new("validation-page")).unwrap();
    let spacing = &host.ctx.global_style().spacing;
    let wheel_pos = window.rect.right_bottom()
        - vec2(
            f32::from(spacing.window_margin.right) + spacing.item_spacing.x,
            f32::from(spacing.window_margin.bottom) + spacing.item_spacing.y,
        );
    host.frame(vec![
        Event::PointerMoved(wheel_pos),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -1600.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]);
    for _ in 0..30 {
        host.frame(vec![]);
    }
    let notify = host
        .ctx
        .read_response(Id::new("validation-notify"))
        .unwrap();
    assert!(
        notify.interact_rect.contains_rect(notify.rect),
        "button {:?} must fit its interaction region {:?}",
        notify.rect,
        notify.interact_rect,
    );
    host.click("validation-notify");
    assert_eq!(host.page.notices.len(), 2);
}

#[test]
fn controller_navigation_reveals_frames_and_list_rows_in_a_narrow_viewport() {
    let mut host = Host {
        size: Some(vec2(640.0, 560.0)),
        ..Default::default()
    };
    host.open();
    for _ in 0..4 {
        host.pad(GamepadState {
            next_focus: true,
            ..Default::default()
        });
        for _ in 0..30 {
            host.frame(vec![]);
        }
        let focused = host.ctx.memory(|memory| memory.focused()).unwrap();
        let response = host.ctx.read_response(focused).unwrap();
        assert!(
            response
                .interact_rect
                .expand(1.0)
                .contains(response.rect.left_top() + vec2(1.0, 1.0)),
            "controller frame {focused:?} must reveal its header"
        );
    }
    assert_eq!(
        host.ctx.memory(|memory| memory.focused()),
        Some(Id::new("validation-records-region"))
    );
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    let list = host.ctx.memory(|memory| memory.focused()).unwrap();
    for _ in 0..23 {
        host.pad(GamepadState {
            direction: Some(Direction::Down),
            ..Default::default()
        });
    }
    assert_eq!(host.ctx.memory(|memory| memory.focused()), Some(list));
    host.frame(vec![]);
    let row = host
        .ctx
        .read_response(Id::new(("validation-record", 23_usize)))
        .unwrap();
    assert!(row.interact_rect.contains_rect(row.rect));
    assert_eq!(host.page.selected, 0);
    host.pad(GamepadState {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(host.page.selected, 23);
    host.pad(GamepadState {
        cancel: true,
        ..Default::default()
    });
    assert_eq!(
        host.ctx.memory(|memory| memory.focused()),
        Some(Id::new("validation-records-region"))
    );
    assert!(host.page.modal.is_open());
    host.pad(GamepadState {
        cancel: true,
        ..Default::default()
    });
    assert!(!host.page.modal.is_open());
}
