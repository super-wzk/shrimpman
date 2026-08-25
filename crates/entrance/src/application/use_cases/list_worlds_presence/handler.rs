use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::{inbound::ListWorldsPresence, outbound::ListWorldsPresenceResponse};
use crate::{EntranceSessionContext, InternalError};

pub(super) struct ListWorldsPresenceHandler;

impl Handler<EntranceSessionContext, BinrwOutboundSender> for ListWorldsPresenceHandler {
    type Inbound = ListWorldsPresence;
    type Error = InternalError;

    async fn handle(
        &self,
        context: EntranceSessionContext,
        inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let response = execute(context, inbound).await?;
        outbound.send(response).await?;
        Ok(())
    }
}

async fn execute(
    _context: EntranceSessionContext,
    inbound: ListWorldsPresence,
) -> Result<ListWorldsPresenceResponse, InternalError> {
    let _ = inbound.character_ids;
    unimplemented!()
}
