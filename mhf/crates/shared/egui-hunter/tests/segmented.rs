pub mod events;

use egui::{Context, Event, Key, Rect, Shape, vec2};
use egui_hunter::{Segment, SegmentedControl};

struct Harness {
    context: Context,
    selected: Option<u8>,
    enabled: bool,
    labels: Vec<Rect>,
}

impl Harness {
    fn new() -> Self {
        let context = events::themed_context();
        Self {
            context,
            selected: None,
            enabled: true,
            labels: Vec::new(),
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> bool {
        let mut changed = false;
        let (_, output) = events::frame(
            &self.context,
            events::input(vec2(400.0, 200.0), None, events),
            |ui| {
                ui.add_enabled_ui(self.enabled, |ui| {
                    changed = SegmentedControl::new(egui::Id::new("styles"))
                        .show(
                            ui,
                            &mut self.selected,
                            &[
                                Segment::new(0, "Earth"),
                                Segment::new(1, "Heaven").enabled(false),
                                Segment::new(2, "Storm"),
                            ],
                        )
                        .changed();
                });
            },
        );
        self.labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                Shape::Text(text) => Some(Rect::from_min_size(text.pos, text.galley.size())),
                _ => None,
            })
            .collect();
        changed
    }

    fn click(&mut self, index: usize) -> bool {
        self.frame(vec![]);
        let pos = self.labels[index].center();
        self.frame(events::pointer(pos, true));
        self.frame(events::pointer(pos, false))
    }

    fn key(&mut self, key: Key) -> bool {
        let changed = self.frame(vec![events::key(key)]);
        self.frame(vec![events::key_event(key, false)]);
        changed
    }
}

#[test]
fn clicks_only_report_changes_and_disabled_choices_do_not_select() {
    let mut ui = Harness::new();
    assert!(!ui.frame(vec![]));
    assert_eq!(ui.selected, None);
    assert!(!ui.click(1));
    assert!(ui.click(0));
    assert_eq!(ui.selected, Some(0));
    assert!(!ui.click(0));
    ui.enabled = false;
    assert!(!ui.click(2));
    assert_eq!(ui.selected, Some(0));
}

#[test]
fn arrows_skip_disabled_choices_wrap_and_keep_native_keyboard_activation() {
    let mut ui = Harness::new();
    ui.click(0);
    assert!(ui.key(Key::ArrowRight));
    assert_eq!(ui.selected, Some(2));
    assert!(ui.key(Key::ArrowRight));
    assert_eq!(ui.selected, Some(0));
    assert!(ui.key(Key::ArrowLeft));
    assert_eq!(ui.selected, Some(2));
    ui.selected = None;
    assert!(ui.key(Key::Enter));
    assert_eq!(ui.selected, Some(2));
}
