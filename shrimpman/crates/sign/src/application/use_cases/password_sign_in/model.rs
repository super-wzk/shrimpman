use jiff::Timestamp;
use shrimpman_domain::{
    account::CourseRights,
    character::{Character, CharacterId},
    mezeporta::MezeportaFesta,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
    sign_in_notice::SignInNotice,
};

pub(crate) struct Request {
    pub(crate) username: String,
    pub(crate) password: String,
}

pub(crate) enum Outcome {
    Success(Box<Success>),
    IllegalInput,
    WrongPassword,
}

pub(crate) struct Success {
    pub(crate) session: IssuedSession,
    pub(crate) entrance_servers: Vec<String>,
    pub(crate) characters: Vec<SignedInCharacter>,
    pub(crate) notices: Vec<SignInNotice>,
    pub(crate) last_character_id: Option<CharacterId>,
    pub(crate) rights: CourseRights,
    pub(crate) return_expires_at: Timestamp,
    pub(crate) festa: Option<MezeportaFesta>,
}

pub(crate) struct IssuedSession {
    pub(crate) id: SignSessionId,
    pub(crate) token: [u8; SIGN_SESSION_TOKEN_LEN],
    pub(crate) issued_at: Timestamp,
}

pub(crate) struct SignedInCharacter {
    pub(crate) character: Character,
    pub(crate) last_sign_in_at: Timestamp,
}
