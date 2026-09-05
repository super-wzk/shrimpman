use binrw::BinRead;
use shrimpman_protocol::{BinrwHandlerDecoder, BinrwOutboundSender, Handler};

use crate::{EntranceSessionContext, InternalError};

pub(super) type EntranceHandlerDecoder = BinrwHandlerDecoder<EntranceSessionContext, InternalError>;

pub(crate) struct EntranceRouteRegistration {
    pub(super) commands: &'static [&'static str],
    pub(super) decoder: EntranceHandlerDecoder,
}

impl EntranceRouteRegistration {
    pub(crate) const fn new(
        commands: &'static [&'static str],
        handler: &'static impl Handler<
            EntranceSessionContext,
            BinrwOutboundSender,
            Error = InternalError,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
        >,
    ) -> Self {
        Self {
            commands,
            decoder: EntranceHandlerDecoder::big_endian(handler),
        }
    }
}

inventory::collect!(EntranceRouteRegistration);
