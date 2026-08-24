use rand::{RngExt, distr::Alphanumeric};
use sha2::{Digest, Sha256};

pub(crate) const SESSION_TOKEN_LEN: usize = 16;

pub(crate) fn generate_session_token() -> [u8; SESSION_TOKEN_LEN] {
    let mut rng = rand::rng();
    std::array::from_fn(|_| rng.sample(Alphanumeric))
}

pub(crate) fn hash_session_token(token: &[u8]) -> Vec<u8> {
    Sha256::digest(token).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_hashes_a_session_token() {
        let token = generate_session_token();

        assert!(token.iter().all(u8::is_ascii_alphanumeric));
        assert_eq!(hash_session_token(&token).len(), 32);
    }
}
