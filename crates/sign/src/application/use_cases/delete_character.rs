mod handler;
mod inbound;

use handler::DeleteCharacterHandler;

use crate::router::{SignRouteRegistration, VersionSelector};

inventory::submit! {
    SignRouteRegistration::new(
        &["DELETE:"],
        VersionSelector::Any,
        &DeleteCharacterHandler,
    )
}
