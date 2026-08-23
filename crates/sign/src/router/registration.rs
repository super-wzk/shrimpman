use binrw::{BinRead, BinWrite};
use shrimpman_protocol::{BinrwHandlerDecoder, Handler};

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
            Error = InternalError,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
            Outbound: for<'args> BinWrite<Args<'args> = ()>,
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
