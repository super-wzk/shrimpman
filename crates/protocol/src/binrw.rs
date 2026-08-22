use std::{any::Any, io::Cursor};

use ::binrw::{BinRead, BinResult, BinWrite, Endian};
use bytes::Bytes;

use crate::{EncodePayload, ErasedHandler, ErasedPacket, Handler, PacketDecoder};

type DecodeFn = fn(Endian, &mut Cursor<Bytes>) -> BinResult<ErasedPacket>;

type HandlerRef = &'static (dyn Any + Send + Sync);

type DecodeHandlerFn<Context, Error> =
    fn(
        Endian,
        HandlerRef,
        &mut Cursor<Bytes>,
    ) -> BinResult<Box<dyn ErasedHandler<Context, BinrwOutbound, Error>>>;

type EncodeFn = fn(&(dyn Any + Send), Endian) -> BinResult<Bytes>;

/// Adapts a [`BinRead`] packet type to the service-independent packet decoder.
#[derive(Clone, Copy)]
pub struct BinrwPacketDecoder {
    endian: Endian,
    decode: DecodeFn,
}

impl BinrwPacketDecoder {
    const fn new<Packet>(endian: Endian) -> Self
    where
        Packet: for<'args> BinRead<Args<'args> = ()> + Send + 'static,
    {
        Self {
            endian,
            decode: decode::<Packet>,
        }
    }

    pub const fn big_endian<Packet>() -> Self
    where
        Packet: for<'args> BinRead<Args<'args> = ()> + Send + 'static,
    {
        Self::new::<Packet>(Endian::Big)
    }

    pub const fn little_endian<Packet>() -> Self
    where
        Packet: for<'args> BinRead<Args<'args> = ()> + Send + 'static,
    {
        Self::new::<Packet>(Endian::Little)
    }
}

impl<Metadata> PacketDecoder<Metadata> for BinrwPacketDecoder {
    type Output = ErasedPacket;
    type Error = ::binrw::Error;

    fn decode(
        &self,
        _metadata: &Metadata,
        payload: &mut Cursor<Bytes>,
    ) -> Result<Self::Output, Self::Error> {
        (self.decode)(self.endian, payload)
    }
}

fn decode<Packet>(endian: Endian, payload: &mut Cursor<Bytes>) -> BinResult<ErasedPacket>
where
    Packet: for<'args> BinRead<Args<'args> = ()> + Send + 'static,
{
    Packet::read_options(payload, endian, ()).map(ErasedPacket::new)
}

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
        H: Handler<Context, Error = Error>,
        H::Inbound: for<'args> BinRead<Args<'args> = ()>,
        H::Outbound: for<'args> BinWrite<Args<'args> = ()>,
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
            Error = Error,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
            Outbound: for<'args> BinWrite<Args<'args> = ()>,
        >,
    ) -> Self {
        Self::new(Endian::Big, handler)
    }

    pub const fn little_endian(
        handler: &'static impl Handler<
            Context,
            Error = Error,
            Inbound: for<'args> BinRead<Args<'args> = ()>,
            Outbound: for<'args> BinWrite<Args<'args> = ()>,
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
    type Output = Box<dyn ErasedHandler<Context, BinrwOutbound, Error>>;
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
    H: Handler<Context>,
{
    inbound: H::Inbound,
    endian: Endian,
    handler: &'static H,
}

#[async_trait::async_trait]
impl<Context, Error, H> ErasedHandler<Context, BinrwOutbound, Error>
    for BinrwBoundHandler<Context, H>
where
    Context: Send + 'static,
    Error: Send + 'static,
    H: Handler<Context, Error = Error>,
    H::Outbound: for<'args> BinWrite<Args<'args> = ()>,
{
    fn mode(&self) -> crate::DispatchMode {
        H::MODE
    }

    async fn handle(self: Box<Self>, context: Context) -> Result<Vec<BinrwOutbound>, Error> {
        self.handler
            .handle(context, self.inbound)
            .await
            .map(|outbounds| {
                outbounds
                    .into_iter()
                    .map(|outbound| BinrwOutbound::new(outbound, self.endian))
                    .collect()
            })
    }
}

fn decode_handler<Context, Error, H>(
    endian: Endian,
    handler: HandlerRef,
    payload: &mut Cursor<Bytes>,
) -> BinResult<Box<dyn ErasedHandler<Context, BinrwOutbound, Error>>>
where
    Context: Send + 'static,
    Error: Send + 'static,
    H: Handler<Context, Error = Error>,
    H::Inbound: for<'args> BinRead<Args<'args> = ()>,
    H::Outbound: for<'args> BinWrite<Args<'args> = ()>,
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

#[cfg(test)]
mod tests {
    use ::binrw::BinRead;

    use super::*;

    #[derive(BinRead)]
    struct Word(u16);

    #[test]
    fn applies_the_selected_endian() {
        for (decoder, expected) in [
            (BinrwPacketDecoder::big_endian::<Word>(), 0x0102),
            (BinrwPacketDecoder::little_endian::<Word>(), 0x0201),
        ] {
            let mut payload = Cursor::new(Bytes::from_static(&[1, 2]));
            let packet = decoder.decode(&(), &mut payload).unwrap();

            assert_eq!(packet.downcast_ref::<Word>().unwrap().0, expected);
        }
    }
}
