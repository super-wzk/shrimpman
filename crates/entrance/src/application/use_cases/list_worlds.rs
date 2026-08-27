mod handler;
pub(super) mod outbound;

use handler::ListWorldsHandler;

use crate::router::EntranceRouteRegistration;

inventory::submit! {
    EntranceRouteRegistration::new(&["ALL"], &ListWorldsHandler)
}
