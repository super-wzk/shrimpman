pub mod events;

use egui::{Context, Event, FullOutput, Id, Key, Pos2, Shape, Ui, vec2};
use egui_hunter::{Density, Icon, IconButton, Theme};

fn frame<R>(
    ctx: &Context,
    events: Vec<Event>,
    mut show: impl FnMut(&mut Ui) -> R,
) -> (R, FullOutput) {
    events::frame(ctx, events::input(vec2(800.0, 600.0), None, events), |ui| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, &mut show)
            .inner
    })
}

#[test]
fn icon_actions_expose_accessible_names_roles_and_selection() {
    use egui::accesskit::{Action, Role, Toggled};

    let ctx = Context::default();
    ctx.enable_accesskit();
    Theme::default().apply(&ctx);
    let (responses, output) = frame(&ctx, vec![], |ui| {
        [
            ui.add(IconButton::new(Icon::Apply, "Apply project").id(Id::new("apply"))),
            ui.add(
                IconButton::new(Icon::Eye, "Show trace")
                    .id(Id::new("trace"))
                    .selected(true),
            ),
        ]
    });
    let tree = output.platform_output.accesskit_update.unwrap();
    for (response, label) in responses.iter().zip(["Apply project", "Show trace"]) {
        let node = &tree
            .nodes
            .iter()
            .find(|(id, _)| *id == response.id.accesskit_id())
            .unwrap()
            .1;
        assert_eq!(node.role(), Role::Button);
        assert_eq!(node.label(), Some(label));
        assert!(node.supports_action(Action::Click));
        assert!(node.supports_action(Action::Focus));
    }
    let trace = &tree
        .nodes
        .iter()
        .find(|(id, _)| *id == responses[1].id.accesskit_id())
        .unwrap()
        .1;
    assert_eq!(trace.toggled(), Some(Toggled::True));
}

#[test]
fn hover_press_and_focus_preserve_geometry_and_show_a_focus_tooltip() {
    for density in [Density::Standard, Density::Compact] {
        let ctx = Context::default();
        Theme::default().density(density).apply(&ctx);
        let id = Id::new("apply");
        let mut draw = |ui: &mut Ui| {
            ui.visuals_mut().widgets.hovered.expansion = 5.0;
            ui.visuals_mut().widgets.active.expansion = 8.0;
            ui.add(IconButton::new(Icon::Apply, "Apply project").id(id))
        };
        let (initial, _) = frame(&ctx, vec![], &mut draw);
        let point = initial.rect.center();
        let (hovered, hover_output) = frame(&ctx, vec![Event::PointerMoved(point)], &mut draw);
        assert!(hovered.hovered());
        let (pressed, press_output) = frame(&ctx, events::pointer(point, true), &mut draw);
        assert!(pressed.is_pointer_button_down_on());
        frame(
            &ctx,
            events::pointer(Pos2::new(-10.0, -10.0), false),
            &mut draw,
        );
        initial.request_focus();
        let (focused, focus_output) = frame(&ctx, vec![], &mut draw);
        assert!(focused.has_focus());
        for (response, output) in [
            (hovered, hover_output),
            (pressed, press_output),
            (focused, focus_output),
        ] {
            assert_eq!(response.rect, initial.rect);
            let surface = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    Shape::Rect(rect)
                        if rect.rect.contains(initial.rect.center())
                            && (rect.fill != egui::Color32::TRANSPARENT
                                || rect.stroke.width > 0.0) =>
                    {
                        Some(rect)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(surface.rect, initial.rect);
            assert_eq!(surface.stroke_kind, egui::StrokeKind::Inside);
        }
        // Tooltip does not create a selectable/focusable label or open a menu.
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(id));
        assert!(!egui::Popup::is_any_open(&ctx));
        let (_, tooltip_output) = frame(&ctx, vec![], &mut draw);
        assert!(tooltip_output.shapes.iter().any(|shape| matches!(
            &shape.shape, Shape::Text(text) if text.galley.job.text == "Apply project"
        )));
    }
}

#[test]
fn icon_actions_participate_in_native_tab_order() {
    let ctx = events::themed_context();
    let draw = |ui: &mut Ui| {
        [
            ui.add(IconButton::new(Icon::Apply, "Apply project").id(Id::new("apply"))),
            ui.add(IconButton::new(Icon::Save, "Save project").id(Id::new("save"))),
        ]
    };
    let (initial, _) = frame(&ctx, vec![], draw);
    initial[0].request_focus();
    frame(&ctx, events::pulse(Key::Tab), draw);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(initial[1].id));
}

#[test]
fn disabled_icon_actions_keep_their_hover_labels() {
    let ctx = events::themed_context();
    ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    let mut draw =
        |ui: &mut Ui| ui.add_enabled(false, IconButton::new(Icon::Pause, "Pause execution"));
    let (disabled, _) = frame(&ctx, vec![], &mut draw);
    frame(
        &ctx,
        vec![Event::PointerMoved(disabled.rect.center())],
        &mut draw,
    );
    frame(&ctx, vec![], &mut draw);
    let (hovered, output) = frame(&ctx, vec![], &mut draw);
    assert!(!hovered.enabled());
    assert!(!hovered.hovered());
    assert!(hovered.contains_pointer());
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape, Shape::Text(text) if text.galley.job.text == "Pause execution"
    )));
}
