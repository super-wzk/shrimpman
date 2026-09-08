use super::LaunchRequest;
use crate::sign;
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use shrimpman_mhf_launcher::{PasswordCredentials, SignInSuccess};

pub(super) enum Model {
    SignIn(SignIn),
    Characters(Characters),
    Closing,
}

impl Default for Model {
    fn default() -> Self {
        Self::sign_in(None)
    }
}

pub(super) struct SignIn {
    pub(super) form: CredentialsForm,
    pub(super) submitting: bool,
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
    pub(super) remember_password: bool,
}

impl CredentialsForm {
    fn remembered(credentials: PasswordCredentials) -> Self {
        Self {
            username: credentials.username,
            password: credentials.password,
            remember_password: true,
        }
    }

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
    pub(super) selection: CharacterSelection,
    pub(super) operation: CharacterOperation,
}

const CHARACTER_LIMIT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CharacterSelection {
    Existing(CharacterId),
    New,
}

enum LaunchAction {
    UseCharacter(CharacterId),
    CreateCharacter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CharacterOperation {
    Idle,
    CreatingCharacter,
    ConfirmingDeletion(CharacterId),
    DeletingCharacter,
}

impl Characters {
    fn has_existing_character(&self, character_id: CharacterId) -> bool {
        self.sign_in
            .characters
            .iter()
            .any(|character| character.id == character_id && !character.is_new)
    }

    fn pending_character_id(&self) -> Option<CharacterId> {
        self.sign_in
            .characters
            .iter()
            .find(|character| character.is_new)
            .map(|character| character.id)
    }

    pub(super) fn has_new_slot(&self) -> bool {
        self.pending_character_id().is_some() || self.sign_in.characters.len() < CHARACTER_LIMIT
    }

    fn can_select(&self, selection: CharacterSelection) -> bool {
        if !self.is_idle() {
            return false;
        }

        match selection {
            CharacterSelection::Existing(character_id) => self.has_existing_character(character_id),
            CharacterSelection::New => self.has_new_slot(),
        }
    }

    fn can_delete(&self, character_id: CharacterId) -> bool {
        self.is_idle() && self.has_existing_character(character_id)
    }

    pub(super) fn selected_deletable_character_id(&self) -> Option<CharacterId> {
        match self.selection {
            CharacterSelection::Existing(character_id) if self.can_delete(character_id) => {
                Some(character_id)
            }
            _ => None,
        }
    }

    fn launch_action(&self) -> Option<LaunchAction> {
        if !self.is_idle() || self.sign_in.entrance_servers.is_empty() {
            return None;
        }

        match self.selection {
            CharacterSelection::Existing(character_id) => self
                .has_existing_character(character_id)
                .then_some(LaunchAction::UseCharacter(character_id)),
            CharacterSelection::New => match self.pending_character_id() {
                Some(character_id) => Some(LaunchAction::UseCharacter(character_id)),
                None if self.sign_in.characters.len() < CHARACTER_LIMIT => {
                    Some(LaunchAction::CreateCharacter)
                }
                None => None,
            },
        }
    }

    pub(super) fn can_launch(&self) -> bool {
        self.launch_action().is_some()
    }

    pub(super) fn is_idle(&self) -> bool {
        self.operation == CharacterOperation::Idle
    }

    pub(super) fn deletion_target(&self) -> Option<CharacterId> {
        match self.operation {
            CharacterOperation::ConfirmingDeletion(character_id) => Some(character_id),
            _ => None,
        }
    }

    fn into_launch_request(self, selected_character_id: CharacterId) -> LaunchRequest {
        LaunchRequest {
            credentials: self.form.into_credentials(),
            sign_in: self.sign_in,
            selected_character_id,
        }
    }
}

pub(super) enum Message {
    SignIn,
    SignedIn {
        result: Result<SignInSuccess, sign::Error>,
        credential_error: Option<String>,
    },
    SignOut,
    Select(CharacterSelection),
    CharacterCreated(Result<sign::CharacterCreated, sign::Error>),
    DeleteCharacter(CharacterId),
    ConfirmDeletion,
    CancelDeletion,
    CharacterDeleted(Result<CharacterId, sign::Error>),
    Launch,
}

pub(super) enum Effect {
    SignIn {
        credentials: PasswordCredentials,
        remember_password: bool,
    },
    CreateCharacter {
        credentials: PasswordCredentials,
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
    },
    DeleteCharacter {
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
        character_id: CharacterId,
    },
    NotifyError(String),
    Launch(LaunchRequest),
}

impl Model {
    pub(super) fn sign_in(credentials: Option<PasswordCredentials>) -> Self {
        Self::SignIn(SignIn {
            form: credentials.map_or_else(CredentialsForm::default, CredentialsForm::remembered),
            submitting: false,
        })
    }

