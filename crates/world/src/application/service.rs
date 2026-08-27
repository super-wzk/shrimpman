use std::{io::Cursor, num::NonZeroUsize, sync::Arc};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use shrimpman_domain::{character::CharacterId, world::LandKey};
use shrimpman_protocol::{
    BinrwOutbound, CommandPacketDecoder, Dispatcher, PacketStream, PayloadDecode, PayloadDecoder,
    outbound_channel,
};
use shrimpman_transport::MhfConnection;
use tokio::io::{AsyncRead, AsyncWrite};

use super::{
    ConnectionError, PacketDecodeError, WorldRepositories, WorldSession, WorldSessionContext,
    context::WorldServiceContext,
};
use crate::{
    envelope::{LandCommandDecoder, MSG_SYS_END},
    router::{LandInbound, LandRouter, LandRouterBuildError},
};

type RoutedPacketDecoder = CommandPacketDecoder<LandCommandDecoder, LandRouter>;
type LandPacketStream<Io> = PacketStream<MhfConnection<Io>, SinglePacketDecoder>;

/// Coordinates the Land connections owned by one World process.
pub struct WorldService {
    context: Arc<WorldServiceContext>,
    router: LandRouter,
}

impl WorldService {
    /// Builds the service and validates all distributed packet registrations.
    pub fn new(repositories: WorldRepositories) -> Result<Self, LandRouterBuildError> {
        Ok(Self {
            context: Arc::new(WorldServiceContext::new(repositories)),
            router: LandRouter::new()?,
        })
    }

    /// Finds the authenticated session currently bound to a character.
    pub fn session(&self, character_id: CharacterId) -> Option<WorldSession> {
        self.context.session(character_id)
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
        LandSession::new(io, land, Arc::clone(&self.context), self.router)
            .run()
            .await
    }
}

struct LandSession<Io> {
    context: WorldSessionContext,
    packets: LandPacketStream<Io>,
}

impl<Io> LandSession<Io> {
    fn new(io: Io, land: LandKey, service: Arc<WorldServiceContext>, router: LandRouter) -> Self {
        let connection = MhfConnection::new(io);
        let decoder = SinglePacketDecoder::new(router);

        Self {
            context: WorldSessionContext::new(service, land),
            packets: PacketStream::new(connection, decoder),
        }
    }
}

impl<Io> LandSession<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(self) -> Result<(), ConnectionError> {
        let Self { context, packets } = self;
        let (mut writer, mut reader) = packets.split();
        let (outbound, outbound_receiver) = outbound_channel::<BinrwOutbound>(NonZeroUsize::MIN);
        let _session_guard = context.guard();
        let mut dispatcher = Dispatcher::new(NonZeroUsize::MIN);
        // Reading must not wait behind an ordered handler: its response arrives on this stream.
        let (handler_sender, mut handler_receiver) = tokio::sync::mpsc::unbounded_channel();

        let receive_context = context.clone();
        let receive = async move {
            loop {
                let decoded = tokio::select! {
                    _ = receive_context.cancelled() => break,
                    decoded = reader.next() => decoded,
                };
                let Some(decoded) = decoded else {
                    receive_context.disconnect();
                    break;
                };
                let decoded = match decoded {
                    Ok(decoded) => decoded,
                    Err(error) => {
                        receive_context.disconnect();
                        return Err(ConnectionError::Receive(error));
                    }
                };
                let (opcode, (), inbound) = decoded.into_parts();
                match inbound {
                    LandInbound::Handler(handler) => {
                        if handler_sender.send((opcode, handler)).is_err() {
                            receive_context.disconnect();
                            break;
                        }
                    }
                    LandInbound::Response(response) => {
                        let handle = u32::from(response.handle());
                        if !receive_context.complete_response(response) {
                            tracing::debug!(handle, "Ignored unmatched World response");
                        }
                    }
                }
            }

            Ok(())
        };

        let dispatch_context = context.clone();
        let dispatch = async move {
            loop {
                let handler = tokio::select! {
                    _ = dispatch_context.cancelled() => break,
                    handler = handler_receiver.recv() => handler,
                };
                let Some((opcode, handler)) = handler else {
                    break;
                };
                tracing::debug!(
                    opcode = format_args!("{opcode:#06x}"),
                    "Dispatching Land packet"
                );
                tokio::select! {
                    _ = dispatch_context.cancelled() => break,
                    result = dispatcher.dispatch(
                        handler,
                        dispatch_context.clone(),
                        outbound.clone(),
                    ) => result.map_err(ConnectionError::Dispatch)?,
                }
            }

            dispatcher.finish().await.map_err(ConnectionError::Dispatch)
        };

        let send_context = context.clone();
        let send = async move {
            tokio::select! {
                result = outbound_receiver.forward_to(&mut writer) => {
                    result.map_err(ConnectionError::Send)?;
                }
                _ = send_context.cancelled() => {}
            }
            writer.close().await.map_err(ConnectionError::Send)
        };

