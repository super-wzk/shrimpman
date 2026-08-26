use binrw::BinRead;
use shrimpman_domain::world::LandKey;
use shrimpman_protocol::{BinrwHandlerDecoder, BinrwOutboundSender, Handler};

use crate::{application::InternalError, envelope::LandPacket};

pub(super) type LandHandlerDecoder = BinrwHandlerDecoder<LandKey, InternalError>;

pub(crate) struct LandPacketRegistration {
    pub(super) opcode: u16,
    pub(super) decoder: LandHandlerDecoder,
}

impl LandPacketRegistration {
    #[allow(dead_code, reason = "used by distributed Land use-case registrations")]
    pub(crate) const fn new<H>(handler: &'static H) -> Self
    where
        H: Handler<LandKey, BinrwOutboundSender, Error = InternalError>,
        H::Inbound: LandPacket + for<'args> BinRead<Args<'args> = ()>,
    {
        Self {
            opcode: H::Inbound::OPCODE,
            decoder: LandHandlerDecoder::big_endian(handler),
        }
    }
}

inventory::collect!(LandPacketRegistration);
