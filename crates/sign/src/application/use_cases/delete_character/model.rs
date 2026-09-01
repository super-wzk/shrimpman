use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};

pub(crate) struct Request {
    pub(crate) session_token: [u8; SIGN_SESSION_TOKEN_LEN],
    pub(crate) character_id: CharacterId,
    pub(crate) session_id: SignSessionId,
}

pub(crate) enum Outcome {
    Deleted,
    InvalidSession,
    NotFound,
}
