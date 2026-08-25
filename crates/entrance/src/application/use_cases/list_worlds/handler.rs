use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::outbound::WorldList;
use crate::{EntranceSessionContext, InternalError, MhfBin8};

pub(super) struct ListWorldsHandler;

impl Handler<EntranceSessionContext, BinrwOutboundSender> for ListWorldsHandler {
    type Inbound = ();
    type Error = InternalError;

    async fn handle(
        &self,
        context: EntranceSessionContext,
        _inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let response = execute(context).await?;
        outbound.send(response).await?;
        Ok(())
    }
}

async fn execute(
    _context: EntranceSessionContext,
) -> Result<MhfBin8<WorldList>, InternalError> {
    unimplemented!()
}