        tokio::try_join!(receive, dispatch, send)?;
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
    use jiff::{SignedDuration, Timestamp};
    use shrimpman_domain::{
        TimeRange,
        account::AccountId,
        character::CharacterId,
        session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
    };
    use shrimpman_persistence::{AccountRepository, CharacterRepository, SignSessionRepository};
    use shrimpman_protocol::Handler;
    use tokio::io::duplex;

    use super::*;
    use crate::{
        application::InternalError,
        envelope::LandPacket,
        exchange::LandExchange,
        response::{Buffered, MSG_SYS_ACK},
        router::LandRouteRegistration,
    };

    const ECHO_OPCODE: u16 = 0x1234;
    const EXTRA_PACKET_OPCODE: u16 = 0xcdef;
    const START_SERVER_REQUEST_OPCODE: u16 = 0x2345;
    const SERVER_REQUEST_OPCODE: u16 = 0x3456;
    const REQUEST_RESULT_OPCODE: u16 = 0x4567;
    const SESSION_TOKEN: [u8; SIGN_SESSION_TOKEN_LEN] = *b"0123456789ABCDEF";
    const INVALID_SESSION_TOKEN: [u8; SIGN_SESSION_TOKEN_LEN] = *b"FEDCBA9876543210";

    #[derive(BinRead, BinWrite)]
    struct EchoPacket(u8);

    impl LandPacket for EchoPacket {
        const OPCODE: u16 = ECHO_OPCODE;
    }

    #[derive(BinWrite)]
    struct ExtraPacket(u16);

    impl LandPacket for ExtraPacket {
        const OPCODE: u16 = EXTRA_PACKET_OPCODE;
    }

    struct EchoHandler;

    impl Handler<WorldSessionContext, LandExchange> for EchoHandler {
        type Inbound = EchoPacket;
        type Error = InternalError;

        async fn handle(
            &self,
            _context: WorldSessionContext,
            inbound: Self::Inbound,
            exchange: LandExchange,
        ) -> Result<(), Self::Error> {
            exchange
                .send_packet_and_flush(EchoPacket(inbound.0 + 1))
                .await?;
            exchange
                .send_packet(ExtraPacket(u16::from(inbound.0) + 2))
                .await?;
            Ok(())
        }
    }

    inventory::submit! {
        LandRouteRegistration::new(&EchoHandler)
    }

    #[derive(BinRead)]
    struct StartServerRequest;

    impl LandPacket for StartServerRequest {
        const OPCODE: u16 = START_SERVER_REQUEST_OPCODE;
    }

    #[derive(BinWrite)]
    struct ServerRequest(u8);

    impl LandPacket for ServerRequest {
        const OPCODE: u16 = SERVER_REQUEST_OPCODE;
    }

    #[derive(BinWrite)]
    struct RequestResult(u16);

    impl LandPacket for RequestResult {
        const OPCODE: u16 = REQUEST_RESULT_OPCODE;
    }

    struct ServerRequestHandler;

    impl Handler<WorldSessionContext, LandExchange> for ServerRequestHandler {
        type Inbound = StartServerRequest;
        type Error = InternalError;

        async fn handle(
            &self,
            _context: WorldSessionContext,
            _inbound: Self::Inbound,
            exchange: LandExchange,
        ) -> Result<(), Self::Error> {
            let result = match exchange.request::<Buffered<u16>>(ServerRequest(7)).await? {
                Ok(result) => result + 1,
                Err(_) => 0,
            };
            exchange
                .send_packet_and_flush(RequestResult(result))
                .await?;
            Ok(())
        }
    }

    inventory::submit! {
        LandRouteRegistration::new(&ServerRequestHandler)
    }

    struct LoginFixture {
        service: Arc<WorldService>,
        account_id: AccountId,
        character_id: CharacterId,
        session_id: SignSessionId,
    }

    async fn login_fixture() -> LoginFixture {
        let db = crate::test_database().await;

        let accounts = AccountRepository::new(&db);
        let characters = CharacterRepository::new(&db);
        let sign_sessions = SignSessionRepository::new(&db);
        let account = accounts
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
        let character = characters.create_new(&account).await.unwrap();
        let session_id = sign_sessions
            .create(
                &account,
                &SESSION_TOKEN,
                TimeRange::from_duration(Timestamp::now(), SignedDuration::from_mins(5)),
            )
            .await
            .unwrap();
        let service = Arc::new(WorldService::new(crate::test_repositories(&db)).unwrap());

        LoginFixture {
            service,
            account_id: account.id,
            character_id: character.id,
            session_id,
        }
    }

    fn login_payload(
        request_handle: u32,
        character_id: CharacterId,
        session_id: SignSessionId,
        token: &[u8; SIGN_SESSION_TOKEN_LEN],
    ) -> Bytes {
        let character_id = u32::from(character_id);
        let mut payload = Vec::new();
        payload.extend_from_slice(&0x0014_u16.to_be_bytes());
        payload.extend_from_slice(&request_handle.to_be_bytes());
        payload.extend_from_slice(&character_id.to_be_bytes());
        payload.extend_from_slice(&u32::from(session_id).to_be_bytes());
        payload.extend_from_slice(&0_u16.to_be_bytes());
        payload.extend_from_slice(&11_u16.to_be_bytes());
        payload.extend_from_slice(&character_id.to_be_bytes());
        payload.extend_from_slice(&0_u16.to_be_bytes());
        payload.extend_from_slice(&(SIGN_SESSION_TOKEN_LEN as u16 + 1).to_be_bytes());
        payload.extend_from_slice(token);
        payload.push(0);
        payload.extend_from_slice(&MSG_SYS_END.to_be_bytes());
        Bytes::from(payload)
    }

