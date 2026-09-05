use super::model::{CharacterOperation, CharacterSelection, Characters, Message, Model, SignIn};
use super::theme;
use jiff::Timestamp;
use shrimpman_domain::character::{Gender, WeaponType};
use shrimpman_mhf_launcher::SignCharacter;

pub(super) fn show(model: &mut Model, ui: &mut egui::Ui) -> Option<Message> {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(20, 16)),
        )
        .show(ui, |ui| match model {
            Model::SignIn(state) => show_sign_in(state, ui),
            Model::Characters(state) => show_characters(state, ui),
            Model::Closing => {
                show_closing(ui);
                None
            }
        })
        .inner
}

fn show_sign_in(state: &mut SignIn, ui: &mut egui::Ui) -> Option<Message> {
    let content_height = 348.0;
    ui.add_space(((ui.available_height() - content_height) * 0.5).max(0.0));

    let mut message = None;
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new("MONSTER HUNTER FRONTIER")
                .size(23.0)
                .strong()
                .color(theme::ACCENT),
        );
        ui.add_space(2.0);
        let (underline, _) = ui.allocate_exact_size(egui::vec2(64.0, 3.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(underline, 1.5, theme::ACCENT.gamma_multiply(0.8));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Sign in to choose your character")
                .size(13.0)
                .color(theme::TEXT_WEAK),
        );
        ui.add_space(22.0);

        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(20, 18))
            .show(ui, |ui| {
                ui.set_width(300.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.label(
                        egui::RichText::new("Username")
                            .size(13.0)
                            .color(theme::TEXT_WEAK),
                    );
                    let username = ui.add_enabled(
                        !state.submitting,
                        egui::TextEdit::singleline(&mut state.form.username)
                            .margin(theme::TEXT_EDIT_MARGIN)
                            .desired_width(f32::INFINITY),
                    );
                    let autofocus = ui.id().with("username_autofocus");
                    if !ui
                        .ctx()
                        .data(|data| data.get_temp::<bool>(autofocus).unwrap_or(false))
                    {
                        username.request_focus();
                        ui.ctx().data_mut(|data| data.insert_temp(autofocus, true));
                    }

                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("Password")
                            .size(13.0)
                            .color(theme::TEXT_WEAK),
                    );
                    ui.add_enabled(
                        !state.submitting,
                        egui::TextEdit::singleline(&mut state.form.password)
                            .password(true)
                            .margin(theme::TEXT_EDIT_MARGIN)
                            .desired_width(f32::INFINITY),
                    );

                    ui.add_space(10.0);
                    ui.add_enabled(
                        !state.submitting,
                        egui::Checkbox::new(&mut state.form.remember_password, "Remember password"),
                    );

                    ui.add_space(14.0);
                    let can_sign_in = state.can_submit();
                    let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let width = ui.available_width();
                    let clicked =
                        theme::primary_button(ui, "Sign in", can_sign_in, width).clicked();
                    if clicked || (can_sign_in && enter_pressed) {
                        message = Some(Message::SignIn);
                    }

                    if state.submitting {
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(16.0).color(theme::ACCENT));
                                ui.label(
                                    egui::RichText::new("Contacting sign service...")
                                        .size(12.5)
                                        .color(theme::TEXT_WEAK),
                                );
                            });
                        });
                    }

                    if let Some(error) = state.error.as_deref() {
                        ui.add_space(10.0);
                        theme::error_banner(ui, error);
                    }
                });
            });
    });
    message
}

