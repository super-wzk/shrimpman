mod handler;
mod inbound;
mod outbound;

use handler::ListWorldsPresenceHandler;

use crate::router::EntrancePacketRegistration;

inventory::submit! {
    EntrancePacketRegistration::new(&["ALL+"], &ListWorldsPresenceHandler)
}
