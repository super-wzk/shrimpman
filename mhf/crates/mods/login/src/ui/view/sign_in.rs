use super::*;
use egui::{FontId, Rect, Sense, Stroke, pos2, vec2};

pub(super) fn show(
    state: &mut SignIn,
    password_visible: &mut bool,
    ui: &mut egui::Ui,
) -> Option<Message> {
    if ui.available_width() >= 580.0 {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let height = ui.available_height();
            let (brand, _) = ui.allocate_exact_size(vec2(180.0, height), Sense::hover());
            paint_brand(ui, brand, false);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::top_down(Align::Min),
                |ui| form(state, password_visible, ui),
            )
            .inner
        })
        .inner
    } else {
        let (brand, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        paint_brand(ui, brand, true);
        form(state, password_visible, ui)
    }
}

fn form(state: &mut SignIn, password_visible: &mut bool, ui: &mut egui::Ui) -> Option<Message> {
    let mut message = None;
    let height_id = Id::new("sign_in_content_height");
    let previous_height = ui.ctx().data(|data| data.get_temp::<f32>(height_id));
    let content_height = previous_height.unwrap_or(ui.available_height());
    let top = ((ui.available_height() - content_height) * 0.5).max(8.0);
    egui::ScrollArea::vertical()
        .id_salt("sign_in")
        .min_scrolled_height(0.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(top);
            let content = ui.vertical_centered(|ui| {
                let width = (ui.available_width() - 40.0).clamp(120.0, 360.0);
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("登录").size(22.0).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(action) = settings::menu(ui, !state.submitting) {
                                message = Some(action);
                            }
                        });
                    });
                    let username_id = Id::new("sign_in_username");
                    let password_id = Id::new("sign_in_password");
                    let username = ui.add_enabled(
                        !state.submitting,
                        TextField::new(username_id, &mut state.form.username)
                            .label("用户名")
                            .hint("输入用户名"),
                    );
                    let autofocus = Id::new("sign_in_username_autofocus");
                    if !state.submitting
                        && !ui
                            .ctx()
                            .data(|data| data.get_temp::<bool>(autofocus).unwrap_or(false))
                    {
                        username.request_focus();
                        username.scroll_to_me(None);
                        ui.ctx().data_mut(|data| data.insert_temp(autofocus, true));
                    }
                    ui.add_space(4.0);
                    ui.add_enabled(
                        !state.submitting,
                        TextField::new(password_id, &mut state.form.password)
                            .label("密码")
                            .hint("输入密码")
                            .password_visible(password_visible),
                    );
                    ui.add_enabled(
                        !state.submitting,
                        Checkbox::new(&mut state.form.remember_password, "记住密码"),
                    );
                    ui.add_space(4.0);
                    let submit_from_field = ui.memory(|memory| {
                        memory.has_focus(username_id) || memory.has_focus(password_id)
                    }) && ui
                        .input(|input| input.key_pressed(egui::Key::Enter));
                    let mut submit = Button::new(if state.submitting {
                        "正在登录…"
                    } else {
                        "登录"
                    })
                    .id(Id::new("sign_in_submit"))
                    .kind(ButtonKind::Primary)
                    .min_size(vec2(width, 44.0));
                    if !state.submitting {
                        submit = submit.icon(Icon::LogIn);
                    }
                    let can_submit = state.can_submit();
                    if ui.add_enabled(can_submit, submit).clicked()
                        || (can_submit && submit_from_field)
                    {
                        message = Some(Message::SignIn);
                    }
                });
            });
            ui.add_space(8.0);
            let height = content.response.rect.height();
            if previous_height.is_none_or(|previous| (previous - height).abs() > 0.5) {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(height_id, height));
                ui.ctx().request_discard("sign-in content resized");
            }
        });
    message
}

fn paint_brand(ui: &egui::Ui, rect: Rect, compact: bool) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, ui.visuals().window_fill());
    let tokens = Tokens::get(ui);
    let text_x = rect.left() + 20.0;
    let text_y = rect.top() + if compact { 8.0 } else { 24.0 };
    painter.text(
        pos2(text_x, text_y),
        egui::Align2::LEFT_TOP,
        "MONSTER HUNTER",
        FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        pos2(text_x, text_y + 19.0),
        egui::Align2::LEFT_TOP,
        "FRONTIER",
        FontId::proportional(if compact { 18.0 } else { 22.0 }),
        tokens.primary,
    );
    let center = if compact {
        pos2(rect.right() - 42.0, rect.center().y)
    } else {
        pos2(rect.center().x, rect.top() + rect.height() * 0.61)
    };
    let radius = if compact { 18.0 } else { 60.0 };
    let line = Stroke::new(
        1.0,
        ui.visuals()
            .window_fill()
            .lerp_to_gamma(tokens.primary, 0.18),
    );
    painter.circle_stroke(center, radius, line);
    painter.circle_stroke(center, radius * 0.82, line);
    let points = [
        (-0.58, -0.22),
        (-0.32, -0.62),
        (0.0, -0.36),
        (0.32, -0.62),
        (0.58, -0.22),
        (0.42, 0.42),
        (0.0, 0.72),
        (-0.42, 0.42),
        (-0.58, -0.22),
    ];
    painter.add(egui::Shape::line(
        points
            .into_iter()
            .map(|(x, y)| center + vec2(x, y) * radius)
            .collect(),
        line,
    ));
    for side in [-1.0, 1.0] {
        painter.line_segment(
            [
                center + vec2(side * radius * 0.32, -radius * 0.18),
                center + vec2(0.0, radius * 0.45),
            ],
            line,
        );
    }
}