    #[tokio::test]
    async fn reuses_a_packet_type_in_both_directions_and_frames_each_send() {
        let (client_io, server_io) = duplex(4096);
        let db = crate::test_database().await;
        let service = WorldService::new(crate::test_repositories(&db)).unwrap();
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

    #[tokio::test]
    async fn routes_success_and_failure_responses_without_closing_the_connection() {
        let (client_io, server_io) = duplex(4096);
        let db = crate::test_database().await;
        let service = WorldService::new(crate::test_repositories(&db)).unwrap();
        let server = tokio::spawn(async move {
            service
                .serve_land_connection(server_io, LandKey::from("test".to_owned()))
                .await
        });
        let mut client = MhfConnection::new(client_io);

        client
            .send(Bytes::from_static(&[0x23, 0x45, 0x00, 0x10]))
            .await
            .unwrap();

        let request = client.next().await.unwrap().unwrap();
        assert_eq!(&request[..2], &SERVER_REQUEST_OPCODE.to_be_bytes());
        assert_eq!(&request[6..], &[7, 0, 0x10]);
        let handle = &request[2..6];
        let mut response = Vec::new();
        response.extend_from_slice(&MSG_SYS_ACK.to_be_bytes());
        response.extend_from_slice(handle);
        response.push(1);
        response.push(0);
        response.extend_from_slice(&2_u16.to_be_bytes());
        response.extend_from_slice(&0x1234_u16.to_be_bytes());
        response.extend_from_slice(&MSG_SYS_END.to_be_bytes());
        client.send(Bytes::from(response)).await.unwrap();

        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[0x45, 0x67, 0x12, 0x35, 0x00, 0x10])
        );

        client
            .send(Bytes::from_static(&[0x23, 0x45, 0x00, 0x10]))
            .await
            .unwrap();

        let request = client.next().await.unwrap().unwrap();
        let mut response = Vec::new();
        response.extend_from_slice(&MSG_SYS_ACK.to_be_bytes());
        response.extend_from_slice(&request[2..6]);
        response.push(0);
        response.push(1);
        response.extend_from_slice(&0_u16.to_be_bytes());
        response.extend_from_slice(&0_u32.to_be_bytes());
        response.extend_from_slice(&MSG_SYS_END.to_be_bytes());
        client.send(Bytes::from(response)).await.unwrap();

        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[0x45, 0x67, 0x00, 0x00, 0x00, 0x10])
        );

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

    #[tokio::test]
    async fn authenticates_and_indexes_a_live_world_session() {
        let fixture = login_fixture().await;
        let service = Arc::clone(&fixture.service);
        let (client_io, server_io) = duplex(4096);
        let land = LandKey::from("test".to_owned());
        let server = tokio::spawn({
            let service = Arc::clone(&service);
            let land = land.clone();
            async move { service.serve_land_connection(server_io, land).await }
        });
        let mut client = MhfConnection::new(client_io);

        client
            .send(login_payload(
                7,
                fixture.character_id,
                fixture.session_id,
                &SESSION_TOKEN,
            ))
            .await
            .unwrap();

        let response = client.next().await.unwrap().unwrap();
        assert_eq!(&response[..2], &MSG_SYS_ACK.to_be_bytes());
        assert_eq!(&response[2..6], &7_u32.to_be_bytes());
        assert_eq!(&response[6..10], &[0, 0, 0, 0]);
        assert_eq!(&response[14..], &MSG_SYS_END.to_be_bytes());
        let session = service.session(fixture.character_id).unwrap();
        assert_eq!(session.account_id(), fixture.account_id);
        assert_eq!(session.character_id(), fixture.character_id);
        assert_eq!(session.land(), &land);

        session.disconnect();
        assert!(service.session(fixture.character_id).is_none());
        assert!(client.next().await.is_none());
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn closes_a_world_connection_with_an_invalid_login_token() {
        let fixture = login_fixture().await;
        let service = Arc::clone(&fixture.service);
        let (client_io, server_io) = duplex(4096);
        let server = tokio::spawn({
            let service = Arc::clone(&service);
            async move {
                service
                    .serve_land_connection(server_io, LandKey::from("test".to_owned()))
                    .await
            }
        });
        let mut client = MhfConnection::new(client_io);

        client
            .send(login_payload(
                7,
                fixture.character_id,
                fixture.session_id,
                &INVALID_SESSION_TOKEN,
            ))
            .await
            .unwrap();

        assert!(client.next().await.is_none());
        server.await.unwrap().unwrap();
        assert!(service.session(fixture.character_id).is_none());
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
