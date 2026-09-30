mod id;

pub use id::SignSessionId;

/// Number of bytes in an issued Sign session token.
pub const SIGN_SESSION_TOKEN_LEN: usize = 16;
