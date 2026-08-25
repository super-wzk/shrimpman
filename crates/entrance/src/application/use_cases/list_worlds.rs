mod handler;
pub(super) mod outbound;

use handler::ListWorldsHandler;

use crate::router::EntrancePacketRegistration;

inventory::submit! {
    EntrancePacketRegistration::new(&["ALL"], &ListWorldsHandler)
}
