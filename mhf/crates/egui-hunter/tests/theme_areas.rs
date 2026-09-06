use egui::{
    Color32, Context, Id, InnerResponse, Margin, RawInput, Rect, Shape, Stroke, Ui, pos2, vec2,
};
use egui_hunter::{Button, Dialog, DialogState, Popup, RichTooltip, Theme, Tokens, Window};

#[derive(Clone, Copy, Debug)]
enum AreaKind {
    Window,
    Dialog,
    Popup,
    Tooltip,
}

#[derive(Debug)]
struct Probe {
    fill: Color32,
    background: Color32,
    padding: Margin,
    tokens: Tokens,
}

fn inspect(ui: &mut Ui) -> Probe {
    ui.label("Rendered contents");
    Probe {
        fill: ui.visuals().window_fill,
        background: ui.stack().bg_color(),
        padding: ui.spacing().window_margin,
        tokens: Tokens::get(ui),
    }
}

fn frame<R>(ctx: &Context, time: f64, mut content: impl FnMut(&mut Ui) -> R) -> (R, Vec<Shape>) {
    let mut result = None;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            time: Some(time),
            ..Default::default()
        },
        |ui| result = Some(content(ui)),
    );
    output.textures_delta.clear();
    (
        result.unwrap(),
        output.shapes.into_iter().map(|shape| shape.shape).collect(),
    )
}

fn find_panel(shapes: &[Shape], fill: Color32, cut: f32, stroke: Stroke) -> bool {
    shapes.iter().any(|shape| match shape {
        Shape::Vec(shapes) => find_panel(shapes, fill, cut, stroke),
        Shape::Path(path) if path.closed && path.points.len() == 8 && path.fill == fill => {
            let rect = Rect::from_points(&path.points);
            (path.points[0].x - rect.left() - cut).abs() < 0.1
                && (path.stroke.width - stroke.width).abs() < 0.1
                && path.stroke.color == egui::epaint::ColorMode::Solid(stroke.color)
        }
        _ => false,
    })
}

#[test]
fn independent_areas_use_global_defaults_unless_style_and_tokens_are_passed() {
    for kind in [
        AreaKind::Window,
        AreaKind::Dialog,
        AreaKind::Popup,
        AreaKind::Tooltip,
    ] {
        for inherit in [false, true] {
            let ctx = Context::default();
            let theme = Theme::default();
            theme.apply(&ctx);
            let fill = Color32::from_rgb(53, 70, 119);
            let stroke = Stroke::new(2.5, Color32::from_rgb(230, 120, 75));
            let padding = Margin::same(23);
            let local_tokens = Tokens {
                cut: 12.0,
                success: Color32::YELLOW,
                ..theme.tokens
            };
            let mut dialog = DialogState::default();
            let popup_id = Id::new("area-popup");
            if matches!(kind, AreaKind::Popup) {
                egui::Popup::open_id(&ctx, popup_id);
            }
            let mut last = None;
            for pass in 0..4 {
                last = Some(frame(&ctx, f64::from(pass) * 0.2, |ui| {
                    ui.scope(|ui| {
                        ui.visuals_mut().window_fill = fill;
                        ui.visuals_mut().window_stroke = stroke;
                        ui.spacing_mut().window_margin = padding;
                        local_tokens
                            .scope(ui, |ui| {
                                let anchor = ui.add(Button::new("anchor"));
                                let style = ui.style().clone();
                                let tokens = Tokens::get(ui);
                                let output: Option<InnerResponse<Probe>> = match kind {
                                    AreaKind::Window => {
                                        let mut window = Window::new("Area");
                                        window.native = window.native.fixed_pos(pos2(200.0, 160.0));
                                        if inherit {
                                            window = window.style(style).tokens(tokens);
                                        }
                                        window.show(ui.ctx(), inspect)
                                    }
                                    AreaKind::Dialog => {
                                        if pass == 0 {
                                            dialog.open_from(&anchor);
                                        }
                                        let mut view = Dialog::new(Id::new("area-dialog"), "Area");
                                        if inherit {
                                            view = view.style(style).tokens(tokens);
                                        }
                                        view.show(ui.ctx(), &mut dialog, inspect)
                                    }
                                    AreaKind::Popup => {
                                        let mut popup = Popup::new(&anchor).title("Area");
                                        popup.native = popup.native.id(popup_id);
                                        if inherit {
                                            popup = popup.style(style).tokens(tokens);
                                        }
                                        popup.show(inspect)
                                    }
                                    AreaKind::Tooltip => {
                                        anchor.request_focus();
                                        let mut tooltip = RichTooltip::new(&anchor, "Area");
                                        if inherit {
                                            tooltip = tooltip.style(style).tokens(tokens);
                                        }
                                        tooltip.show(inspect)
                                    }
                                };
                                output.expect("area is visible").inner
                            })
                            .inner
                    })
                    .inner
                }));
            }
            let (probe, shapes) = last.unwrap();
            let (expected_fill, expected_stroke, expected_padding, expected_tokens) = if inherit {
                (fill, stroke, padding, local_tokens)
            } else {
                (
                    theme.style.visuals.window_fill,
                    theme.style.visuals.window_stroke,
                    theme.style.spacing.window_margin,
                    theme.tokens,
                )
            };
            assert_eq!(probe.fill, expected_fill, "{kind:?}, inherit={inherit}");
            assert_eq!(
                probe.background, expected_fill,
                "native descendants see the painted panel background"
            );
            assert_eq!(probe.padding, expected_padding);
            assert_eq!(probe.tokens, expected_tokens);
            assert!(
                find_panel(&shapes, expected_fill, expected_tokens.cut, expected_stroke),
                "{kind:?}, inherit={inherit}: the actual panel shape must use the injected style and tokens"
            );
            assert_eq!(
                ctx.global_style().visuals.window_fill,
                theme.style.visuals.window_fill
            );
            assert_eq!(Tokens::from_context(&ctx), theme.tokens);
        }
    }
}
