use super::model::{CharacterOperation, CharacterSelection, Characters, Message, Model, SignIn};
use crate::config::SignEncoding;
use crate::model::SignCharacter;
use crate::settings::{Settings, SettingsCategory};
use egui::{Align, Id, Layout, RichText};
use egui_hunter::{
    Button, ButtonKind, Checkbox, Dialog, DialogState, Icon, NoticeKind, Notifications, Panel,
    TextField, Tokens, notice,
};
use jiff::Timestamp;
use shrimpman_domain::character::{Gender, WeaponType};
use std::borrow::Cow;

mod characters;
mod settings;
mod sign_in;

use characters::{character_name, show_characters};

pub(super) struct View {
    encoding: SignEncoding,
    deletion_dialog: DialogState,
    notifications: Notifications,
    password_visible: bool,
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
            password_visible: false,
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

    pub(super) fn notify_success(&mut self, context: &egui::Context, message: &str) {
        self.notifications.push_for(
            context,
            NoticeKind::Success,
            message,
            std::time::Duration::from_secs(4),
        );
    }

    pub(super) fn show_settings(
        &mut self,
        settings: &mut Settings,
        ui: &mut egui::Ui,
    ) -> Option<Message> {
        let message = settings::show(settings, ui);
        self.notifications
            .show_at(ui.ctx(), egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0));
        message
    }

    pub(super) fn show(&mut self, model: &mut Model, ui: &mut egui::Ui) -> Option<Message> {
        let mut message = egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(if matches!(model, Model::SignIn(_)) {
                        0
                    } else {
                        16
                    })
                    .fill(ui.visuals().panel_fill),
            )
            .show(ui, |ui| match model {
                Model::SignIn(state) => sign_in::show(state, &mut self.password_visible, ui),
                Model::Characters(state) => {
                    self.password_visible = false;
                    show_characters(state, self.encoding, ui)
                }
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
        Dialog::new(Id::new("confirm_character_deletion"), "删除角色？")
            .width(360.0)
            .initial_focus(cancel_id)
            .dismiss_on_backdrop(false)
            .show(ui.ctx(), &mut self.deletion_dialog, |ui| {
                ui.add(
                    egui::Label::new(format!(
                        "将永久删除角色「{}」，此操作无法撤销。",
                        target.as_deref().unwrap_or_default(),
                    ))
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    if columns[0]
                        .add(Button::new("取消").id(cancel_id).full_width())
                        .clicked()
                    {
                        message = Some(Message::CancelDeletion);
                        columns[0].close();
                    }
                    if columns[1]
                        .add(
                            Button::new("确认删除")
                                .kind(ButtonKind::Danger)
                                .full_width(),
                        )
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
        self.notifications
            .show_at(ui.ctx(), egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0));
        message
    }
}

fn show_closing(ui: &mut egui::Ui) {
    ui.add_space((ui.available_height() * 0.4).max(0.0));
    Panel::new("正在启动").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("正在启动游戏…");
        });
    });
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
            (SignEncoding::Utf8, &b"\0padding"[..], "角色 #7"),
            (SignEncoding::Utf8, &b""[..], "角色 #7"),
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
        for size in [egui::vec2(620.0, 440.0), egui::vec2(440.0, 360.0)] {
            let context = egui::Context::default();
            mhf_font::install(&context);
            egui_hunter::Theme::default().apply(&context);
            let mut view = View::default();
            let mut model = Model::sign_in(Some(crate::model::PasswordCredentials {
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
                    assert_eq!(button.rect.height(), 44.0);
                    assert!(button.rect.bottom() <= size.y - 8.0);
                    let username = context.read_response(Id::new("sign_in_username")).unwrap();
                    let password = context.read_response(Id::new("sign_in_password")).unwrap();
                    let reveal = context
                        .read_response(Id::new("sign_in_password").with("visibility"))
                        .unwrap();
                    assert!((username.rect.height() - 40.0).abs() <= 1.0);
                    assert_eq!(username.rect.left(), password.rect.left());
                    assert_eq!(username.rect.right(), password.rect.right());
                    assert!(password.rect.contains_rect(reveal.rect));
                    let form_center = if size.x >= 580.0 {
                        (size.x + 180.0) * 0.5
                    } else {
                        size.x * 0.5
                    };
                    assert!((username.rect.center().x - form_center).abs() <= 1.0);
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
        mhf_font::install(&context);
        egui_hunter::Theme::default().apply(&context);
        let mut view = View::default();
        let mut model = Model::sign_in(Some(crate::model::PasswordCredentials {
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
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(Id::new("sign_in_password").with("visibility"))
        );
        assert!(frame(&context, &mut view, &mut model, key(egui::Key::Enter)).is_none());
        assert!(view.password_visible);
        frame(&context, &mut view, &mut model, key(egui::Key::Tab));
        let remember_focus = context.memory(|memory| memory.focused());
        view.notify_error(&context, "无法连接服务器，请重试。");
        frame(&context, &mut view, &mut model, vec![]);
        assert_eq!(context.memory(|memory| memory.focused()), remember_focus);
        assert!(frame(&context, &mut view, &mut model, key(egui::Key::Enter)).is_none());
        let Model::SignIn(state) = &model else {
            panic!("expected sign-in page");
        };
        assert!(!state.form.remember_password);
        assert_eq!(state.form.password, "secret");

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
    fn zoomed_sign_in_scrolls_every_keyboard_target_into_view() {
        // Default and minimum native windows at 200% UI zoom.
        for size in [egui::vec2(310.0, 220.0), egui::vec2(220.0, 180.0)] {
            let context = egui::Context::default();
            mhf_font::install(&context);
            egui_hunter::Theme::default().apply(&context);
            let mut view = View::default();
            let mut model = Model::sign_in(Some(crate::model::PasswordCredentials {
                username: "hunter".into(),
                password: "secret".into(),
            }));
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut time = 0.0;
            let mut render = |events| {
                time += 0.1;
                let mut message = None;
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ui| message = view.show(&mut model, ui),
                );
                output.textures_delta.clear();
                message
            };
            for _ in 0..3 {
                render(vec![]);
            }
            for step in 0..5 {
                if step > 0 {
                    render(key(egui::Key::Tab));
                }
                for _ in 0..8 {
                    render(vec![]);
                }
                let id = context.memory(|memory| memory.focused()).unwrap();
                let response = context.read_response(id).unwrap();
                assert!(
                    screen.contains_rect(response.rect),
                    "{size:?}: {response:?}"
                );
                assert!(
                    response
                        .interact_rect
                        .contains_rect(response.rect.shrink(0.5)),
                    "{size:?}: focused control is clipped: {response:?}",
                );
            }
            assert_eq!(
                context.memory(|memory| memory.focused()),
                Some(Id::new("sign_in_submit")),
            );
            assert!(matches!(
                render(key(egui::Key::Enter)),
                Some(Message::SignIn)
            ));
        }
    }

    #[test]
    fn deletion_dialog_defaults_to_cancel_and_restores_focus() {
        use shrimpman_domain::character::CharacterId;
        let context = egui::Context::default();
        mhf_font::install(&context);
        egui_hunter::Theme::default().apply(&context);
        let mut view = View::default();
        let id = CharacterId::from(7);
        let mut model = characters_model();
        for _ in 0..2 {
            frame(&context, &mut view, &mut model, vec![]);
        }
        let row_id = Id::new(("character", 7_u32));
        let row = context.read_response(row_id).unwrap();
        assert_eq!(row.rect.height(), 64.0);
        let launch = context.read_response(Id::new("launch_game")).unwrap();
        assert!(!row.rect.intersects(launch.rect));
        context.memory_mut(|memory| memory.request_focus(row_id));
        assert!(matches!(
            frame(&context, &mut view, &mut model, key(egui::Key::Enter)),
            Some(Message::Launch)
        ));
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

    #[test]
    fn minimum_zoomed_character_page_keeps_actions_identifiable_and_keyboard_accessible() {
        for size in [egui::vec2(440.0, 360.0), egui::vec2(220.0, 180.0)] {
            let context = egui::Context::default();
            mhf_font::install(&context);
            egui_hunter::Theme::default().apply(&context);
            let mut view = View::default();
            let mut model = characters_model();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut time = 0.0;
            let mut render = |events| {
                time += 0.1;
                let mut message = None;
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ui| message = view.show(&mut model, ui),
                );
                output.textures_delta.clear();
                (message, output)
            };
            for _ in 0..4 {
                render(vec![]);
            }
            let (_, output) = render(vec![]);
            let visible_text = |output: &egui::FullOutput, label: &str| {
                output.shapes.iter().any(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        text.galley.job.text == label
                            && shape.clip_rect.contains_rect(egui::Rect::from_min_size(
                                text.pos,
                                text.galley.size(),
                            ))
                    } else {
                        false
                    }
                })
            };
            assert!(
                visible_text(&output, "启动游戏"),
                "{size:?}: launch label is clipped"
            );
            let footer_top = ["launch_game", "delete_character"]
                .map(|id| context.read_response(Id::new(id)).unwrap().rect.top())
                .into_iter()
                .fold(f32::INFINITY, f32::min);
            let character_text = output
                .shapes
                .iter()
                .find(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Hunter")
                })
                .expect("the character row must be painted");
            assert!(
                character_text.clip_rect.bottom() <= footer_top,
                "{size:?}: character list paints to {}, covering footer controls from {footer_top}",
                character_text.clip_rect.bottom(),
            );
            for (id, label) in [
                ("sign_out", "退出登录"),
                ("delete_character", "删除角色"),
                ("launch_game", "启动游戏"),
            ] {
                let response = context.read_response(Id::new(id)).unwrap();
                assert!(
                    screen.contains_rect(response.rect),
                    "{size:?}: {response:?}"
                );
                assert!(response.interact_rect.contains_rect(response.rect));
                if size.x == 220.0 && id != "launch_game" {
                    assert_eq!(response.rect.size(), egui::Vec2::splat(36.0));
                    render(vec![egui::Event::PointerMoved(response.rect.center())]);
                    for _ in 0..8 {
                        render(vec![]);
                    }
                    let (_, tooltip) = render(vec![]);
                    assert!(
                        visible_text(&tooltip, label),
                        "{size:?}: {label} tooltip is missing or clipped"
                    );
                } else {
                    assert!(visible_text(&output, label), "{size:?}: {label} is clipped");
                }
            }
            for id in ["sign_out", "launch_game", "delete_character"] {
                context.memory_mut(|memory| memory.request_focus(Id::new(id)));
                let (message, _) = render(key(egui::Key::Enter));
                assert!(matches!(
                    (id, message),
                    ("sign_out", Some(Message::SignOut))
                        | ("launch_game", Some(Message::Launch))
                        | ("delete_character", Some(Message::DeleteCharacter(_)))
                ));
            }
        }
    }

    fn characters_model() -> Model {
        use crate::model::{IssuedSignSession, SignInSuccess};
        use shrimpman_domain::{account::CourseRights, character::CharacterId};
        let id = CharacterId::from(7);
        let timestamp = "2026-09-07T00:00:00Z".parse().unwrap();
        Model::Characters(Characters {
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
        })
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