    pub(super) fn update(self, message: Message) -> (Self, Option<Effect>) {
        match (self, message) {
            (Self::SignIn(mut state), Message::SignIn) => {
                if !state.can_submit() {
                    return (Self::SignIn(state), None);
                }

                let credentials = state.form.to_credentials();
                let remember_password = state.form.remember_password;
                state.submitting = true;
                (
                    Self::SignIn(state),
                    Some(Effect::SignIn {
                        credentials,
                        remember_password,
                    }),
                )
            }
            (
                Self::SignIn(state),
                Message::SignedIn {
                    result: Ok(sign_in),
                    credential_error,
                },
            ) => {
                let selection = sign_in
                    .last_character_id
                    .and_then(|character_id| {
                        sign_in
                            .characters
                            .iter()
                            .find(|character| character.id == character_id)
                    })
                    .or_else(|| sign_in.characters.first())
                    .map_or(CharacterSelection::New, |character| {
                        if character.is_new {
                            CharacterSelection::New
                        } else {
                            CharacterSelection::Existing(character.id)
                        }
                    });
                (
                    Self::Characters(Characters {
                        form: state.form,
                        sign_in,
                        selection,
                        operation: CharacterOperation::Idle,
                    }),
                    credential_error.map(|error| {
                        eprintln!("{error}");
                        Effect::NotifyError(
                            "Signed in, but could not update the saved password.".into(),
                        )
                    }),
                )
            }
            (
                Self::SignIn(mut state),
                Message::SignedIn {
                    result: Err(error), ..
                },
            ) => {
                state.submitting = false;
                (
                    Self::SignIn(state),
                    Some(Effect::NotifyError(sign_error_message(&error))),
                )
            }
            (Self::Characters(state), Message::SignOut) if state.is_idle() => (
                Self::SignIn(SignIn {
                    form: state.form,
                    submitting: false,
                }),
                None,
            ),
            (Self::Characters(mut state), Message::Select(selection)) => {
                if state.can_select(selection) {
                    state.selection = selection;
                }
                (Self::Characters(state), None)
            }
            (Self::Characters(mut state), Message::CharacterCreated(Ok(created))) => {
                let selected_character_id = match created {
                    sign::CharacterCreated::Character(character) => {
                        let id = character.id;
                        if let Some(existing) = state
                            .sign_in
                            .characters
                            .iter_mut()
                            .find(|existing| existing.id == id)
                        {
                            *existing = character;
                        } else {
                            state.sign_in.characters.push(character);
                        }
                        id
                    }
                    sign::CharacterCreated::SignedIn(sign_in) => {
                        let Some(character) =
                            sign_in.characters.iter().find(|character| character.is_new)
                        else {
                            return Self::Characters(state).update(Message::CharacterCreated(Err(
                                sign::Error::invalid_response(
                                    "character creation returned no pending character",
                                ),
                            )));
                        };
                        let id = character.id;
                        state.sign_in = sign_in;
                        id
                    }
                };
                let request = state.into_launch_request(selected_character_id);
                (Self::Closing, Some(Effect::Launch(request)))
            }
            (Self::Characters(mut state), Message::DeleteCharacter(character_id)) => {
                if state.can_delete(character_id) {
                    state.operation = CharacterOperation::ConfirmingDeletion(character_id);
                }
                (Self::Characters(state), None)
            }
            (Self::Characters(mut state), Message::CancelDeletion) => {
                state.operation = CharacterOperation::Idle;
                (Self::Characters(state), None)
            }
            (Self::Characters(mut state), Message::ConfirmDeletion) => {
                let Some(character_id) = state.deletion_target() else {
                    return (Self::Characters(state), None);
                };

                let effect = Effect::DeleteCharacter {
                    session_id: state.sign_in.session.session_id,
                    session_token: state.sign_in.session.token,
                    character_id,
                };
                state.operation = CharacterOperation::DeletingCharacter;
                (Self::Characters(state), Some(effect))
            }
            (Self::Characters(mut state), Message::CharacterDeleted(Ok(character_id))) => {
                state.operation = CharacterOperation::Idle;
                state
                    .sign_in
                    .characters
                    .retain(|character| character.id != character_id);
                if state.sign_in.last_character_id == Some(character_id) {
                    state.sign_in.last_character_id = None;
                }
                if state.selection == CharacterSelection::Existing(character_id) {
                    state.selection = state
                        .sign_in
                        .characters
                        .iter()
                        .find(|character| !character.is_new)
                        .map_or(CharacterSelection::New, |character| {
                            CharacterSelection::Existing(character.id)
                        });
                }
                (Self::Characters(state), None)
            }
            (
                Self::Characters(mut state),
                Message::CharacterCreated(Err(error)) | Message::CharacterDeleted(Err(error)),
            ) => {
                let error_message = sign_error_message(&error);
                if error.code() == Some("invalid_session") {
                    return (
                        Self::SignIn(SignIn {
                            form: state.form,
                            submitting: false,
                        }),
                        Some(Effect::NotifyError(error_message)),
                    );
                }

                state.operation = CharacterOperation::Idle;
                (
                    Self::Characters(state),
                    Some(Effect::NotifyError(error_message)),
                )
            }
            (Self::Characters(mut state), Message::Launch) => match state.launch_action() {
                Some(LaunchAction::UseCharacter(selected_character_id)) => {
                    let request = state.into_launch_request(selected_character_id);
                    (Self::Closing, Some(Effect::Launch(request)))
                }
                Some(LaunchAction::CreateCharacter) => {
                    let effect = Effect::CreateCharacter {
                        credentials: state.form.to_credentials(),
                        session_id: state.sign_in.session.session_id,
                        session_token: state.sign_in.session.token,
                    };
                    state.operation = CharacterOperation::CreatingCharacter;
                    (Self::Characters(state), Some(effect))
                }
                None => (Self::Characters(state), None),
            },
            (model, _) => (model, None),
        }
    }
}

fn sign_error_message(error: &sign::Error) -> String {
    eprintln!("{error}");
    match error {
        sign::Error::Timeout | sign::Error::Http(ureq::Error::Timeout(_)) => {
            return format!(
                "The request timed out ({} seconds maximum). Check your connection and try again.",
                sign::REQUEST_TIMEOUT.as_secs(),
            );
        }
        sign::Error::Http(_) | sign::Error::Tcp(_) => {
            return "Cannot reach the sign service. Check your connection and try again."
                .to_owned();
        }
        sign::Error::InvalidResponse(_) => {
            return "The sign service returned an invalid response. Please try again.".to_owned();
        }
        _ => {}
    }
    match error.code() {
        Some("wrong_password") => "The username or password is incorrect.".to_owned(),
        Some("illegal_input") => "The username or password is not valid.".to_owned(),
        Some("invalid_session") => "The Sign session expired. Sign in again.".to_owned(),
        Some("pending_character_exists") => {
            "A new character is already waiting for setup.".to_owned()
        }
        Some("character_not_found") => "The character no longer exists.".to_owned(),
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
    use shrimpman_mhf_launcher::{IssuedSignSession, SignCharacter};
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn timed_out_sign_in_preserves_credentials_and_allows_retry() {
        for error in [
            sign::Error::Timeout,
            sign::Error::Http(ureq::Error::Timeout(ureq::Timeout::Global)),
        ] {
            let (model, _) = sign_in_model().update(Message::SignIn);
            let (model, effect) = model.update(Message::SignedIn {
                result: Err(error),
                credential_error: None,
            });
            let Some(Effect::NotifyError(message)) = effect else {
                panic!("timeout must emit an error notification");
            };
            let Model::SignIn(state) = model else {
                panic!("timeout must keep the sign-in form open");
            };
            assert!(state.can_submit());
            assert_eq!(state.form.username, "  hunter  ");
            assert_eq!(state.form.password, "secret");
            assert!(message.contains("timed out"));
        }
    }

    #[test]
    fn saved_credentials_prefill_the_sign_in_form() {
        let model = Model::sign_in(Some(PasswordCredentials {
            username: "hunter".to_owned(),
            password: "secret".to_owned(),
        }));

        let Model::SignIn(state) = model else {
            panic!("saved credentials did not open the sign-in page");
        };
        assert_eq!(state.form.username, "hunter");
        assert_eq!(state.form.password, "secret");
        assert!(state.form.remember_password);
    }

    #[test]
    fn sign_in_transitions_to_character_selection() {
        let model = sign_in_model();
        let (model, effect) = model.update(Message::SignIn);

        let Some(Effect::SignIn {
            credentials,
            remember_password,
        }) = effect
        else {
            panic!("sign-in did not emit a Sign effect");
        };
        assert_eq!(credentials.username, "hunter");
        assert_eq!(credentials.password, "secret");
        assert!(!remember_password);
        let Model::SignIn(state) = model else {
            panic!("sign-in request changed the page");
        };
        assert!(state.submitting);

        let (model, effect) = Model::SignIn(state).update(Message::SignedIn {
            result: Ok(sign_in_success()),
            credential_error: None,
        });
        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("successful sign-in did not open character selection");
        };
        assert_eq!(
            state.selection,
            CharacterSelection::Existing(CharacterId::from(7))
        );
        assert_eq!(state.form.username, "  hunter  ");
    }

    #[test]
    fn sign_in_effect_preserves_the_remember_password_choice() {
        let mut state = match sign_in_model() {
            Model::SignIn(state) => state,
            _ => unreachable!(),
        };
        state.form.remember_password = true;

        let (_, effect) = Model::SignIn(state).update(Message::SignIn);

        let Some(Effect::SignIn {
            remember_password, ..
        }) = effect
        else {
            panic!("sign-in did not emit a Sign effect");
        };
        assert!(remember_password);
    }

    #[test]
    fn launching_an_empty_slot_creates_the_character_then_launches() {
        let (model, effect) = Model::Characters(empty_characters()).update(Message::Launch);

        let Some(Effect::CreateCharacter {
            session_id,
            session_token,
            ..
        }) = effect
        else {
            panic!("character creation did not emit a Sign effect");
        };
        assert_eq!(session_id, SignSessionId::from(11));
        assert_eq!(session_token, *b"0123456789abcdef");
        let Model::Characters(state) = model else {
            panic!("character creation changed the page before it completed");
        };
        assert_eq!(state.operation, CharacterOperation::CreatingCharacter);

        let (model, effect) = Model::Characters(state).update(Message::CharacterCreated(Ok(
            sign::CharacterCreated::Character(new_character(8)),
        )));
        assert!(matches!(model, Model::Closing));
        let Some(Effect::Launch(request)) = effect else {
            panic!("created character was not launched");
        };
        assert_eq!(request.selected_character_id, CharacterId::from(8));
        assert!(
            request
                .sign_in
                .characters
                .iter()
                .any(|character| character.id == CharacterId::from(8))
        );
    }

    #[test]
    fn tcp_character_creation_launches_with_the_refreshed_session() {
        let mut refreshed = sign_in_success();
        refreshed.session.session_id = SignSessionId::from(99);
        refreshed.session.token = *b"fedcba9876543210";
        refreshed.characters.push(new_character(8));
        let (model, effect) = Model::Characters(characters()).update(Message::CharacterCreated(
            Ok(sign::CharacterCreated::SignedIn(refreshed)),
        ));
        assert!(matches!(model, Model::Closing));
        let Some(Effect::Launch(request)) = effect else {
            panic!("created character was not launched");
        };
        assert_eq!(request.selected_character_id, CharacterId::from(8));
        assert_eq!(request.sign_in.session.session_id, SignSessionId::from(99));
        assert_eq!(request.sign_in.session.token, *b"fedcba9876543210");
    }

    #[test]
    fn launching_a_pending_new_character_does_not_create_another_one() {
        let mut state = characters();
        state.sign_in.characters.push(new_character(8));
        state.selection = CharacterSelection::New;

        let (model, effect) = Model::Characters(state).update(Message::Launch);

        assert!(matches!(model, Model::Closing));
        let Some(Effect::Launch(request)) = effect else {
            panic!("pending character was not launched directly");
        };
        assert_eq!(request.selected_character_id, CharacterId::from(8));
    }

    #[test]
    fn pending_new_character_cannot_be_deleted() {
        let mut state = characters();
        state.sign_in.characters.push(new_character(8));
        state.selection = CharacterSelection::New;
        assert!(!state.can_delete(CharacterId::from(8)));
        assert_eq!(state.selected_deletable_character_id(), None);

        let (model, effect) =
            Model::Characters(state).update(Message::DeleteCharacter(CharacterId::from(8)));

        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("rejected deletion changed the page");
        };
        assert_eq!(state.operation, CharacterOperation::Idle);
    }

