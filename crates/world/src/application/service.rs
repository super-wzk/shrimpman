use std::{io::Cursor, num::NonZeroUsize};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use shrimpman_domain::world::LandKey;
use shrimpman_protocol::{
    BinrwOutbound, CommandPacketDecoder, Dispatcher, PacketStream, PayloadDecode, PayloadDecoder,
    outbound_channel,
};
use shrimpman_transport::MhfConnection;
use tokio::io::{AsyncRead, AsyncWrite};

use super::{ConnectionError, InternalError, PacketDecodeError};
use crate::{
    envelope::{LandCommandDecoder, MSG_SYS_END},
    router::{LandRouter, LandRouterBuildError},
};

type RoutedPacketDecoder = CommandPacketDecoder<LandCommandDecoder, LandRouter>;
type LandPacketStream<Io> = PacketStream<MhfConnection<Io>, SinglePacketDecoder>;

/// Coordinates the Land connections owned by one World process.
pub struct WorldService {
    router: LandRouter,
}

impl WorldService {
    /// Builds the service and validates all distributed packet registrations.
    pub fn new() -> Result<Self, LandRouterBuildError> {
        Ok(Self {
            router: LandRouter::new()?,
        })
    }

    /// Runs a persistent Land exchange without a Sign/Entrance initialization prelude.
    pub async fn serve_land_connection<Io>(
        &self,
        io: Io,
        land: LandKey,
    ) -> Result<(), ConnectionError>
    where
        Io: AsyncRead + AsyncWrite + Unpin,
    {
        LandSession::new(io, land, self.router).run().await
    }
}

struct LandSession<Io> {
    land: LandKey,
    packets: LandPacketStream<Io>,
    dispatcher: Dispatcher<InternalError>,
}

impl<Io> LandSession<Io> {
    fn new(io: Io, land: LandKey, router: LandRouter) -> Self {
        let connection = MhfConnection::new(io);
        let decoder = SinglePacketDecoder::new(router);

        Self {
            land,
            packets: PacketStream::new(connection, decoder),
            dispatcher: Dispatcher::new(NonZeroUsize::MIN),
        }
    }
}

impl<Io> LandSession<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(self) -> Result<(), ConnectionError> {
        let Self {
            land,
            packets,
            mut dispatcher,
        } = self;
        let (mut writer, mut reader) = packets.split();
        let (outbound, receiver) = outbound_channel::<BinrwOutbound>(NonZeroUsize::MIN);

        let dispatch = async move {
            while let Some(decoded) = reader.next().await {
                let decoded = decoded.map_err(ConnectionError::Receive)?;
                let (opcode, (), handler) = decoded.into_parts();
                tracing::debug!(
                    opcode = format_args!("{opcode:#06x}"),
                    "Dispatching Land packet"
                );
                dispatcher
                    .dispatch(handler, land.clone(), outbound.clone())
                    .await
                    .map_err(ConnectionError::Dispatch)?;
            }

            dispatcher.finish().await.map_err(ConnectionError::Dispatch)
        };

        let send = async move {
            receiver
                .forward_to(&mut writer)
                .await
                .map_err(ConnectionError::Send)?;
            writer.close().await.map_err(ConnectionError::Send)
        };

        tokio::try_join!(dispatch, send)?;
        Ok(())
    }
}

/// Removes and validates the group terminator before routing one packet body.
///
/// Land batching is intentionally rejected for now: the routed decoder must
/// consume every byte before the final MSG_SYS_END marker.
struct SinglePacketDecoder {
    routed: RoutedPacketDecoder,
}

impl SinglePacketDecoder {
    fn new(router: LandRouter) -> Self {
        Self {
            routed: CommandPacketDecoder::new(LandCommandDecoder, router),
        }
    }
}

impl PayloadDecoder for SinglePacketDecoder {
    type Inbound = <RoutedPacketDecoder as PayloadDecoder>::Inbound;
    type Error = PacketDecodeError;

