use super::model::{CharacterOperation, CharacterSelection, Characters, Message, Model, SignIn};
use egui::{Align, Id, Layout, RichText};
use egui_hunter::{
    Button, ButtonKind, Checkbox, Dialog, DialogState, Icon, NoticeKind, Notifications, Panel,
    Surface, TextField, notice,
};
use jiff::Timestamp;
use shrimpman_domain::character::{Gender, WeaponType};
use shrimpman_mhf_launcher::{SignCharacter, runtime::SignEncoding};
use std::borrow::Cow;

pub(super) struct View {
    encoding: SignEncoding,
    deletion_dialog: DialogState,
    notifications: Notifications,
}

impl Default for View {
    fn default() -> Self {
        Self::new(SignEncoding::Utf8)
    }
}

impl View {
    pub(super) fn new(encoding: SignEncoding) -> Self {
        Self {
            encoding,
            deletion_dialog: DialogState::default(),
            notifications: Notifications::with_capacity(Id::new("sign-notifications"), 1),
        }
    }

    pub(super) fn notify_error(&mut self, context: &egui::Context, message: &str) {
        let mut chars = message.chars();
        let mut text = chars.by_ref().take(240).collect::<String>();
        if chars.next().is_some() {
            text.push('…');
        }
        self.notifications.push_for(
            context,
            NoticeKind::Danger,
            text,
            std::time::Duration::from_secs(6),
        );
    }

    pub(super) fn show(&mut self, model: &mut Model, ui: &mut egui::Ui) -> Option<Message> {
        let mut message = egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(20)
                    .fill(ui.visuals().panel_fill),
            )
            .show(ui, |ui| match model {
                Model::SignIn(state) => show_sign_in(state, ui),
                Model::Characters(state) => show_characters(state, self.encoding, ui),
                Model::Closing => {
                    show_closing(ui);
                    None
                }
            })
            .inner;

        let target = match model {
            Model::Characters(state) => state.deletion_target().map(|id| {
                state
                    .sign_in
                    .characters
                    .iter()
                    .find(|character| character.id == id)
                    .map(|character| character_name(character, self.encoding))
                    .unwrap_or_default()
            }),
            _ => None,
        };
        if target.is_some() {
            self.deletion_dialog.open(ui.ctx());
        } else {
            self.deletion_dialog.close(ui.ctx());
        }
        let cancel_id = Id::new("cancel_character_deletion");
        Dialog::new(Id::new("confirm_character_deletion"), "Delete character?")
            .width(360.0)
            .initial_focus(cancel_id)
            .dismiss_on_backdrop(false)
            .show(ui.ctx(), &mut self.deletion_dialog, |ui| {
                ui.add(
                    egui::Label::new(format!(
                        "{} will be permanently deleted. This cannot be undone.",
                        target.as_deref().unwrap_or_default(),
                    ))
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    if columns[0]
                        .add(Button::new("Cancel").id(cancel_id).full_width())
                        .clicked()
                    {
                        message = Some(Message::CancelDeletion);
                        columns[0].close();
                    }
                    if columns[1]
                        .add(Button::new("Delete").kind(ButtonKind::Danger).full_width())
                        .clicked()
                    {
                        message = Some(Message::ConfirmDeletion);
                        columns[1].close();
                    }
                });
            });
        if target.is_some() && !self.deletion_dialog.is_open() && message.is_none() {
            message = Some(Message::CancelDeletion);
        }
        // Capture the opener before the model disables it during confirmation.
        if matches!(message, Some(Message::DeleteCharacter(_))) {
            self.deletion_dialog.open(ui.ctx());
        }
        self.notifications.show(ui.ctx());
        message
    }
}

