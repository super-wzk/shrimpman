mod handler;

use handler::ListWorldsHandler;

use crate::router::EntrancePacketRegistration;

inventory::submit! {
    EntrancePacketRegistration::new(&["ALL"], &ListWorldsHandler)
}
