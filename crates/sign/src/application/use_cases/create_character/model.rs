use shrimpman_domain::{
    character::Character,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};

pub(crate) struct Request {
    pub(crate) session_token: [u8; SIGN_SESSION_TOKEN_LEN],
    pub(crate) session_id: SignSessionId,
}

pub(crate) enum Outcome {
    Created(Character),
    InvalidSession,
    PendingCharacterExists(Character),
}
