pub(crate) mod handler;
mod inbound;
pub(crate) mod model;

use handler::DeleteCharacterHandler;

use crate::router::{SignRouteRegistration, VersionSelector};

inventory::submit! {
    SignRouteRegistration::new(
        &["DELETE:"],
        VersionSelector::Any,
        &DeleteCharacterHandler,
    )
}
