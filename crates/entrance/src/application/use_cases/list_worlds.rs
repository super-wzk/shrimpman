mod handler;
pub(super) mod outbound;

use handler::ListWorldsHandler;
pub(super) use handler::build_world_list;

use crate::router::EntranceRouteRegistration;

inventory::submit! {
    EntranceRouteRegistration::new(&["ALL"], &ListWorldsHandler)
}
