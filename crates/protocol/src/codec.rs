use std::{any::Any, io::Cursor};

use bytes::Bytes;
use thiserror::Error;

use crate::{PayloadDecode, PayloadDecoder, RouteResolver};

/// A decoded command and the metadata carried by its envelope.
pub struct DecodedCommand<Command, Metadata> {
    command: Command,
    metadata: Metadata,
}

impl<Command, Metadata> DecodedCommand<Command, Metadata> {
    pub const fn new(command: Command, metadata: Metadata) -> Self {
        Self { command, metadata }
    }

    pub fn into_parts(self) -> (Command, Metadata) {
        (self.command, self.metadata)
    }
}

/// Decodes a service envelope into its command key and metadata.
pub trait CommandDecoder {
    type Command;
    type Metadata;
    type Error;

    fn decode_command(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<DecodedCommand<Self::Command, Self::Metadata>, Self::Error>;
}

/// A decoded packet whose concrete body type is retained internally.
pub struct ErasedPacket {
    value: Box<dyn Any + Send>,
}

impl ErasedPacket {
    pub fn new<Packet>(packet: Packet) -> Self
    where
        Packet: Send + 'static,
    {
        Self {
            value: Box::new(packet),
        }
    }

    pub fn downcast_ref<Packet>(&self) -> Option<&Packet>
    where
        Packet: 'static,
    {
        self.value.downcast_ref()
    }

    pub fn downcast<Packet>(self) -> Result<Packet, Self>
    where
        Packet: Send + 'static,
    {
        let Self { value } = self;

        match value.downcast() {
            Ok(packet) => Ok(*packet),
            Err(value) => Err(Self { value }),
        }
    }
}

/// Decodes a packet body selected by a route.
pub trait PacketDecoder<Metadata> {
    type Output;
    type Error;

    fn decode(
        &self,
        metadata: &Metadata,
        payload: &mut Cursor<Bytes>,
    ) -> Result<Self::Output, Self::Error>;
}

/// A routed packet together with its original command metadata.
pub struct DecodedPacket<Command, Metadata, Packet> {
    command: Command,
    metadata: Metadata,
    packet: Packet,
}

impl<Command, Metadata, Packet> DecodedPacket<Command, Metadata, Packet> {
    pub const fn command(&self) -> &Command {
        &self.command
    }

    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub const fn packet(&self) -> &Packet {
        &self.packet
    }

