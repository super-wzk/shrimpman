use shrimpman_protocol::{BinrwOutboundSender, Handler};

use crate::{EntranceSessionContext, InternalError};

pub(super) struct ListWorldsHandler;

impl Handler<EntranceSessionContext, BinrwOutboundSender> for ListWorldsHandler {
    type Inbound = ();
    type Error = InternalError;

    async fn handle(
        &self,
        _context: EntranceSessionContext,
        _inbound: Self::Inbound,
        _outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }
}
