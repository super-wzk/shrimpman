mod handler;
mod inbound;

use handler::LoginHandler;

use crate::router::LandRouteRegistration;

inventory::submit! {
    LandRouteRegistration::new(&LoginHandler)
}
