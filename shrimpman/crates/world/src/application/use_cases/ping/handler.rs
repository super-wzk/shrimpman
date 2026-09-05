use shrimpman_protocol::{DispatchMode, Handler};

use super::inbound::PingRequest;
use crate::{
    application::{InternalError, WorldSessionContext},
    exchange::LandExchange,
    response::{CommandResponse, Value},
};

pub(super) struct PingHandler;

impl Handler<WorldSessionContext, LandExchange> for PingHandler {
    type Inbound = PingRequest;
    type Error = InternalError;

    const MODE: DispatchMode = DispatchMode::Concurrent;

    async fn handle(
        &self,
        _context: WorldSessionContext,
        inbound: Self::Inbound,
        exchange: LandExchange,
    ) -> Result<(), Self::Error> {
        exchange
            .send_packet_and_flush(CommandResponse::success(inbound.request_handle, Value(0)))
            .await?;
        Ok(())
    }
}