fn show_characters(state: &Characters, ui: &mut egui::Ui) -> Option<Message> {
    let mut message = None;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Choose your character")
                    .size(20.0)
                    .strong(),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new(format!("Signed in as {}", state.form.username.trim()))
                    .size(12.0)
                    .color(theme::TEXT_WEAK),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(state.is_idle(), egui::Button::new("Sign out"))
                .clicked()
            {
                message = Some(Message::SignOut);
            }
        });
    });
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if state.sign_in.entrance_servers.is_empty() {
        theme::warning_banner(ui, "No Entrance service is currently available.");
        ui.add_space(4.0);
    }
    if let Some(error) = state.error.as_deref() {
        theme::error_banner(ui, error);
        ui.add_space(4.0);
    }

    egui::Panel::bottom("character_actions")
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme::BG))
        .show(ui, |ui| {
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let deletion_target = state.selected_deletable_character_id();
                if ui
                    .add_enabled(
                        deletion_target.is_some(),
                        egui::Button::new(egui::RichText::new("Delete").color(theme::ERROR_TEXT)),
                    )
                    .clicked()
                    && let Some(character_id) = deletion_target
                {
                    message = Some(Message::DeleteCharacter(character_id));
                }

                let status = match state.operation {
                    CharacterOperation::CreatingCharacter => Some("Creating character..."),
                    CharacterOperation::DeletingCharacter => Some("Deleting character..."),
                    _ => None,
                };
                if let Some(status) = status {
                    ui.add_space(6.0);
                    ui.add(egui::Spinner::new().size(16.0).color(theme::ACCENT));
                    ui.label(
                        egui::RichText::new(status)
                            .size(12.5)
                            .color(theme::TEXT_WEAK),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, "Launch game", state.can_launch(), 150.0).clicked()
                    {
                        message = Some(Message::Launch);
                    }
                });
            });
            ui.add_space(2.0);
        });

    let (up, down, enter, delete) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::ArrowUp),
            input.key_pressed(egui::Key::ArrowDown),
            input.key_pressed(egui::Key::Enter),
            input.key_pressed(egui::Key::Delete),
        )
    });
    if state.is_idle() {
        if (up || down)
            && let Some(selection) = adjacent_selection(state, down)
        {
            message = Some(Message::Select(selection));
        }
        if enter && state.can_launch() {
            message = Some(Message::Launch);
        }
        if delete && let Some(character_id) = state.selected_deletable_character_id() {
            message = Some(Message::DeleteCharacter(character_id));
        }
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for character in state
                .sign_in
                .characters
                .iter()
                .filter(|character| !character.is_new)
            {
                if let Some(card_message) = character_card(state, character, ui) {
                    message = Some(card_message);
                }
                ui.add_space(6.0);
            }
            if let Some(card_message) = character_slot_card(state, ui) {
                message = Some(card_message);
            }
        });

    if let Some(character_id) = state.deletion_target() {
        let name = state
            .sign_in
            .characters
            .iter()
            .find(|character| character.id == character_id)
            .map(character_name)
            .unwrap_or_default();
        egui::Modal::new(egui::Id::new("confirm_character_deletion")).show(ui.ctx(), |ui| {
            ui.set_width(320.0);
            ui.label(egui::RichText::new("Delete character?").size(16.0).strong());
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "{name} will be permanently deleted. This cannot be undone."
                ))
                .color(theme::TEXT_WEAK),
            );
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 0.0)))
                    .clicked()
                {
                    message = Some(Message::CancelDeletion);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Delete")
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(theme::ERROR)
                            .min_size(egui::vec2(100.0, 0.0)),
                        )
                        .clicked()
                    {
                        message = Some(Message::ConfirmDeletion);
                    }
                });
            });
        });
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            message = Some(Message::CancelDeletion);
        }
    }

    message
}

fn show_closing(ui: &mut egui::Ui) {
    ui.add_space(ui.available_height() * 0.4);
    ui.vertical_centered(|ui| {
        ui.add(egui::Spinner::new().size(28.0).color(theme::ACCENT));
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("Launching game...")
                .size(15.0)
                .color(theme::TEXT_WEAK),
        );
    });
}

