use std::{num::NonZeroUsize, sync::Arc, time::Instant};

use futures_util::StreamExt;
use shrimpman_protocol::{CommandPacketDecoder, Dispatcher, PacketStream, outbound_channel};
use shrimpman_transport::MhfConnection;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tracing::Instrument;

use super::{ConnectionError, InternalError, SignServiceContext, SignSessionContext};
use crate::{
    envelope::SignCommandDecoder,
    router::{SignRouter, SignRouterBuildError},
};

const INITIALIZATION_LEN: usize = 8;

type SignPacketStream<Io> =
    PacketStream<MhfConnection<Io>, CommandPacketDecoder<SignCommandDecoder, SignRouter>>;

/// Serves the Sign protocol over accepted connections.
pub struct SignService {
    context: Arc<SignServiceContext>,
    router: SignRouter,
}

impl SignService {
    /// Builds the service and validates all distributed packet registrations.
    pub fn new(context: SignServiceContext) -> Result<Self, SignRouterBuildError> {
        Ok(Self {
            context: Arc::new(context),
            router: SignRouter::new()?,
        })
    }

    /// Runs the complete Sign exchange for one accepted byte stream.
    ///
    /// Sign connections carry one request. The service consumes the raw
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

        let context = SignSessionContext::new(Arc::clone(&self.context));

        SignSession::new(io, context, self.router).run().await
    }
}

struct SignSession<Io> {
    context: SignSessionContext,
    packets: SignPacketStream<Io>,
    dispatcher: Dispatcher<InternalError>,
}

impl<Io> SignSession<Io> {
    fn new(io: Io, context: SignSessionContext, router: SignRouter) -> Self {
        let connection = MhfConnection::new(io);
        let decoder = CommandPacketDecoder::new(SignCommandDecoder, router);

        Self {
            context,
            packets: PacketStream::new(connection, decoder),
            dispatcher: Dispatcher::new(NonZeroUsize::MIN),
        }
    }
}

impl<Io> SignSession<Io>
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
        let (command, version, handler) = decoded.into_parts();
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
        let span = tracing::info_span!(
            "sign_request",
            command = command.as_str(),
            client_version = version.number()
        );
        async move {
            let started_at = Instant::now();
            tracing::info!("Processing Sign request");
            tokio::try_join!(dispatch, writer)?;
            tracing::info!(
                elapsed_ms = started_at.elapsed().as_millis(),
                "Completed Sign request"
            );
            Ok(())
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use binrw::{BinRead, BinWrite};
    use bytes::Bytes;
    use futures_util::SinkExt;
    use shrimpman_protocol::{BinrwOutboundSender, DispatchMode, Handler};
    use tokio::{
        io::{AsyncWriteExt, duplex},
        sync::Notify,
    };

    use super::*;
    use crate::router::{SignPacketRegistration, VersionSelector};

    #[derive(BinRead)]
    struct TestRequest(u8);

    #[derive(BinWrite)]
    struct TestResponse(u8);

    struct TestHandler;
    static CONTINUE_TEST_HANDLER: Notify = Notify::const_new();

    impl Handler<SignSessionContext, BinrwOutboundSender> for TestHandler {
        type Inbound = TestRequest;
        type Error = InternalError;

        const MODE: DispatchMode = DispatchMode::Concurrent;

        async fn handle(
            &self,
            _context: SignSessionContext,
            inbound: Self::Inbound,
            outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            outbound.send_and_flush(TestResponse(inbound.0 + 1)).await?;
            CONTINUE_TEST_HANDLER.notified().await;
            outbound.send(TestResponse(inbound.0 + 2)).await?;
            Ok(())
        }
    }

    inventory::submit! {
        SignPacketRegistration::new(
            &["TEST:"],
            VersionSelector::Any,
            &TestHandler,
        )
    }

    #[tokio::test]
    async fn flushes_then_drains_responses_before_closing_the_connection() {
        let (mut client_io, server_io) = duplex(4096);
        let service = SignService::new(SignServiceContext::for_test(true).await).unwrap();
        let server = tokio::spawn(async move { service.serve_connection(server_io).await });

        client_io.write_all(&[0; INITIALIZATION_LEN]).await.unwrap();
        let mut client = MhfConnection::new(client_io);
        client
            .send(Bytes::from_static(b"TEST:041\0\x07"))
            .await
            .unwrap();

        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[8])
        );
        CONTINUE_TEST_HANDLER.notify_one();
        assert_eq!(
            client.next().await.unwrap().unwrap(),
            Bytes::from_static(&[9])
        );
        assert!(client.next().await.is_none());
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn rejects_connections_without_a_complete_initialization() {
        let (mut client_io, server_io) = duplex(64);
        let service = SignService::new(SignServiceContext::for_test(true).await).unwrap();
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
