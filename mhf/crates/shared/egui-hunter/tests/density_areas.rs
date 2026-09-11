use egui::{Align2, Context, Id, InnerResponse, RawInput, Rect, Ui, pos2, vec2};
use egui_hunter::{
    Button, ButtonKind, Density, Dialog, DialogState, NoticeKind, Notifications, Popup,
    ResponsiveColumns, RichTooltip, Theme, Tokens, Window,
};

fn frame<R>(ctx: &Context, time: f64, mut content: impl FnMut(&mut Ui) -> R) -> R {
    let mut result = None;
    ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            time: Some(time),
            ..Default::default()
        },
        |ui| result = Some(content(ui)),
    )
    .drop_without_applying_deltas();
    result.unwrap()
}

#[derive(Clone, Copy, Debug)]
enum AreaKind {
    Window,
    Dialog,
    Popup,
    Tooltip,
}

fn probe(ui: &mut Ui) -> (Density, f32, f32) {
    let primary = ui.add(Button::new("Action").kind(ButtonKind::Primary));
    (
        Density::get(ui),
        ui.spacing().interact_size.y,
        primary.rect.height(),
    )
}

#[test]
fn explicit_area_inheritance_carries_density_with_native_style() {
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
            let global = ctx.global_style();
            let mut dialog = DialogState::default();
            let popup_id = Id::new("density-popup");
            if matches!(kind, AreaKind::Popup) {
                egui::Popup::open_id(&ctx, popup_id);
            }
            let mut actual = None;
            for pass in 0..4 {
                actual = Some(frame(&ctx, pass as f64 * 0.2, |ui| {
                    let result = Density::Compact
                        .scope(ui, |ui| {
                            let anchor = ui.add(Button::new("Anchor"));
                            let style = ui.style().clone();
                            let tokens = Tokens::get(ui);
                            let output: Option<InnerResponse<(Density, f32, f32)>> = match kind {
                                AreaKind::Window => {
                                    let mut view = Window::new("Window");
                                    view.native = view.native.fixed_pos(pos2(200.0, 160.0));
                                    if inherit {
                                        view = view.style(style).tokens(tokens);
                                    }
                                    view.show(ui.ctx(), probe)
                                }
                                AreaKind::Dialog => {
                                    if pass == 0 {
                                        dialog.open_from(&anchor);
                                    }
                                    let mut view = Dialog::new(Id::new("density-dialog"), "Dialog");
                                    if inherit {
                                        view = view.style(style).tokens(tokens);
                                    }
                                    view.show(ui.ctx(), &mut dialog, probe)
                                }
                                AreaKind::Popup => {
                                    let mut view = Popup::new(&anchor);
                                    view.native = view.native.id(popup_id);
                                    if inherit {
                                        view = view.style(style).tokens(tokens);
                                    }
                                    view.show(probe)
                                }
                                AreaKind::Tooltip => {
                                    anchor.request_focus();
                                    let mut view = RichTooltip::new(&anchor, "Tooltip");
                                    if inherit {
                                        view = view.style(style).tokens(tokens);
                                    }
                                    view.show(probe)
                                }
                            };
                            output.expect("visible Area").inner
                        })
                        .inner;
                    assert_eq!(Density::get(ui), Density::Standard);
                    result
                }));
            }
            let expected = if inherit {
                (Density::Compact, 24.0, 28.0)
            } else {
                (Density::Standard, 36.0, 44.0)
            };
            assert_eq!(actual.unwrap(), expected, "{kind:?}, inherit={inherit}");
            assert_eq!(*ctx.global_style(), *global);
            assert_eq!(Tokens::from_context(&ctx), theme.tokens);
        }
    }
}

#[test]
fn floating_notifications_can_inherit_local_density_without_changing_global_toasts() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    let mut compact = Notifications::new(Id::new("compact-notice"));
    let mut standard = Notifications::new(Id::new("standard-notice"));
    compact.push(&ctx, NoticeKind::Warning, "Notice");
    standard.push(&ctx, NoticeKind::Warning, "Notice");
    let mut heights = (0.0, 0.0);
    for pass in 0..4 {
        heights = frame(&ctx, pass as f64 * 0.1, |ui| {
            let compact_height = Density::Compact
                .scope(ui, |ui| {
                    compact
                        .show_at_in(ui, Align2::LEFT_BOTTOM, vec2(20.0, -20.0))
                        .unwrap()
                        .response
                        .rect
                        .height()
                })
                .inner;
            let standard_height = standard
                .show_at(ui.ctx(), Align2::RIGHT_BOTTOM, vec2(-20.0, -20.0))
                .unwrap()
                .response
                .rect
                .height();
            assert_eq!(Density::get(ui), Density::Standard);
            (compact_height, standard_height)
        });
    }
    assert!(
        heights.0 > 0.0 && heights.0 < heights.1,
        "notification heights: {heights:?}"
    );
}

#[test]
fn responsive_column_gap_follows_style_and_explicit_gap_remains_authoritative() {
    let ctx = Context::default();
    Theme::default().apply(&ctx);
    for (density, explicit, expected) in [
        (Density::Standard, None, 16.0),
        (Density::Compact, None, 12.0),
        (Density::Compact, Some(9.0), 9.0),
    ] {
        let rectangles = frame(&ctx, 0.0, |ui| {
            density
                .scope(ui, |ui| {
                    ui.set_width(800.0);
                    let mut columns =
                        ResponsiveColumns::new(Id::new("density-columns")).min_column_width(200.0);
                    if let Some(gap) = explicit {
                        columns = columns.gap(gap);
                    }
                    columns
                        .show(ui, 2, |ui, _| {
                            let rect = ui.max_rect();
                            ui.label("Column");
                            rect
                        })
                        .inner
                })
                .inner
        });
        assert!((rectangles[1].left() - rectangles[0].right() - expected).abs() < 0.01);
    }
}

#[test]
fn a_compact_host_theme_and_standard_local_scope_keep_fonts_and_colors() {
    let ctx = Context::default();
    let standard = Theme::default();
    let compact = standard.clone().density(Density::Compact);
    assert_eq!(standard.style.text_styles, compact.style.text_styles);
    assert_eq!(standard.style.visuals, compact.style.visuals);
    compact.apply(&ctx);
    frame(&ctx, 0.0, |ui| {
        assert_eq!(Density::get(ui), Density::Compact);
        assert_eq!(ui.spacing().interact_size.y, 24.0);
        Density::Standard.scope(ui, |ui| {
            assert_eq!(Density::get(ui), Density::Standard);
            assert_eq!(ui.spacing().interact_size.y, 36.0);
        });
        assert_eq!(Density::get(ui), Density::Compact);
        assert_eq!(ui.spacing().interact_size.y, 24.0);
    });
}