fn character_card(
    state: &Characters,
    character: &SignCharacter,
    ui: &mut egui::Ui,
) -> Option<Message> {
    let selection = CharacterSelection::Existing(character.id);
    let selected = state.selection == selection;
    let fill = if selected {
        theme::ACCENT.gamma_multiply(0.10)
    } else {
        theme::CARD_BG
    };
    let stroke = if selected {
        egui::Stroke::new(1.5, theme::ACCENT)
    } else {
        egui::Stroke::new(1.0, theme::BORDER)
    };

    let (weapon_name, weapon_color) = weapon_style(character.weapon_type);
    let gender = match character.gender {
        Gender::Male => "Male",
        Gender::Female => "Female",
    };
    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;

            // Explicit heights avoid egui's 34 px minimum interaction row height.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        egui::RichText::new(character_name(character))
                            .size(15.5)
                            .strong(),
                    );
                },
            );

            let details = match character.last_sign_in_at {
                Some(last_sign_in_at) => format!(
                    "HR {} · GR {} · {} · Last sign-in {}",
                    character.hr,
                    character.gr,
                    gender,
                    format_local_date(last_sign_in_at)
                ),
                None => format!("HR {} · GR {} · {}", character.hr, character.gr, gender),
            };
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 15.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    ui.label(
                        egui::RichText::new(weapon_name)
                            .size(12.0)
                            .strong()
                            .color(weapon_color),
                    );
                    ui.label(
                        egui::RichText::new(format!("· {details}"))
                            .size(11.5)
                            .color(theme::TEXT_WEAK),
                    );
                },
            );
        });

    let response = inner
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.hovered() && !selected && state.is_idle() {
        ui.painter().rect_stroke(
            response.rect,
            10,
            egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
    }
    let rect = response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(rect.left() + 2.0, rect.top() + 7.0),
            egui::pos2(rect.left() + 5.0, rect.bottom() - 7.0),
        ),
        1.5,
        weapon_color,
    );

    card_message(state, selection, &response, state.is_idle())
}

/// The account's new-character slot, whether or not Sign has already created
/// the pending character behind it.
fn character_slot_card(state: &Characters, ui: &mut egui::Ui) -> Option<Message> {
    let selection = CharacterSelection::New;
    let available = state.has_new_slot();
    let selected = state.selection == selection;
    let clickable = state.is_idle() && available;

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 52.0),
        if clickable {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hovered = clickable && response.hovered();
    let response = if hovered {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };

    if selected {
        ui.painter()
            .rect_filled(rect, 10, theme::ACCENT.gamma_multiply(0.10));
    }
    let border = if selected {
        egui::Stroke::new(1.5, theme::ACCENT)
    } else if hovered {
        egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.55))
    } else {
        egui::Stroke::new(1.0, theme::BORDER)
    };
    ui.painter()
        .rect_stroke(rect, 10, border, egui::StrokeKind::Inside);

    let label = if available {
        "New character"
    } else {
        "All 16 character slots are in use"
    };
    let color = if !available {
        theme::TEXT_WEAK.gamma_multiply(0.7)
    } else if selected || hovered {
        theme::ACCENT
    } else {
        theme::TEXT_WEAK
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );

    card_message(state, selection, &response, clickable)
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
    if response.double_clicked() && state.selection == selection && state.can_launch() {
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

fn weapon_style(weapon_type: WeaponType) -> (&'static str, egui::Color32) {
    match weapon_type {
        WeaponType::SwordAndShield => ("Sword & Shield", egui::Color32::from_rgb(143, 163, 191)),
        WeaponType::HeavyBowgun => ("Heavy Bowgun", egui::Color32::from_rgb(95, 158, 110)),
        WeaponType::Hammer => ("Hammer", egui::Color32::from_rgb(201, 162, 39)),
        WeaponType::GreatSword => ("Great Sword", egui::Color32::from_rgb(224, 108, 91)),
        WeaponType::Lance => ("Lance", egui::Color32::from_rgb(111, 168, 220)),
        WeaponType::LightBowgun => ("Light Bowgun", egui::Color32::from_rgb(155, 194, 91)),
        WeaponType::LongSword => ("Long Sword", egui::Color32::from_rgb(217, 79, 112)),
        WeaponType::DualBlades => ("Dual Blades", egui::Color32::from_rgb(232, 152, 90)),
        WeaponType::HuntingHorn => ("Hunting Horn", egui::Color32::from_rgb(169, 139, 224)),
        WeaponType::Gunlance => ("Gunlance", egui::Color32::from_rgb(91, 168, 160)),
        WeaponType::Bow => ("Bow", egui::Color32::from_rgb(127, 176, 105)),
        WeaponType::Tonfa => ("Tonfa", egui::Color32::from_rgb(124, 140, 228)),
        WeaponType::SwitchAxe => ("Switch Axe", egui::Color32::from_rgb(208, 105, 158)),
        WeaponType::MagnetSpike => ("Magnet Spike", egui::Color32::from_rgb(154, 163, 178)),
    }
}

fn format_local_date(timestamp: Timestamp) -> String {
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d")
        .to_string()
}

fn character_name(character: &SignCharacter) -> String {
    if character.name.is_empty() {
        format!("Character #{}", u32::from(character.id))
    } else {
        character.name.clone()
    }
}
