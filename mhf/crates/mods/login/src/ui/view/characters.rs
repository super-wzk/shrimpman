use super::*;

pub(super) fn show_characters(
    state: &Characters,
    encoding: SignEncoding,
    ui: &mut egui::Ui,
) -> Option<Message> {
    let mut message = None;
    let action_width = |label: &str| {
        egui::WidgetText::from(label)
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            )
            .size()
            .x
            + ui.spacing().button_padding.x * 2.0
            + ui.spacing().icon_width
            + ui.spacing().icon_spacing
    };
    let compact_actions = ui.available_width()
        < action_width("删除角色")
            + action_width("启动游戏").max(120.0)
            + ui.spacing().item_spacing.x;
    ui.horizontal_top(|ui| {
        let title_width = if compact_actions {
            (ui.available_width() - 36.0 - ui.spacing().item_spacing.x).max(0.0)
        } else {
            (ui.available_width() - 140.0).max(120.0)
        };
        ui.allocate_ui_with_layout(
            egui::vec2(title_width, 40.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("角色选择").size(20.0).strong());
                ui.add(
                    egui::Label::new(RichText::new(state.form.username.trim()).small().weak())
                        .truncate(),
                );
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if compact_actions {
                ui.spacing_mut().button_padding.x = 8.0;
                ui.spacing_mut().icon_spacing = 0.0;
            }
            let response = ui
                .add_enabled(
                    state.is_idle(),
                    Button::new(if compact_actions { "" } else { "退出登录" })
                        .id(Id::new("sign_out"))
                        .kind(ButtonKind::Quiet)
                        .icon(Icon::LogOut)
                        .min_size(egui::Vec2::splat(36.0)),
                )
                .on_hover_text("退出登录");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), "退出登录")
            });
            if response.clicked() {
                message = Some(Message::SignOut);
            }
        });
    });
    ui.add_space(8.0);

    egui::Panel::bottom("character_actions")
        .show_separator_line(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            ui.add_space(6.0);
            let status = match state.operation {
                CharacterOperation::CreatingCharacter => Some("正在创建角色…"),
                CharacterOperation::DeletingCharacter => Some("正在删除角色…"),
                _ => None,
            };
            if let Some(status) = status {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(status);
                });
            }
            ui.horizontal(|ui| {
                let deletion_target = state.selected_deletable_character_id();
                let delete = ui
                    .scope(|ui| {
                        if compact_actions {
                            ui.spacing_mut().button_padding.x = 8.0;
                            ui.spacing_mut().icon_spacing = 0.0;
                        }
                        ui.add_enabled(
                            deletion_target.is_some(),
                            Button::new(if compact_actions { "" } else { "删除角色" })
                                .id(Id::new("delete_character"))
                                .kind(ButtonKind::DangerQuiet)
                                .icon(Icon::Trash)
                                .min_size(if compact_actions {
                                    egui::Vec2::splat(36.0)
                                } else {
                                    egui::vec2(0.0, 44.0)
                                }),
                        )
                    })
                    .inner
                    .on_hover_text("删除角色");
                delete.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        delete.enabled(),
                        "删除角色",
                    )
                });
                if delete.clicked()
                    && let Some(character_id) = deletion_target
                {
                    message = Some(Message::DeleteCharacter(character_id));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let width = ui.available_width().clamp(120.0, 160.0);
                    if ui
                        .add_enabled(
                            state.can_launch(),
                            Button::new("启动游戏")
                                .id(Id::new("launch_game"))
                                .kind(ButtonKind::Primary)
                                .icon(Icon::ArrowUpRight)
                                .min_size(egui::vec2(width, 44.0)),
                        )
                        .clicked()
                    {
                        message = Some(Message::Launch);
                    }
                });
            });
        });

    if state.is_idle() && message.is_none() {
        let focused_control = ui.memory(|memory| memory.focused().is_some());
        let (up, down, enter, delete) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Delete),
            )
        });
        if (up || down)
            && let Some(selection) = adjacent_selection(state, down)
        {
            message = Some(Message::Select(selection));
        }
        if enter && !focused_control && state.can_launch() {
            message = Some(Message::Launch);
        }
        if delete && let Some(character_id) = state.selected_deletable_character_id() {
            message = Some(Message::DeleteCharacter(character_id));
        }
    }

    egui::ScrollArea::vertical()
        .id_salt("characters")
        .content_margin(egui::Margin {
            right: 12,
            ..egui::Margin::ZERO
        })
        .min_scrolled_height(0.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if state.sign_in.entrance_servers.is_empty() {
                notice(
                    ui,
                    NoticeKind::Warning,
                    "当前没有可用的 Entrance 服务，暂时无法启动游戏。",
                );
            }
            for character in state
                .sign_in
                .characters
                .iter()
                .filter(|character| !character.is_new)
            {
                let selection = CharacterSelection::Existing(character.id);
                let response = character_row(state, character, encoding, ui);
                if matches!(message, Some(Message::Select(target)) if target == selection) {
                    response.scroll_to_me(None);
                }
                if let Some(action) = card_message(state, selection, &response, state.is_idle()) {
                    message = Some(action);
                }
            }
            let response = ui.add_enabled(
                state.is_idle() && state.has_new_slot(),
                Button::new(if state.has_new_slot() {
                    "新建角色"
                } else {
                    "16 个角色槽位已用满"
                })
                .id(Id::new("new_character"))
                .kind(ButtonKind::Quiet)
                .icon(Icon::Plus)
                .selected(state.selection == CharacterSelection::New)
                .min_size(egui::vec2(0.0, 44.0))
                .full_width(),
            );
            if matches!(message, Some(Message::Select(CharacterSelection::New))) {
                response.scroll_to_me(None);
            }
            if let Some(action) = card_message(
                state,
                CharacterSelection::New,
                &response,
                state.is_idle() && state.has_new_slot(),
            ) {
                message = Some(action);
            }
        });
    message
}

