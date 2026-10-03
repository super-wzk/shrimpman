use egui::{
    Context, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, Vec2,
};

pub fn themed_context() -> Context {
    let ctx = Context::default();
    egui_hunter::Theme::default().apply(&ctx);
    ctx
}

pub fn input(size: Vec2, time: Option<f64>, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        time,
        events,
        ..Default::default()
    }
}

pub fn frame<R>(
    ctx: &Context,
    raw: RawInput,
    mut show: impl FnMut(&mut Ui) -> R,
) -> (R, FullOutput) {
    let mut result = None;
    let mut output = ctx.run_ui(raw, |ui| result = Some(show(ui)));
    output.textures_delta.clear();
    (result.unwrap(), output)
}

pub fn key(key: Key) -> Event {
    key_event(key, true)
}

pub fn pointer(pos: Pos2, pressed: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        },
    ]
}

pub fn pulse(key: Key) -> Vec<Event> {
    [true, false].map(|pressed| key_event(key, pressed)).into()
}

pub fn key_event(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}
