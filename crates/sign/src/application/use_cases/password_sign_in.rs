pub(crate) mod handler;
mod inbound;
pub(crate) mod model;
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