fn character_row(
    state: &Characters,
    character: &SignCharacter,
    encoding: SignEncoding,
    ui: &mut egui::Ui,
) -> egui::Response {
    let selected = state.selection == CharacterSelection::Existing(character.id);
    let name = character_name(character, encoding);
    let gender = match character.gender {
        Gender::Male => "男",
        Gender::Female => "女",
    };
    let metadata = format!(
        "{}  ·  HR {}  ·  GR {}  ·  {gender}",
        weapon_name(character.weapon_type),
        character.hr,
        character.gr
    );
    let date = character
        .last_sign_in_at
        .map(|timestamp| format!("最近登录 {}", format_local_date(timestamp)));
    let wide = ui.available_width() >= 500.0;
    let height = if !wide && date.is_some() { 72.0 } else { 64.0 };
    ui.add_enabled_ui(state.is_idle(), |ui| {
        let response = ui.add(
            Button::new("")
                .id(Id::new(("character", u32::from(character.id))))
                .kind(ButtonKind::Quiet)
                .selected(selected)
                .min_size(egui::vec2(0.0, height))
                .full_width(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                response.enabled(),
                selected,
                format!(
                    "{name}，{metadata}{}",
                    date.as_ref()
                        .map(|value| format!("，{value}"))
                        .unwrap_or_default()
                ),
            )
        });
        let rect = response.rect;
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            if selected && !response.has_focus() {
                painter.rect_stroke(
                    rect,
                    ui.visuals().widgets.inactive.corner_radius,
                    egui::Stroke::new(1.0, Tokens::get(ui).primary),
                    egui::StrokeKind::Inside,
                );
            }
            weapon_icon(character.weapon_type).paint(
                &painter,
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 26.0, rect.center().y),
                    egui::Vec2::splat(28.0),
                ),
                egui::Color32::WHITE,
            );
            let text_left = rect.left() + 52.0;
            let date_width = if wide && date.is_some() { 136.0 } else { 0.0 };
            let details_width = (rect.right() - 36.0 - text_left).max(24.0);
            let title_width = (details_width - date_width).max(24.0);
            let title_y = rect.top() + if !wide && date.is_some() { 8.0 } else { 10.0 };
            row_text(
                ui,
                &painter,
                &name,
                15.0,
                ui.visuals().text_color(),
                egui::pos2(text_left, title_y),
                title_width,
            );
            row_text(
                ui,
                &painter,
                &metadata,
                12.0,
                ui.visuals().weak_text_color(),
                egui::pos2(text_left, title_y + 23.0),
                details_width,
            );
            if let Some(date) = date {
                let at = if wide {
                    egui::pos2(rect.right() - 36.0 - date_width, title_y + 2.0)
                } else {
                    egui::pos2(text_left, title_y + 41.0)
                };
                row_text(
                    ui,
                    &painter,
                    &date,
                    12.0,
                    ui.visuals().weak_text_color(),
                    at,
                    if wide { date_width } else { details_width },
                );
            }
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

