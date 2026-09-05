mod handler;
mod inbound;

use handler::PingHandler;

use crate::router::LandRouteRegistration;

inventory::submit! {
    LandRouteRegistration::new(&PingHandler)
}
