mod handler;
mod inbound;
mod outbound;

use handler::PasswordSignInHandler;

use crate::router::{SignPacketRegistration, VersionSelector};

const SESSION_TOKEN_LEN: usize = 16;

inventory::submit! {
    SignPacketRegistration::new(
        &["SIGN:", "DSGN:", "DLTSKEYSIGN:"],
        VersionSelector::Any,
        &PasswordSignInHandler,
    )
}
