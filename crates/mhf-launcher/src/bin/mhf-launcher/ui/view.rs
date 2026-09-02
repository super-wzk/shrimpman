use super::model::{Characters, Message, Model, SignIn};
use super::theme;
use eframe::egui;
use jiff::Timestamp;
use shrimpman_domain::character::{CharacterId, Gender, WeaponType};
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
    let content_height = 320.0;
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
                            .desired_width(f32::INFINITY)
                            .min_size(egui::vec2(0.0, 32.0)),
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
                            .desired_width(f32::INFINITY)
                            .min_size(egui::vec2(0.0, 32.0)),
                    );

                    ui.add_space(16.0);
                    let can_sign_in = state.can_submit();
                    let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let width = ui.available_width();
                    let clicked =
                        theme::primary_button(ui, "Sign in", can_sign_in, width).clicked();
                    if clicked || can_sign_in && enter_pressed {
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
                .add_enabled(!state.creating, egui::Button::new("Sign out"))
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
                if ui
                    .add_enabled(
                        state.can_create(),
                        egui::Button::new("New character").min_size(egui::vec2(0.0, 34.0)),
                    )
                    .clicked()
                {
                    message = Some(Message::CreateCharacter);
                }

                if state.creating {
                    ui.add_space(6.0);
                    ui.add(egui::Spinner::new().size(16.0).color(theme::ACCENT));
                    ui.label(
                        egui::RichText::new("Creating character...")
                            .size(12.5)
                            .color(theme::TEXT_WEAK),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(
                        ui,
                        "Launch game",
                        state.launch_character_id().is_some(),
                        150.0,
                    )
                    .clicked()
                    {
                        message = Some(Message::Launch);
                    }
                });
            });
            ui.add_space(2.0);
        });

    if state.sign_in.characters.is_empty() {
        ui.add_space(ui.available_height() * 0.3);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No characters yet").size(17.0).strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Create a new character to begin your hunt.")
                    .size(13.0)
                    .color(theme::TEXT_WEAK),
            );
        });
    } else {
        let (up, down, enter) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
                input.key_pressed(egui::Key::Enter),
            )
        });
        if (up || down)
            && let Some(next) = adjacent_character(state, down)
        {
            message = Some(Message::SelectCharacter(next));
        }
        if enter && state.launch_character_id().is_some() {
            message = Some(Message::Launch);
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for character in &state.sign_in.characters {
                    if let Some(card_message) = character_card(state, character, ui) {
                        message = Some(card_message);
                    }
                    ui.add_space(6.0);
                }
            });
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
    let selected = state.selected_character_id == Some(character.id);
    let fill = if selected {
        theme::ACCENT.gamma_multiply(0.14)
    } else {
        theme::CARD_BG
    };
    let stroke = if selected {
        egui::Stroke::new(1.5, theme::ACCENT)
    } else {
        egui::Stroke::new(1.0, theme::BORDER)
    };

    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                avatar(character, ui);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(1.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(character_name(character))
                                .size(15.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(format!("· {}", gender_name(character.gender)))
                                .size(12.0)
                                .color(theme::TEXT_WEAK),
                        );
                        if character.is_new {
                            theme::chip(
                                ui,
                                "NEW",
                                theme::ACCENT.gamma_multiply(0.25),
                                theme::ACCENT,
                                true,
                            );
                        }
                    });
                    if let Some(last_sign_in_at) = character.last_sign_in_at {
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Last sign-in {}",
                                format_local(last_sign_in_at)
                            ))
                            .size(11.5)
                            .color(theme::TEXT_WEAK),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::chip(
                        ui,
                        format!("GR {}", character.gr),
                        theme::HOVER_BG,
                        theme::TEXT_WEAK,
                        false,
                    );
                    theme::chip(
                        ui,
                        format!("HR {}", character.hr),
                        theme::HOVER_BG,
                        theme::TEXT_WEAK,
                        false,
                    );
                    theme::chip(
                        ui,
                        weapon_name(character.weapon_type),
                        theme::HOVER_BG,
                        theme::ACCENT,
                        false,
                    );
                });
            });
        });

    let response = inner
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.hovered() && !selected && !state.creating {
        ui.painter().rect_stroke(
            response.rect,
            10,
            egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
    }
    if selected {
        let rect = response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + 2.0, rect.top() + 8.0),
                egui::pos2(rect.left() + 5.0, rect.bottom() - 8.0),
            ),
            1.5,
            theme::ACCENT,
        );
    }

    if state.creating {
        return None;
    }
    if response.double_clicked() && selected && state.launch_character_id().is_some() {
        return Some(Message::Launch);
    }
    if response.clicked() {
        return Some(Message::SelectCharacter(character.id));
    }
    None
}

fn avatar(character: &SignCharacter, ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, theme::ACCENT.gamma_multiply(0.16));
    let label = if character.name.is_empty() {
        u32::from(character.id).to_string()
    } else {
        character
            .name
            .chars()
            .next()
            .unwrap_or('#')
            .to_uppercase()
            .collect()
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &label,
        egui::FontId::proportional(if label.len() > 1 { 13.0 } else { 17.0 }),
        theme::ACCENT,
    );
}

fn adjacent_character(state: &Characters, forward: bool) -> Option<CharacterId> {
    let characters = &state.sign_in.characters;
    if characters.is_empty() {
        return None;
    }
    let current = state
        .selected_character_id
        .and_then(|id| characters.iter().position(|character| character.id == id));
    let index = match (current, forward) {
        (Some(index), true) => (index + 1).min(characters.len() - 1),
        (Some(index), false) => index.saturating_sub(1),
        (None, _) => 0,
    };
    characters.get(index).map(|character| character.id)
}

fn gender_name(gender: Gender) -> &'static str {
    match gender {
        Gender::Male => "Male",
        Gender::Female => "Female",
    }
}

fn format_local(timestamp: Timestamp) -> String {
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

fn weapon_name(weapon_type: WeaponType) -> &'static str {
    match weapon_type {
        WeaponType::SwordAndShield => "Sword & Shield",
        WeaponType::HeavyBowgun => "Heavy Bowgun",
        WeaponType::Hammer => "Hammer",
        WeaponType::GreatSword => "Great Sword",
        WeaponType::Lance => "Lance",
        WeaponType::LightBowgun => "Light Bowgun",
        WeaponType::LongSword => "Long Sword",
        WeaponType::DualBlades => "Dual Blades",
        WeaponType::HuntingHorn => "Hunting Horn",
        WeaponType::Gunlance => "Gunlance",
        WeaponType::Bow => "Bow",
        WeaponType::Tonfa => "Tonfa",
        WeaponType::SwitchAxe => "Switch Axe",
        WeaponType::MagnetSpike => "Magnet Spike",
    }
}

fn character_name(character: &SignCharacter) -> String {
    if character.name.is_empty() {
        format!("Character #{}", u32::from(character.id))
    } else {
        character.name.clone()
    }
}