fn show_sign_in(state: &mut SignIn, ui: &mut egui::Ui) -> Option<Message> {
    let mut message = None;
    let height_id = ui.id().with("sign_in_content_height");
    let content_height = ui.ctx().data(|data| data.get_temp::<f32>(height_id));
    let top_space =
        ((ui.available_height() - content_height.unwrap_or(ui.available_height())) * 0.5).max(0.0);
    egui::ScrollArea::vertical()
        .id_salt("sign_in")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(top_space);
            let content = ui.vertical_centered(|ui| {
                ui.label(RichText::new("MONSTER HUNTER FRONTIER").heading().strong());
                ui.add_space(12.0);

                let width = ui.available_width().min(400.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        Panel::new("Sign in")
                            .surface(Surface::Parchment)
                            .show(ui, |ui| {
                                ui.add_space(4.0);
                                let username_id = Id::new("sign_in_username");
                                let password_id = Id::new("sign_in_password");
                                let username = ui.add_enabled(
                                    !state.submitting,
                                    TextField::new(username_id, &mut state.form.username)
                                        .label("Username")
                                        .hint("Enter your username"),
                                );
                                let autofocus = ui.id().with("username_autofocus");
                                if !state.submitting
                                    && !ui.ctx().data(|data| {
                                        data.get_temp::<bool>(autofocus).unwrap_or(false)
                                    })
                                {
                                    username.request_focus();
                                    ui.ctx().data_mut(|data| data.insert_temp(autofocus, true));
                                }
                                ui.add_enabled(
                                    !state.submitting,
                                    TextField::new(password_id, &mut state.form.password)
                                        .label("Password")
                                        .hint("Enter your password")
                                        .password(true),
                                );
                                ui.add_enabled(
                                    !state.submitting,
                                    Checkbox::new(
                                        &mut state.form.remember_password,
                                        "Remember password",
                                    ),
                                );
                                ui.add_space(4.0);
                                let can_sign_in = state.can_submit();
                                let submit_from_field = ui.memory(|memory| {
                                    memory.has_focus(username_id) || memory.has_focus(password_id)
                                }) && ui
                                    .input(|input| input.key_pressed(egui::Key::Enter));
                                let label = if state.submitting {
                                    "Signing in..."
                                } else {
                                    "Sign in"
                                };
                                let clicked = ui
                                    .add_enabled(
                                        can_sign_in,
                                        Button::new(label)
                                            .id(Id::new("sign_in_submit"))
                                            .kind(ButtonKind::Primary)
                                            .icon(Icon::Quest)
                                            .min_size(egui::vec2(0.0, 42.0))
                                            .full_width(),
                                    )
                                    .clicked();
                                if clicked || (can_sign_in && submit_from_field) {
                                    message = Some(Message::SignIn);
                                }
                            });
                    },
                );
            });
            let height = content.response.rect.height();
            if content_height.is_none_or(|previous| (previous - height).abs() > 0.5) {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(height_id, height));
                ui.ctx().request_discard("sign-in content resized");
            }
        });
    message
}

fn show_characters(
    state: &Characters,
    encoding: SignEncoding,
    ui: &mut egui::Ui,
) -> Option<Message> {
    let mut message = None;
    ui.heading("Character selection");
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::Label::new(
                RichText::new(format!("Signed in as {}", state.form.username.trim())).weak(),
            )
            .wrap(),
        );
        if ui
            .add_enabled(state.is_idle(), Button::new("Sign out"))
            .clicked()
        {
            message = Some(Message::SignOut);
        }
    });
    ui.add_space(8.0);

    egui::Panel::bottom("character_actions")
        .show_separator_line(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            ui.add_space(8.0);
            let status = match state.operation {
                CharacterOperation::CreatingCharacter => Some("Creating character..."),
                CharacterOperation::DeletingCharacter => Some("Deleting character..."),
                _ => None,
            };
            if let Some(status) = status {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(status);
                });
            }
            ui.columns(2, |columns| {
                let deletion_target = state.selected_deletable_character_id();
                if columns[0]
                    .add_enabled(
                        deletion_target.is_some(),
                        Button::new("Delete")
                            .id(Id::new("delete_character"))
                            .kind(ButtonKind::Danger)
                            .full_width(),
                    )
                    .clicked()
                    && let Some(character_id) = deletion_target
                {
                    message = Some(Message::DeleteCharacter(character_id));
                }
                if columns[1]
                    .add_enabled(
                        state.can_launch(),
                        Button::new("Launch game")
                            .id(Id::new("launch_game"))
                            .kind(ButtonKind::Primary)
                            .icon(Icon::Sword)
                            .full_width(),
                    )
                    .clicked()
                {
                    message = Some(Message::Launch);
                }
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
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if state.sign_in.entrance_servers.is_empty() {
                show_notice(
                    ui,
                    NoticeKind::Warning,
                    "No Entrance service is currently available.",
                );
            }
            for character in state
                .sign_in
                .characters
                .iter()
                .filter(|character| !character.is_new)
            {
                let selection = CharacterSelection::Existing(character.id);
                let response = character_card(state, character, encoding, ui);
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
                    "New character"
                } else {
                    "All 16 character slots are in use"
                })
                .id(Id::new("new_character"))
                .icon(Icon::Quest)
                .selected(state.selection == CharacterSelection::New)
                .min_size(egui::vec2(0.0, 52.0))
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

fn character_card(
    state: &Characters,
    character: &SignCharacter,
    encoding: SignEncoding,
    ui: &mut egui::Ui,
) -> egui::Response {
    let selected = state.selection == CharacterSelection::Existing(character.id);
    ui.push_id(character.id, |ui| {
        ui.add_enabled_ui(state.is_idle(), |ui| {
            let card = Panel::new("")
                .surface(if selected {
                    Surface::Parchment
                } else {
                    Surface::Leather
                })
                .show(ui, |ui| {
                    let button = ui.add(
                        Button::new(&character_name(character, encoding))
                            .icon(Icon::Sword)
                            .selected(selected)
                            .full_width(),
                    );
                    let gender = match character.gender {
                        Gender::Male => "Male",
                        Gender::Female => "Female",
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "{} · HR {} · GR {} · {gender}",
                                weapon_name(character.weapon_type),
                                character.hr,
                                character.gr,
                            ))
                            .small()
                            .weak(),
                        )
                        .wrap(),
                    );
                    if let Some(timestamp) = character.last_sign_in_at {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "Last sign-in {}",
                                    format_local_date(timestamp)
                                ))
                                .small()
                                .weak(),
                            )
                            .wrap(),
                        );
                    }
                    button
                });
            card.inner
                .union(card.response.interact(egui::Sense::CLICK))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
        })
        .inner
    })
    .inner
}

