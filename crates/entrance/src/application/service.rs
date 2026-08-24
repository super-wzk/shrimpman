use std::{num::NonZeroUsize, sync::Arc, time::Instant};

use futures_util::StreamExt;
use shrimpman_protocol::{CommandPacketDecoder, Dispatcher, PacketStream, outbound_channel};
use shrimpman_transport::MhfConnection;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tracing::Instrument;

use super::{ConnectionError, EntranceServiceContext, EntranceSessionContext, InternalError};
use crate::{
    envelope::EntranceCommandDecoder,
    router::{EntranceRouter, EntranceRouterBuildError},
};

const INITIALIZATION_LEN: usize = 8;

type EntrancePacketStream<Io> =
    PacketStream<MhfConnection<Io>, CommandPacketDecoder<EntranceCommandDecoder, EntranceRouter>>;

/// Serves the Entrance protocol over accepted connections.
pub struct EntranceService {
    context: Arc<EntranceServiceContext>,
    router: EntranceRouter,
}

impl EntranceService {
    /// Builds the service and validates all distributed packet registrations.
    pub fn new(context: EntranceServiceContext) -> Result<Self, EntranceRouterBuildError> {
        Ok(Self {
            context: Arc::new(context),
            router: EntranceRouter::new()?,
        })
    }

    /// Runs the complete Entrance exchange for one accepted byte stream.
    ///
    /// Entrance connections carry one request. The service consumes the raw
    /// initialization prelude, dispatches that request, sends every response
    /// in order, and closes the encrypted connection.
    pub async fn serve_connection<Io>(&self, mut io: Io) -> Result<(), ConnectionError>
    where
        Io: AsyncRead + AsyncWrite + Unpin,
    {
        let mut initialization = [0; INITIALIZATION_LEN];
        io.read_exact(&mut initialization)
            .await
            .map_err(ConnectionError::Initialization)?;

        let context = EntranceSessionContext::new(Arc::clone(&self.context));

        EntranceSession::new(io, context, self.router).run().await
    }
}

struct EntranceSession<Io> {
    context: EntranceSessionContext,
    packets: EntrancePacketStream<Io>,
    dispatcher: Dispatcher<InternalError>,
}

impl<Io> EntranceSession<Io> {
    fn new(io: Io, context: EntranceSessionContext, router: EntranceRouter) -> Self {
        let connection = MhfConnection::new(io);
        let decoder = CommandPacketDecoder::new(EntranceCommandDecoder, router);

        Self {
            context,
            packets: PacketStream::new(connection, decoder),
            dispatcher: Dispatcher::new(NonZeroUsize::MIN),
        }
    }
}

impl<Io> EntranceSession<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(self) -> Result<(), ConnectionError> {
        let Self {
            context,
            mut packets,
            mut dispatcher,
        } = self;
        let decoded = packets
            .next()
            .await
            .ok_or(ConnectionError::UnexpectedEof)?
            .map_err(ConnectionError::Receive)?;
        let (command, (), handler) = decoded.into_parts();
        let (outbound, receiver) = outbound_channel(NonZeroUsize::MIN);

        let dispatch = async move {
            dispatcher
                .dispatch(handler, context, outbound)
                .await
                .map_err(ConnectionError::Dispatch)?;
            dispatcher.finish().await.map_err(ConnectionError::Dispatch)
        };

        let writer = async move {
            receiver
                .forward_to(&mut packets)
                .await
                .map_err(ConnectionError::Send)?;
            packets.close().await.map_err(ConnectionError::Close)
        };
        let span = tracing::info_span!("entrance_request", command = command.as_str());
        async move {
            let started_at = Instant::now();
            tracing::info!("Processing Entrance request");
            tokio::try_join!(dispatch, writer)?;
            tracing::info!(
                elapsed_ms = started_at.elapsed().as_millis(),
                "Completed Entrance request"
            );
            Ok(())
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use binrw::{BinRead, BinWrite};
    use bytes::Bytes;
    use futures_util::SinkExt;
    use shrimpman_discovery::client::DiscoveryClient;
    use shrimpman_protocol::{BinrwOutboundSender, Handler};
    use tokio::io::{AsyncWriteExt, duplex};

    use super::*;
    use crate::router::EntrancePacketRegistration;

    #[derive(BinRead)]
    struct TestRequest(u8);

    #[derive(BinWrite)]
    struct TestResponse(u8);

    struct TestHandler;
    static RECEIVED_VALUE: AtomicU8 = AtomicU8::new(0);

    #[async_trait::async_trait]
    impl Handler<EntranceSessionContext, BinrwOutboundSender> for TestHandler {
        type Inbound = TestRequest;
        type Error = InternalError;

        async fn handle(
            &self,
            _context: EntranceSessionContext,
            inbound: Self::Inbound,
            outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            RECEIVED_VALUE.store(inbound.0, Ordering::Relaxed);
            outbound.send(TestResponse(inbound.0 + 1)).await?;
            Ok(())
        }
    }

    inventory::submit! {
        EntrancePacketRegistration::new(
            &["TEST-SERVICE"],
            &TestHandler,
        )
    }

    fn service() -> EntranceService {
        EntranceService::new(EntranceServiceContext::new(
            DiscoveryClient::connect(Default::default()).unwrap(),
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn dispatches_a_null_terminated_command_then_closes_the_connection() {
        RECEIVED_VALUE.store(0, Ordering::Relaxed);
        let (mut client_io, server_io) = duplex(4096);
        let service = service();
        let server = tokio::spawn(async move { service.serve_connection(server_io).await });

        client_io.write_all(&[0; INITIALIZATION_LEN]).await.unwrap();
        let mut client = MhfConnection::new(client_io);
        client
            .send(Bytes::from_static(b"TEST-SERVICE\0\x07"))
            .await
            .unwrap();

        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[8])
        );
        assert!(client.next().await.is_none());
        server.await.unwrap().unwrap();
        assert_eq!(RECEIVED_VALUE.load(Ordering::Relaxed), 7);
    }

    #[tokio::test]
    async fn rejects_connections_without_a_complete_initialization() {
        let (mut client_io, server_io) = duplex(64);
        let service = service();
        let server = tokio::spawn(async move { service.serve_connection(server_io).await });

        client_io.write_all(&[0; 7]).await.unwrap();
        client_io.shutdown().await.unwrap();

        assert!(matches!(
            server.await.unwrap(),
            Err(ConnectionError::Initialization(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof
        ));
    }
}