    fn decode_next(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<PayloadDecode<Self::Inbound>, Self::Error> {
        let start = payload.position() as usize;
        let payload_len = payload.get_ref().len();
        let Some(packet_end) = payload_len.checked_sub(size_of::<u16>()) else {
            return Err(PacketDecodeError::MissingEndMarker);
        };
        if packet_end < start {
            return Err(PacketDecodeError::MissingEndMarker);
        }

        let end = &payload.get_ref()[packet_end..];
        let actual = u16::from_be_bytes([end[0], end[1]]);
        if actual != MSG_SYS_END {
            return Err(PacketDecodeError::InvalidEndMarker { actual });
        }

        let mut packet = Cursor::new(payload.get_ref().slice(start..packet_end));
        let decoded = self
            .routed
            .decode_next(&mut packet)
            .map_err(PacketDecodeError::Packet)?;
        let remaining = packet.get_ref().len() as u64 - packet.position();
        if remaining != 0 {
            return Err(PacketDecodeError::TrailingBody { remaining });
        }

        payload.set_position(payload_len as u64);
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use binrw::{BinRead, BinWrite};
    use futures_util::{SinkExt, StreamExt};
    use shrimpman_protocol::{BinrwOutboundSender, Handler};
    use tokio::io::duplex;

    use super::*;
    use crate::{
        envelope::{LandOutboundSender, LandPacket},
        router::LandPacketRegistration,
    };

    const REQUEST_OPCODE: u16 = 0x1234;
    const SECOND_RESPONSE_OPCODE: u16 = 0xcdef;

    #[derive(BinRead, BinWrite)]
    struct TestRequest(u8);

    impl LandPacket for TestRequest {
        const OPCODE: u16 = REQUEST_OPCODE;
    }

    #[derive(BinWrite)]
    struct SecondResponse(u16);

    impl LandPacket for SecondResponse {
        const OPCODE: u16 = SECOND_RESPONSE_OPCODE;
    }

    struct TestHandler;

    impl Handler<LandKey, BinrwOutboundSender> for TestHandler {
        type Inbound = TestRequest;
        type Error = InternalError;

        async fn handle(
            &self,
            _land: LandKey,
            inbound: Self::Inbound,
            outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            outbound
                .send_packet_and_flush(TestRequest(inbound.0 + 1))
                .await?;
            outbound
                .send_packet(SecondResponse(u16::from(inbound.0) + 2))
                .await?;
            Ok(())
        }
    }

    inventory::submit! {
        LandPacketRegistration::new(&TestHandler)
    }

    #[tokio::test]
    async fn reuses_a_packet_type_in_both_directions_and_frames_each_send() {
        let (client_io, server_io) = duplex(4096);
        let service = WorldService::new().unwrap();
        let server = tokio::spawn(async move {
            service
                .serve_land_connection(server_io, LandKey::from("test".to_owned()))
                .await
        });
        let mut client = MhfConnection::new(client_io);

        client
            .send(Bytes::from_static(&[0x12, 0x34, 7, 0x00, 0x10]))
            .await
            .unwrap();

        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[0x12, 0x34, 8, 0x00, 0x10])
        );
        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[0xcd, 0xef, 0x00, 0x09, 0x00, 0x10])
        );
        client.close().await.unwrap();
        server.await.unwrap().unwrap();
    }

    #[test]
    fn rejects_multiple_packets_before_the_end_marker() {
        let mut decoder = SinglePacketDecoder::new(LandRouter::new().unwrap());
        let mut payload = Cursor::new(Bytes::from_static(&[
            0x12, 0x34, 7, 0x12, 0x34, 8, 0x00, 0x10,
        ]));

        let error = match decoder.decode_next(&mut payload) {
            Ok(_) => panic!("multiple Land packets were accepted in one payload"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            PacketDecodeError::TrailingBody { remaining: 3 }
        ));
    }
}
