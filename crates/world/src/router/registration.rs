use std::{any::Any, future::Future, io::Cursor, pin::Pin};

use binrw::{BinRead, BinResult};
use bytes::Bytes;
use shrimpman_protocol::{
    BinrwOutbound, BinrwOutboundSender, ErasedHandler, Handler, OutboundSender, PacketDecoder,
};

use crate::{
    application::{InternalError, WorldSessionContext},
    envelope::LandPacket,
    exchange::LandExchange,
};

pub(super) type LandErasedHandler =
    Box<dyn ErasedHandler<WorldSessionContext, OutboundSender<BinrwOutbound>, InternalError>>;

type HandlerRef = &'static (dyn Any + Send + Sync);
type DecodeHandlerFn = fn(HandlerRef, &mut Cursor<Bytes>) -> BinResult<LandErasedHandler>;

#[derive(Clone, Copy)]
pub(crate) struct LandHandlerDecoder {
    handler: HandlerRef,
    decode: DecodeHandlerFn,
}

impl LandHandlerDecoder {
    const fn new<H>(handler: &'static H) -> Self
    where
        H: Handler<WorldSessionContext, LandExchange, Error = InternalError>,
        H::Inbound: LandPacket + for<'args> BinRead<Args<'args> = ()>,
    {
        Self {
            handler,
            decode: decode_handler::<H>,
        }
    }
}

impl PacketDecoder<()> for LandHandlerDecoder {
    type Output = LandErasedHandler;
    type Error = binrw::Error;

    fn decode(
        &self,
        _metadata: &(),
        payload: &mut Cursor<Bytes>,
    ) -> Result<Self::Output, Self::Error> {
        (self.decode)(self.handler, payload)
    }
}

struct LandBoundHandler<H>
where
    H: Handler<WorldSessionContext, LandExchange>,
{
    inbound: H::Inbound,
    handler: &'static H,
}

impl<H> ErasedHandler<WorldSessionContext, OutboundSender<BinrwOutbound>, InternalError>
    for LandBoundHandler<H>
where
    H: Handler<WorldSessionContext, LandExchange, Error = InternalError>,
{
    fn mode(&self) -> shrimpman_protocol::DispatchMode {
        H::MODE
    }

    fn handle(
        self: Box<Self>,
        context: WorldSessionContext,
        outbound: OutboundSender<BinrwOutbound>,
    ) -> Pin<Box<dyn Future<Output = Result<(), InternalError>> + Send + 'static>> {
        let Self { inbound, handler } = *self;
        let exchange = context.exchange(BinrwOutboundSender::big_endian(outbound));

        Box::pin(handler.handle(context, inbound, exchange))
    }
}

fn decode_handler<H>(
    handler: HandlerRef,
    payload: &mut Cursor<Bytes>,
) -> BinResult<LandErasedHandler>
where
    H: Handler<WorldSessionContext, LandExchange, Error = InternalError>,
    H::Inbound: LandPacket + for<'args> BinRead<Args<'args> = ()>,
{
    let inbound = H::Inbound::read_be(payload)?;
    let handler = handler
        .downcast_ref::<H>()
        .expect("a LandHandlerDecoder retains its original handler type");

    Ok(Box::new(LandBoundHandler { inbound, handler }))
}

pub(crate) struct LandRouteRegistration {
    pub(super) opcode: u16,
    pub(super) decoder: LandHandlerDecoder,
}

impl LandRouteRegistration {
    pub(crate) const fn new<H>(handler: &'static H) -> Self
    where
        H: Handler<WorldSessionContext, LandExchange, Error = InternalError>,
        H::Inbound: LandPacket + for<'args> BinRead<Args<'args> = ()>,
    {
        Self {
            opcode: H::Inbound::OPCODE,
            decoder: LandHandlerDecoder::new(handler),
        }
    }
}

inventory::collect!(LandRouteRegistration);
