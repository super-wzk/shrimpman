use super::LaunchRequest;
use crate::http;
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use shrimpman_mhf_launcher::{PasswordCredentials, SignCharacter, SignInSuccess};

pub(super) enum Model {
    SignIn(SignIn),
    Characters(Characters),
    Closing,
}

impl Default for Model {
    fn default() -> Self {
        Self::SignIn(SignIn::default())
    }
}

#[derive(Default)]
pub(super) struct SignIn {
    pub(super) form: CredentialsForm,
    pub(super) submitting: bool,
    pub(super) error: Option<String>,
}

impl SignIn {
    pub(super) fn can_submit(&self) -> bool {
        !self.submitting && !self.form.username.trim().is_empty() && !self.form.password.is_empty()
    }
}

#[derive(Default)]
pub(super) struct CredentialsForm {
    pub(super) username: String,
    pub(super) password: String,
}

impl CredentialsForm {
    fn to_credentials(&self) -> PasswordCredentials {
        PasswordCredentials {
            username: self.username.trim().to_owned(),
            password: self.password.clone(),
        }
    }

    fn into_credentials(self) -> PasswordCredentials {
        PasswordCredentials {
            username: self.username.trim().to_owned(),
            password: self.password,
        }
    }
}

pub(super) struct Characters {
    pub(super) form: CredentialsForm,
    pub(super) sign_in: SignInSuccess,
    pub(super) selected_character_id: Option<CharacterId>,
    pub(super) creating: bool,
    pub(super) error: Option<String>,
}

impl Characters {
    pub(super) fn can_create(&self) -> bool {
        !self.creating
            && self.sign_in.characters.len() < 16
            && !self
                .sign_in
                .characters
                .iter()
                .any(|character| character.is_new)
    }

    pub(super) fn launch_character_id(&self) -> Option<CharacterId> {
        if self.creating || self.sign_in.entrance_servers.is_empty() {
            return None;
        }
        self.selected_character_id
    }
}

pub(super) enum Message {
    SignIn,
    SignedIn(Result<SignInSuccess, http::Error>),
    SignOut,
    SelectCharacter(CharacterId),
    CreateCharacter,
    CharacterCreated(Result<SignCharacter, http::Error>),
    Launch,
}

pub(super) enum Effect {
    SignIn(PasswordCredentials),
    CreateCharacter {
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
    },
    Launch(LaunchRequest),
}

impl Model {
    pub(super) fn update(self, message: Message) -> (Self, Option<Effect>) {
        match (self, message) {
            (Self::SignIn(mut state), Message::SignIn) => {
                if !state.can_submit() {
                    return (Self::SignIn(state), None);
                }

                let credentials = state.form.to_credentials();
                state.submitting = true;
                state.error = None;
                (Self::SignIn(state), Some(Effect::SignIn(credentials)))
            }
            (Self::SignIn(state), Message::SignedIn(Ok(sign_in))) => {
                let selected_character_id = sign_in
                    .last_character_id
                    .or_else(|| sign_in.characters.first().map(|character| character.id));
                (
                    Self::Characters(Characters {
                        form: state.form,
                        sign_in,
                        selected_character_id,
                        creating: false,
                        error: None,
                    }),
                    None,
                )
            }
            (Self::SignIn(mut state), Message::SignedIn(Err(error))) => {
                state.submitting = false;
                state.error = Some(http_error_message(&error));
                (Self::SignIn(state), None)
            }
            (Self::Characters(state), Message::SignOut) if !state.creating => (
                Self::SignIn(SignIn {
                    form: state.form,
                    submitting: false,
                    error: None,
                }),
                None,
            ),
            (Self::Characters(mut state), Message::SelectCharacter(character_id)) => {
                if !state.creating
                    && state
                        .sign_in
                        .characters
                        .iter()
                        .any(|character| character.id == character_id)
                {
                    state.selected_character_id = Some(character_id);
                }
                (Self::Characters(state), None)
            }
            (Self::Characters(mut state), Message::CreateCharacter) => {
                if !state.can_create() {
                    return (Self::Characters(state), None);
                }

                let effect = Effect::CreateCharacter {
                    session_id: state.sign_in.session.session_id,
                    session_token: state.sign_in.session.token,
                };
                state.creating = true;
                state.error = None;
                (Self::Characters(state), Some(effect))
            }
            (Self::Characters(mut state), Message::CharacterCreated(Ok(character))) => {
                state.creating = false;
                state.selected_character_id = Some(character.id);
                if let Some(existing) = state
                    .sign_in
                    .characters
                    .iter_mut()
                    .find(|existing| existing.id == character.id)
                {
                    *existing = character;
                } else {
                    state.sign_in.characters.push(character);
                }
                state.error = None;
                (Self::Characters(state), None)
            }
            (Self::Characters(mut state), Message::CharacterCreated(Err(error))) => {
                let error_message = http_error_message(&error);
                if error.code() == Some("invalid_session") {
                    return (
                        Self::SignIn(SignIn {
                            form: state.form,
                            submitting: false,
                            error: Some(error_message),
                        }),
                        None,
                    );
                }

                state.creating = false;
                state.error = Some(error_message);
                (Self::Characters(state), None)
            }
            (Self::Characters(state), Message::Launch) => {
                let Some(selected_character_id) = state.launch_character_id() else {
                    return (Self::Characters(state), None);
                };

                let request = LaunchRequest {
                    credentials: state.form.into_credentials(),
                    sign_in: state.sign_in,
                    selected_character_id,
                };
                (Self::Closing, Some(Effect::Launch(request)))
            }
            (model, _) => (model, None),
        }
    }
}