fn show_notice(ui: &mut egui::Ui, kind: NoticeKind, text: &str) {
    egui::ScrollArea::vertical()
        .id_salt(("notice", kind as u8))
        .max_height(104.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            notice(ui, kind, text);
        });
}

fn show_closing(ui: &mut egui::Ui) {
    ui.add_space((ui.available_height() * 0.4).max(0.0));
    Panel::new("Departure").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Launching game...");
        });
    });
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

fn format_local_date(timestamp: Timestamp) -> String {
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d")
        .to_string()
}

fn character_name(character: &SignCharacter, encoding: SignEncoding) -> Cow<'_, str> {
    let end = character
        .name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(character.name.len());
    let bytes = &character.name[..end];
    if bytes.is_empty() {
        format!("Character #{}", u32::from(character.id)).into()
    } else {
        encoding.decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_names_decode_for_display_without_changing_raw_bytes() {
        for (encoding, bytes, expected) in [
            (
                SignEncoding::Utf8,
                "日本日本日本".as_bytes(),
                "日本日本日本",
            ),
            (
                SignEncoding::ShiftJis,
                &b"\x93\xfa\x96\x7b\0\xff"[..],
                "日本",
            ),
            (SignEncoding::Utf8, &b"Hunter\xff"[..], "Hunter\u{fffd}"),
            (SignEncoding::Utf8, &b"\0padding"[..], "Character #7"),
            (SignEncoding::Utf8, &b""[..], "Character #7"),
        ] {
            let character = SignCharacter {
                id: 7.into(),
                name: bytes.to_vec(),
                gr: 0,
                hr: 0,
                weapon_type: WeaponType::GreatSword,
                gender: Gender::Female,
                last_sign_in_at: None,
                is_new: false,
            };

            assert_eq!(character_name(&character, encoding), expected);
            assert_eq!(character.name, bytes);
        }
    }

    #[test]
    fn error_notifications_do_not_move_the_form_or_take_focus() {
        for size in [egui::vec2(680.0, 520.0), egui::vec2(440.0, 430.0)] {
            let context = egui::Context::default();
            shrimpman_mhf_launcher::font::install(&context);
            egui_hunter::Theme::default().apply(&context);
            let mut view = View::default();
            let mut model = Model::sign_in(Some(shrimpman_mhf_launcher::PasswordCredentials {
                username: "hunter".into(),
                password: "secret".into(),
            }));
            let mut before = None;
            for step in 0..6 {
                if step == 3 {
                    view.notify_error(&context, &"网络错误 network-error/".repeat(100));
                }
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        time: Some(f64::from(step) * 0.1),
                        ..Default::default()
                    },
                    |ui| {
                        let _ = view.show(&mut model, ui);
                    },
                );
                output.textures_delta.clear();
                let button = context.read_response(Id::new("sign_in_submit")).unwrap();
                if step == 2 {
                    before = Some(button.rect);
                }
                if step > 2 {
                    assert_eq!(Some(button.rect), before);
                    assert_eq!(
                        context.memory(|m| m.focused()),
                        Some(Id::new("sign_in_username"))
                    );
                    assert!(button.interact_rect.contains_rect(button.rect));
                    assert!(!view.notifications.is_empty());
                }
            }
        }
    }

    #[test]
    fn keyboard_sign_in_respects_focus_and_submission_state() {
        let context = egui::Context::default();
        shrimpman_mhf_launcher::font::install(&context);
        egui_hunter::Theme::default().apply(&context);
        let mut view = View::default();
        let mut model = Model::sign_in(Some(shrimpman_mhf_launcher::PasswordCredentials {
            username: "hunter".to_owned(),
            password: "secret".to_owned(),
        }));
        for _ in 0..2 {
            frame(&context, &mut view, &mut model, vec![]);
        }
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(Id::new("sign_in_username"))
        );

        frame(&context, &mut view, &mut model, key(egui::Key::Tab));
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(Id::new("sign_in_password"))
        );
        assert!(matches!(
            frame(&context, &mut view, &mut model, key(egui::Key::Enter)),
            Some(Message::SignIn)
        ));

        frame(&context, &mut view, &mut model, key(egui::Key::Tab));
        assert!(frame(&context, &mut view, &mut model, key(egui::Key::Enter)).is_none());
        let Model::SignIn(state) = &model else {
            panic!("expected sign-in page");
        };
        assert!(!state.form.remember_password);

        frame(&context, &mut view, &mut model, key(egui::Key::Tab));
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(Id::new("sign_in_submit"))
        );
        assert!(matches!(
            frame(&context, &mut view, &mut model, key(egui::Key::Enter)),
            Some(Message::SignIn)
        ));
        model = model.update(Message::SignIn).0;
        assert!(frame(&context, &mut view, &mut model, key(egui::Key::Enter)).is_none());
    }

    #[test]
    fn deletion_dialog_defaults_to_cancel_and_restores_focus() {
        use shrimpman_domain::{account::CourseRights, character::CharacterId};
        use shrimpman_mhf_launcher::{IssuedSignSession, SignInSuccess};
        let context = egui::Context::default();
        shrimpman_mhf_launcher::font::install(&context);
        egui_hunter::Theme::default().apply(&context);
        let mut view = View::default();
        let id = CharacterId::from(7);
        let timestamp = "2026-09-07T00:00:00Z".parse().unwrap();
        let mut model = Model::Characters(Characters {
            form: Default::default(),
            sign_in: SignInSuccess {
                session: IssuedSignSession {
                    session_id: 11.into(),
                    token: *b"0123456789abcdef",
                    issued_at: timestamp,
                },
                entrance_servers: vec!["127.0.0.1:53310".parse().unwrap()],
                characters: vec![SignCharacter {
                    id,
                    name: b"Hunter".to_vec(),
                    gr: 2,
                    hr: 3,
                    weapon_type: WeaponType::GreatSword,
                    gender: Gender::Female,
                    last_sign_in_at: None,
                    is_new: false,
                }],
                notices: vec![],
                last_character_id: None,
                rights: CourseRights::empty(),
                return_expires_at: timestamp,
                festa: None,
            },
            selection: CharacterSelection::Existing(id),
            operation: CharacterOperation::Idle,
        });
        for _ in 0..2 {
            frame(&context, &mut view, &mut model, vec![]);
        }
        context.memory_mut(|memory| memory.request_focus(Id::new("delete_character")));
        let message = frame(&context, &mut view, &mut model, key(egui::Key::Enter));
        assert!(matches!(message, Some(Message::DeleteCharacter(target)) if target == id));
        model = model.update(message.unwrap()).0;
        for _ in 0..2 {
            frame(&context, &mut view, &mut model, vec![]);
        }
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(Id::new("cancel_character_deletion"))
        );
        let message = frame(&context, &mut view, &mut model, key(egui::Key::Enter));
        assert!(matches!(message, Some(Message::CancelDeletion)));
        model = model.update(message.unwrap()).0;
        for _ in 0..2 {
            frame(&context, &mut view, &mut model, vec![]);
        }
        assert!(!view.deletion_dialog.is_open());
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(Id::new("delete_character"))
        );

        model = model.update(Message::DeleteCharacter(id)).0;
        for _ in 0..2 {
            frame(&context, &mut view, &mut model, vec![]);
        }
        assert!(matches!(
            frame(&context, &mut view, &mut model, key(egui::Key::Escape)),
            Some(Message::CancelDeletion)
        ));
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .into()
    }

    fn frame(
        context: &egui::Context,
        view: &mut View,
        model: &mut Model,
        events: Vec<egui::Event>,
    ) -> Option<Message> {
        let mut message = None;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(680.0, 520.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                message = view.show(model, ui);
            },
        );
        output.textures_delta.clear();
        message
    }
}
