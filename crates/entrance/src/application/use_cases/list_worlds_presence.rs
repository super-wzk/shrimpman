mod handler;
mod inbound;
mod outbound;

use handler::ListWorldsPresenceHandler;

use crate::router::EntranceRouteRegistration;

inventory::submit! {
    EntranceRouteRegistration::new(&["ALL+"], &ListWorldsPresenceHandler)
}
