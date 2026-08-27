mod handler;
mod inbound;
mod outbound;

use handler::PasswordSignInHandler;

use crate::router::{SignRouteRegistration, VersionSelector};

inventory::submit! {
    SignRouteRegistration::new(
        &["SIGN:", "DSGN:", "DLTSKEYSIGN:"],
        VersionSelector::Any,
        &PasswordSignInHandler,
    )
}