fn row_text(
    ui: &egui::Ui,
    painter: &egui::Painter,
    text: &str,
    size: f32,
    color: egui::Color32,
    at: egui::Pos2,
    width: f32,
) {
    let galley = egui::WidgetText::from(RichText::new(text).size(size).color(color)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        width,
        egui::TextStyle::Body,
    );
    painter.galley(at, galley, color);
}

fn weapon_icon(weapon_type: WeaponType) -> Icon {
    match weapon_type {
        WeaponType::SwordAndShield => Icon::SwordAndShield,
        WeaponType::HeavyBowgun => Icon::HeavyBowgun,
        WeaponType::Hammer => Icon::Hammer,
        WeaponType::GreatSword => Icon::GreatSword,
        WeaponType::Lance => Icon::Lance,
        WeaponType::LightBowgun => Icon::LightBowgun,
        WeaponType::LongSword => Icon::LongSword,
        WeaponType::DualBlades => Icon::DualBlades,
        WeaponType::HuntingHorn => Icon::HuntingHorn,
        WeaponType::Gunlance => Icon::Gunlance,
        WeaponType::Bow => Icon::Bow,
        WeaponType::Tonfa => Icon::Tonfa,
        WeaponType::SwitchAxe => Icon::SwitchAxe,
        WeaponType::MagnetSpike => Icon::MagnetSpike,
    }
}

fn card_message(
    state: &Characters,
    selection: CharacterSelection,
    response: &egui::Response,
    interactive: bool,
) -> Option<Message> {
    if !interactive {
        return None;
    }
    let keyboard_activation = response.clicked()
        && response.has_focus()
        && response
            .ctx
            .input(|input| input.key_pressed(egui::Key::Enter));
    if (response.double_clicked() || keyboard_activation)
        && state.selection == selection
        && state.can_launch()
    {
        return Some(Message::Launch);
    }
    response.clicked().then_some(Message::Select(selection))
}

fn adjacent_selection(state: &Characters, forward: bool) -> Option<CharacterSelection> {
    let has_new_slot = state.has_new_slot();
    let selections = || {
        state
            .sign_in
            .characters
            .iter()
            .filter(|character| !character.is_new)
            .map(|character| CharacterSelection::Existing(character.id))
            .chain(has_new_slot.then_some(CharacterSelection::New))
    };
    let selection_count = selections().count();
    if selection_count == 0 {
        return None;
    }

    let current = selections().position(|selection| selection == state.selection);
    let index = match (current, forward) {
        (Some(index), true) => (index + 1).min(selection_count - 1),
        (Some(index), false) => index.saturating_sub(1),
        (None, _) => 0,
    };
    selections().nth(index)
}

fn weapon_name(weapon_type: WeaponType) -> &'static str {
    match weapon_type {
        WeaponType::SwordAndShield => "片手剑",
        WeaponType::HeavyBowgun => "重弩",
        WeaponType::Hammer => "大锤",
        WeaponType::GreatSword => "大剑",
        WeaponType::Lance => "长枪",
        WeaponType::LightBowgun => "轻弩",
        WeaponType::LongSword => "太刀",
        WeaponType::DualBlades => "双剑",
        WeaponType::HuntingHorn => "狩猎笛",
        WeaponType::Gunlance => "铳枪",
        WeaponType::Bow => "弓",
        WeaponType::Tonfa => "穿龙棍",
        WeaponType::SwitchAxe => "斩斧 F",
        WeaponType::MagnetSpike => "磁斩锤",
    }
}

fn format_local_date(timestamp: Timestamp) -> String {
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d")
        .to_string()
}

pub(super) fn character_name(character: &SignCharacter, encoding: SignEncoding) -> Cow<'_, str> {
    let end = character
        .name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(character.name.len());
    let bytes = &character.name[..end];
    if bytes.is_empty() {
        format!("角色 #{}", u32::from(character.id)).into()
    } else {
        encoding.decode(bytes)
    }
}