    #[test]
    fn character_deletion_removes_the_character_and_selects_the_new_slot() {
        let (model, effect) =
            Model::Characters(characters()).update(Message::DeleteCharacter(CharacterId::from(7)));
        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("deletion request changed the page");
        };
        assert_eq!(state.deletion_target(), Some(CharacterId::from(7)));

        let (model, effect) = Model::Characters(state).update(Message::ConfirmDeletion);
        let Some(Effect::DeleteCharacter {
            session_id,
            session_token,
            character_id,
        }) = effect
        else {
            panic!("character deletion did not emit a Sign effect");
        };
        assert_eq!(session_id, SignSessionId::from(11));
        assert_eq!(session_token, *b"0123456789abcdef");
        assert_eq!(character_id, CharacterId::from(7));
        let Model::Characters(state) = model else {
            panic!("character deletion changed the page");
        };
        assert_eq!(state.operation, CharacterOperation::DeletingCharacter);

        let (model, effect) =
            Model::Characters(state).update(Message::CharacterDeleted(Ok(CharacterId::from(7))));
        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("deleted character changed the page");
        };
        assert_eq!(state.operation, CharacterOperation::Idle);
        assert!(state.sign_in.characters.is_empty());
        assert_eq!(state.selection, CharacterSelection::New);
    }

    #[test]
    fn cancel_deletion_keeps_the_character() {
        let (model, effect) =
            Model::Characters(characters()).update(Message::DeleteCharacter(CharacterId::from(7)));
        assert!(effect.is_none());

        let (model, effect) = model.update(Message::CancelDeletion);
        assert!(effect.is_none());
        let Model::Characters(state) = model else {
            panic!("cancelling deletion changed the page");
        };
        assert_eq!(state.operation, CharacterOperation::Idle);
        assert_eq!(state.sign_in.characters.len(), 1);
    }

    #[test]
    fn invalid_session_returns_to_sign_in() {
        let (model, _) = Model::Characters(empty_characters()).update(Message::Launch);
        let error = sign::Error::HttpResponse {
            status: 401,
            code: Some("invalid_session".to_owned()),
        };
        let (model, effect) = model.update(Message::CharacterCreated(Err(error)));

        let Some(Effect::NotifyError(message)) = effect else {
            panic!("invalid session must emit an error notification");
        };
        let Model::SignIn(state) = model else {
            panic!("invalid session did not return to sign-in");
        };
        assert_eq!(state.form.username, "  hunter  ");
        assert_eq!(message, "The Sign session expired. Sign in again.");
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
        })
    }

    fn characters() -> Characters {
        Characters {
            form: credentials_form(),
            sign_in: sign_in_success(),
            selection: CharacterSelection::Existing(CharacterId::from(7)),
            operation: CharacterOperation::Idle,
        }
    }

    fn empty_characters() -> Characters {
        let mut state = characters();
        state.sign_in.characters.clear();
        state.selection = CharacterSelection::New;
        state
    }

    fn new_character(id: u32) -> SignCharacter {
        SignCharacter {
            id: CharacterId::from(id),
            name: Vec::new(),
            gr: 0,
            hr: 1,
            weapon_type: WeaponType::SwordAndShield,
            gender: Gender::Male,
            last_sign_in_at: None,
            is_new: true,
        }
    }

    fn credentials_form() -> CredentialsForm {
        CredentialsForm {
            username: "  hunter  ".to_owned(),
            password: "secret".to_owned(),
            remember_password: false,
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
                name: b"Hunter".to_vec(),
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
