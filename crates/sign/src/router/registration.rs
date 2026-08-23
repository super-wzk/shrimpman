use binrw::BinRead;
use shrimpman_protocol::{BinrwHandlerDecoder, BinrwOutboundSender, Handler};

use crate::{InternalError, SignSessionContext};

use super::VersionSelector;

pub(super) type SignHandlerDecoder = BinrwHandlerDecoder<SignSessionContext, InternalError>;

pub(crate) struct SignPacketRegistration {
    pub(super) commands: &'static [&'static str],
    pub(super) versions: VersionSelector,
    pub(super) decoder: SignHandlerDecoder,
}

impl SignPacketRegistration {
    pub(crate) const fn new(
        commands: &'static [&'static str],
        versions: VersionSelector,
        handler: &'static impl Handler<
            SignSessionContext,
            BinrwOutboundSender,
            Error = InternalError,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
        >,
    ) -> Self {
        Self {
            commands,
            versions,
            decoder: SignHandlerDecoder::big_endian(handler),
        }
    }
}

inventory::collect!(SignPacketRegistration);
