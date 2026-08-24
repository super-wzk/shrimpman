mod handler;
mod inbound;

use handler::ListWorldsPresenceHandler;

use crate::router::EntrancePacketRegistration;

inventory::submit! {
    EntrancePacketRegistration::new(&["ALL+"], &ListWorldsPresenceHandler)
}
