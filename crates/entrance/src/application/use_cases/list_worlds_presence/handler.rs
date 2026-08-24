use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::inbound::ListWorldsPresence;
use crate::{EntranceSessionContext, InternalError};

pub(super) struct ListWorldsPresenceHandler;

impl Handler<EntranceSessionContext, BinrwOutboundSender> for ListWorldsPresenceHandler {
    type Inbound = ListWorldsPresence;
    type Error = InternalError;

    async fn handle(
        &self,
        _context: EntranceSessionContext,
        inbound: Self::Inbound,
        _outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let _ = inbound.character_ids;
        unimplemented!()
    }
}