fn http_error_message(error: &http::Error) -> String {
    match error.code() {
        Some("wrong_password") => "The username or password is incorrect.".to_owned(),
        Some("illegal_input") => "The username or password is not valid.".to_owned(),
        Some("invalid_session") => "The Sign session expired. Sign in again.".to_owned(),
        Some("pending_character_exists") => {
            "A new character is already waiting for setup.".to_owned()
        }
        Some("internal_error") => "The Sign service failed to process the request.".to_owned(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use shrimpman_domain::{
        account::CourseRights,
        character::{Gender, WeaponType},
    };
    use shrimpman_mhf_launcher::IssuedSignSession;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn sign_in_transitions_to_character_selection() {
        let model = sign_in_model();
        let (model, effect) = model.update(Message::SignIn);

        let Some(Effect::SignIn(credentials)) = effect else {
            panic!("sign-in did not emit an HTTP effect");
        };
        assert_eq!(credentials.username, "hunter");
        assert_eq!(credentials.password, "secret");
        let Model::SignIn(state) = model else {
            panic!("sign-in request changed the page");
        };
        assert!(state.submitting);

        let (model, effect) = Model::SignIn(state).update(Message::SignedIn(Ok(sign_in_success())));
        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("successful sign-in did not open character selection");
        };
        assert_eq!(state.selected_character_id, Some(CharacterId::from(7)));
        assert_eq!(state.form.username, "  hunter  ");
    }

    #[test]
    fn character_creation_updates_and_selects_the_character() {
        let state = characters();
        let (model, effect) = Model::Characters(state).update(Message::CreateCharacter);

        let Some(Effect::CreateCharacter {
            session_id,
            session_token,
        }) = effect
        else {
            panic!("character creation did not emit an HTTP effect");
        };
        assert_eq!(session_id, SignSessionId::from(11));
        assert_eq!(session_token, *b"0123456789abcdef");

        let character = SignCharacter {
            id: CharacterId::from(8),
            name: String::new(),
            gr: 0,
            hr: 1,
            weapon_type: WeaponType::SwordAndShield,
            gender: Gender::Male,
            last_sign_in_at: None,
            is_new: true,
        };
        let (model, effect) = model.update(Message::CharacterCreated(Ok(character)));
        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("created character changed the page");
        };
        assert!(!state.creating);
        assert_eq!(state.selected_character_id, Some(CharacterId::from(8)));
        assert_eq!(state.sign_in.characters.len(), 2);
    }

    #[test]
    fn invalid_session_returns_to_sign_in() {
        let (model, _) = Model::Characters(characters()).update(Message::CreateCharacter);
        let error = http::Error::Response {
            status: 401,
            code: Some("invalid_session".to_owned()),
        };
        let (model, effect) = model.update(Message::CharacterCreated(Err(error)));

        assert!(effect.is_none());
        let Model::SignIn(state) = model else {
            panic!("invalid session did not return to sign-in");
        };
        assert_eq!(state.form.username, "  hunter  ");
        assert_eq!(
            state.error.as_deref(),
            Some("The Sign session expired. Sign in again.")
        );
    }

    #[test]
    fn launch_emits_the_final_request() {
        let (model, effect) = Model::Characters(characters()).update(Message::Launch);

        assert!(matches!(model, Model::Closing));
        let Some(Effect::Launch(request)) = effect else {
            panic!("launch did not emit the final request");
        };
        assert_eq!(request.credentials.username, "hunter");
        assert_eq!(request.credentials.password, "secret");
        assert_eq!(request.selected_character_id, CharacterId::from(7));
    }

    fn sign_in_model() -> Model {
        Model::SignIn(SignIn {
            form: credentials_form(),
            submitting: false,
            error: None,
        })
    }

    fn characters() -> Characters {
        Characters {
            form: credentials_form(),
            sign_in: sign_in_success(),
            selected_character_id: Some(CharacterId::from(7)),
            creating: false,
            error: None,
        }
    }

    fn credentials_form() -> CredentialsForm {
        CredentialsForm {
            username: "  hunter  ".to_owned(),
            password: "secret".to_owned(),
        }
    }

    fn sign_in_success() -> SignInSuccess {
        SignInSuccess {
            session: IssuedSignSession {
                session_id: SignSessionId::from(11),
                token: *b"0123456789abcdef",
                issued_at: timestamp("2026-09-02T00:00:00Z"),
            },
            entrance_servers: vec![SocketAddrV4::new(Ipv4Addr::LOCALHOST, 53310)],
            characters: vec![SignCharacter {
                id: CharacterId::from(7),
                name: "Hunter".to_owned(),
                gr: 2,
                hr: 3,
                weapon_type: WeaponType::GreatSword,
                gender: Gender::Female,
                last_sign_in_at: Some(timestamp("2026-09-01T12:00:00Z")),
                is_new: false,
            }],
            notices: Vec::new(),
            last_character_id: None,
            rights: CourseRights::empty(),
            return_expires_at: timestamp("2026-10-02T00:00:00Z"),
            festa: None,
        }
    }

    fn timestamp(value: &str) -> Timestamp {
        value.parse().expect("test timestamp must be valid")
    }
}
