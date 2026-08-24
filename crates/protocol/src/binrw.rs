use std::{any::Any, future::Future, io::Cursor, pin::Pin};

use ::binrw::{BinRead, BinResult, BinWrite, Endian};
use bytes::Bytes;

use crate::{
    EncodePayload, ErasedHandler, Handler, OutboundSendError, OutboundSender, PacketDecoder,
};

type HandlerRef = &'static (dyn Any + Send + Sync);

type BinrwErasedHandler<Context, Error> =
    dyn ErasedHandler<Context, OutboundSender<BinrwOutbound>, Error>;

type DecodeHandlerFn<Context, Error> = fn(
    Endian,
    HandlerRef,
    &mut Cursor<Bytes>,
) -> BinResult<Box<BinrwErasedHandler<Context, Error>>>;

type EncodeFn = fn(&(dyn Any + Send), Endian) -> BinResult<Bytes>;

/// A type-erased outbound value that retains its [`BinWrite`] encoder.
pub struct BinrwOutbound {
    value: Box<dyn Any + Send>,
    endian: Endian,
    encode: EncodeFn,
}

impl BinrwOutbound {
    fn new<Outbound>(outbound: Outbound, endian: Endian) -> Self
    where
        Outbound: for<'args> BinWrite<Args<'args> = ()> + Send + 'static,
    {
        Self {
            value: Box::new(outbound),
            endian,
            encode: encode_outbound::<Outbound>,
        }
    }
}

/// Sends strongly typed [`BinWrite`] values through a connection's outbox.
#[derive(Clone)]
pub struct BinrwOutboundSender {
    sender: OutboundSender<BinrwOutbound>,
    endian: Endian,
}

impl BinrwOutboundSender {
    fn new(sender: OutboundSender<BinrwOutbound>, endian: Endian) -> Self {
        Self { sender, endian }
    }

    /// Enqueues an outbound value without waiting for a transport flush.
    pub async fn send<Outbound>(&self, outbound: Outbound) -> Result<(), OutboundSendError>
    where
        Outbound: for<'args> BinWrite<Args<'args> = ()> + Send + 'static,
    {
        self.sender
            .send(BinrwOutbound::new(outbound, self.endian))
            .await
    }

    /// Enqueues an outbound value and waits for the transport to flush it.
    pub async fn send_and_flush<Outbound>(
        &self,
        outbound: Outbound,
    ) -> Result<(), OutboundSendError>
    where
        Outbound: for<'args> BinWrite<Args<'args> = ()> + Send + 'static,
    {
        self.sender
            .send_and_flush(BinrwOutbound::new(outbound, self.endian))
            .await
    }
}

impl EncodePayload for BinrwOutbound {
    type Error = ::binrw::Error;

    fn encode(self) -> Result<Bytes, Self::Error> {
        (self.encode)(self.value.as_ref(), self.endian)
    }
}

fn encode_outbound<Outbound>(value: &(dyn Any + Send), endian: Endian) -> BinResult<Bytes>
where
    Outbound: for<'args> BinWrite<Args<'args> = ()> + Send + 'static,
{
    let outbound = value
        .downcast_ref::<Outbound>()
        .expect("a BinrwOutbound retains its original outbound type");
    let mut output = Cursor::new(Vec::new());
    outbound.write_options(&mut output, endian, ())?;
    Ok(Bytes::from(output.into_inner()))
}

/// Adapts a [`BinRead`] inbound type and a handler instance to a packet decoder.
pub struct BinrwHandlerDecoder<Context, Error>
where
    Context: Send + 'static,
    Error: Send + 'static,
{
    endian: Endian,
    handler: HandlerRef,
    decode: DecodeHandlerFn<Context, Error>,
}

impl<Context, Error> Clone for BinrwHandlerDecoder<Context, Error>
where
    Context: Send + 'static,
    Error: Send + 'static,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<Context, Error> Copy for BinrwHandlerDecoder<Context, Error>
where
    Context: Send + 'static,
    Error: Send + 'static,
{
}

impl<Context, Error> BinrwHandlerDecoder<Context, Error>
where
    Context: Send + 'static,
    Error: Send + 'static,
{
    const fn new<H>(endian: Endian, handler: &'static H) -> Self
    where
        H: Handler<Context, BinrwOutboundSender, Error = Error>,
        H::Inbound: for<'args> BinRead<Args<'args> = ()>,
    {
        Self {
            endian,
            handler,
            decode: decode_handler::<Context, Error, H>,
        }
    }

    pub const fn big_endian(
        handler: &'static impl Handler<
            Context,
            BinrwOutboundSender,
            Error = Error,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
        >,
    ) -> Self {
        Self::new(Endian::Big, handler)
    }

    pub const fn little_endian(
        handler: &'static impl Handler<
            Context,
            BinrwOutboundSender,
            Error = Error,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
        >,
    ) -> Self {
        Self::new(Endian::Little, handler)
    }
}

impl<Metadata, Context, Error> PacketDecoder<Metadata> for BinrwHandlerDecoder<Context, Error>
where
    Context: Send + 'static,
    Error: Send + 'static,
{
    type Output = Box<BinrwErasedHandler<Context, Error>>;
    type Error = ::binrw::Error;

    fn decode(
        &self,
        _metadata: &Metadata,
        payload: &mut Cursor<Bytes>,
    ) -> Result<Self::Output, Self::Error> {
        (self.decode)(self.endian, self.handler, payload)
    }
}

struct BinrwBoundHandler<Context, H>
where
    Context: Send + 'static,
    H: Handler<Context, BinrwOutboundSender>,
{
    inbound: H::Inbound,
    endian: Endian,
    handler: &'static H,
}

impl<Context, Error, H> ErasedHandler<Context, OutboundSender<BinrwOutbound>, Error>
    for BinrwBoundHandler<Context, H>
where
    Context: Send + 'static,
    Error: Send + 'static,
    H: Handler<Context, BinrwOutboundSender, Error = Error>,
{
    fn mode(&self) -> crate::DispatchMode {
        H::MODE
    }

    fn handle(
        self: Box<Self>,
        context: Context,
        outbound: OutboundSender<BinrwOutbound>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'static>> {
        let Self {
            inbound,
            endian,
            handler,
        } = *self;

        Box::pin(handler.handle(context, inbound, BinrwOutboundSender::new(outbound, endian)))
    }
}

fn decode_handler<Context, Error, H>(
    endian: Endian,
    handler: HandlerRef,
    payload: &mut Cursor<Bytes>,
) -> BinResult<Box<BinrwErasedHandler<Context, Error>>>
where
    Context: Send + 'static,
    Error: Send + 'static,
    H: Handler<Context, BinrwOutboundSender, Error = Error>,
    H::Inbound: for<'args> BinRead<Args<'args> = ()>,
{
    let inbound = H::Inbound::read_options(payload, endian, ())?;
    let handler = handler
        .downcast_ref::<H>()
        .expect("a BinrwHandlerDecoder retains its original handler type");
    Ok(Box::new(BinrwBoundHandler::<Context, H> {
        inbound,
        endian,
        handler,
    }))
}