    pub fn into_parts(self) -> (Command, Metadata, Packet) {
        (self.command, self.metadata, self.packet)
    }
}

/// An error produced while decoding and routing a command-oriented packet.
#[derive(Debug, Error)]
pub enum CommandPacketDecodeError<CommandError, RouteError, PacketError> {
    #[error("failed to decode command: {0}")]
    Command(#[source] CommandError),

    #[error("failed to resolve packet route: {0}")]
    Route(#[source] RouteError),

    #[error("failed to decode packet: {0}")]
    Decode(#[source] PacketError),
}

/// Incrementally decodes packets by composing command, route, and body decoders.
pub struct CommandPacketDecoder<Decoder, Routes> {
    command_decoder: Decoder,
    routes: Routes,
}

impl<Decoder, Routes> CommandPacketDecoder<Decoder, Routes> {
    pub const fn new(command_decoder: Decoder, routes: Routes) -> Self {
        Self {
            command_decoder,
            routes,
        }
    }
}

impl<Decoder, Routes> PayloadDecoder for CommandPacketDecoder<Decoder, Routes>
where
    Decoder: CommandDecoder,
    Routes: RouteResolver<Decoder::Command, Decoder::Metadata>,
    Routes::Target: PacketDecoder<Decoder::Metadata>,
{
    type Inbound = DecodedPacket<
        Decoder::Command,
        Decoder::Metadata,
        <Routes::Target as PacketDecoder<Decoder::Metadata>>::Output,
    >;
    type Error = CommandPacketDecodeError<
        Decoder::Error,
        Routes::Error,
        <Routes::Target as PacketDecoder<Decoder::Metadata>>::Error,
    >;

    fn decode_next(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<PayloadDecode<Self::Inbound>, Self::Error> {
        if payload.position() == payload.get_ref().len() as u64 {
            return Ok(PayloadDecode::Complete);
        }

        let decoded = self
            .command_decoder
            .decode_command(payload)
            .map_err(CommandPacketDecodeError::Command)?;
        let (command, metadata) = decoded.into_parts();
        let target = self
            .routes
            .resolve(&command, &metadata)
            .map_err(CommandPacketDecodeError::Route)?;
        let packet = target
            .decode(&metadata, payload)
            .map_err(CommandPacketDecodeError::Decode)?;

        Ok(PayloadDecode::Item(DecodedPacket {
            command,
            metadata,
            packet,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, io::Read, num::NonZeroUsize};

    use super::*;
    use binrw::BinRead;

    use crate::{BinrwHandlerDecoder, Dispatcher, EncodePayload, Handler};

    struct ByteCommandDecoder;

    impl CommandDecoder for ByteCommandDecoder {
        type Command = u8;
        type Metadata = ();
        type Error = std::io::Error;

        fn decode_command(
            &mut self,
            payload: &mut Cursor<Bytes>,
        ) -> Result<DecodedCommand<Self::Command, Self::Metadata>, Self::Error> {
            let mut command = [0];
            payload.read_exact(&mut command)?;
            Ok(DecodedCommand::new(command[0], ()))
        }
    }

    struct BytePacketDecoder;

    impl PacketDecoder<()> for BytePacketDecoder {
        type Output = u8;
        type Error = std::io::Error;

        fn decode(
            &self,
            _metadata: &(),
            payload: &mut Cursor<Bytes>,
        ) -> Result<Self::Output, Self::Error> {
            let mut body = [0];
            payload.read_exact(&mut body)?;
            Ok(body[0])
        }
    }

    struct ByteRoutes;

    impl RouteResolver<u8, ()> for ByteRoutes {
        type Target = BytePacketDecoder;
        type Error = Infallible;

        fn resolve(&self, _key: &u8, _metadata: &()) -> Result<Self::Target, Self::Error> {
            Ok(BytePacketDecoder)
        }
    }

    #[derive(BinRead)]
    struct Add(u8);
    struct AddHandler {
        offset: u8,
    }
    static ADD_HANDLER: AddHandler = AddHandler { offset: 1 };

    #[async_trait::async_trait]
    impl Handler<u8> for AddHandler {
        type Inbound = Add;
        type Outbound = u8;
        type Error = Infallible;

        async fn handle(
            &self,
            context: u8,
            inbound: Self::Inbound,
        ) -> Result<Vec<Self::Outbound>, Self::Error> {
            Ok(vec![inbound.0 + context + self.offset])
        }
    }

    struct HandlerRoutes;

    impl RouteResolver<u8, ()> for HandlerRoutes {
        type Target = BinrwHandlerDecoder<u8, Infallible>;
        type Error = Infallible;

        fn resolve(&self, _key: &u8, _metadata: &()) -> Result<Self::Target, Self::Error> {
            Ok(BinrwHandlerDecoder::big_endian(&ADD_HANDLER))
        }
    }

    #[test]
    fn routes_commands_without_exposing_the_packet_type() {
        let mut decoder = CommandPacketDecoder::new(ByteCommandDecoder, ByteRoutes);
        let mut payload = Cursor::new(Bytes::from_static(&[7, 42]));

        let PayloadDecode::Item(decoded) = decoder.decode_next(&mut payload).unwrap() else {
            panic!("decoder finished before producing a packet")
        };

        assert_eq!(*decoded.command(), 7);
        assert_eq!(*decoded.packet(), 42);
    }

    #[tokio::test]
    async fn decoded_handlers_can_be_dispatched_without_downcasting() {
        let mut decoder = CommandPacketDecoder::new(ByteCommandDecoder, HandlerRoutes);
        let mut payload = Cursor::new(Bytes::from_static(&[7, 42]));

        let PayloadDecode::Item(decoded) = decoder.decode_next(&mut payload).unwrap() else {
            panic!("decoder finished before producing a handler")
        };
        let (_, _, handler) = decoded.into_parts();
        let mut dispatcher = Dispatcher::new(NonZeroUsize::new(1).unwrap());

        let outbounds = dispatcher
            .dispatch_erased(handler, 8)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            outbounds.into_iter().next().unwrap().encode().unwrap(),
            Bytes::from_static(&[51])
        );
    }

    #[test]
    fn failed_downcast_preserves_the_packet() {
        let packet = ErasedPacket::new(42_u8);
        let packet = packet.downcast::<u16>().unwrap_err();

        assert_eq!(packet.downcast::<u8>().ok(), Some(42));
    }
}
