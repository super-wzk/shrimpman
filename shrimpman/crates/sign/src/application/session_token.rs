use rand::{RngExt, distr::Alphanumeric};
use shrimpman_domain::session::SIGN_SESSION_TOKEN_LEN;

pub(crate) fn generate_session_token() -> [u8; SIGN_SESSION_TOKEN_LEN] {
    let mut rng = rand::rng();
    std::array::from_fn(|_| rng.sample(Alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_a_session_token() {
        let token = generate_session_token();

        assert!(token.iter().all(u8::is_ascii_alphanumeric));
    }
}
