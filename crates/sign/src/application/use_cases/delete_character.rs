mod handler;
mod inbound;

use handler::DeleteCharacterHandler;

use crate::router::{SignPacketRegistration, VersionSelector};

inventory::submit! {
    SignPacketRegistration::new(
        &["DELETE:"],
        VersionSelector::Any,
        &DeleteCharacterHandler,
    )
}
