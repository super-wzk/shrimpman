use std::{collections::VecDeque, num::NonZeroUsize, sync::Arc};

use futures_util::{SinkExt, StreamExt};
use shrimpman_protocol::{BinrwOutbound, CommandPacketDecoder, Dispatcher, PacketStream};
use shrimpman_transport::MhfConnection;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

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
    dispatcher: Dispatcher<BinrwOutbound, InternalError>,
    outbound_queue: VecDeque<BinrwOutbound>,
}

impl<Io> SignSession<Io> {
    fn new(io: Io, context: SignSessionContext, router: SignRouter) -> Self {
        let connection = MhfConnection::new(io);
        let decoder = CommandPacketDecoder::new(SignCommandDecoder, router);

        Self {
            context,
            packets: PacketStream::new(connection, decoder),
            dispatcher: Dispatcher::new(NonZeroUsize::MIN),
            outbound_queue: VecDeque::new(),
        }
    }
}

impl<Io> SignSession<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(mut self) -> Result<(), ConnectionError> {
        let decoded = self
            .packets
            .next()
            .await
            .ok_or(ConnectionError::UnexpectedEof)?
            .map_err(ConnectionError::Receive)?;
        let (_, _, handler) = decoded.into_parts();

        if let Some(responses) = self
            .dispatcher
            .dispatch_erased(handler, self.context)
            .await
            .map_err(ConnectionError::Dispatch)?
        {
            self.outbound_queue.extend(responses);
        }

        while !self.dispatcher.is_idle() {
            let responses = self
                .dispatcher
                .next()
                .await
                .expect("a non-idle dispatcher has pending output")
                .map_err(ConnectionError::Dispatch)?;
            self.outbound_queue.extend(responses);
        }

        while let Some(response) = self.outbound_queue.pop_front() {
            self.packets
                .feed(response)
                .await
                .map_err(ConnectionError::Send)?;
        }

        SinkExt::<BinrwOutbound>::close(&mut self.packets)
            .await
            .map_err(ConnectionError::Send)
    }
}

#[cfg(test)]
mod tests {
    use binrw::{BinRead, BinWrite};
    use bytes::Bytes;
    use shrimpman_protocol::{DispatchMode, Handler};
    use tokio::io::{AsyncWriteExt, duplex};

    use super::*;
    use crate::router::{SignPacketRegistration, VersionSelector};

    #[derive(BinRead)]
    struct TestRequest(u8);

    #[derive(BinWrite)]
    struct TestResponse(u8);

    struct TestHandler;

    #[async_trait::async_trait]
    impl Handler<SignSessionContext> for TestHandler {
        type Inbound = TestRequest;
        type Outbound = TestResponse;
        type Error = InternalError;

        const MODE: DispatchMode = DispatchMode::Concurrent;

        async fn handle(
            &self,
            _context: SignSessionContext,
            inbound: Self::Inbound,
        ) -> Result<Vec<Self::Outbound>, Self::Error> {
            Ok(vec![
                TestResponse(inbound.0 + 1),
                TestResponse(inbound.0 + 2),
            ])
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
    async fn drains_queued_responses_and_closes_the_connection() {
        let (mut client_io, server_io) = duplex(4096);
        let service = SignService::new(SignServiceContext).unwrap();
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
        let service = SignService::new(SignServiceContext).unwrap();
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
