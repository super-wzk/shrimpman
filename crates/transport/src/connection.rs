use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_util::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use crate::{
    TransportError,
    codec::{MhfTransportCodec, TransportConfig},
};

/// An asynchronous MHF connection over an arbitrary byte stream.
///
/// The connection owns the encrypted framing state for one peer. Service-level
/// handshakes and payload parsing remain the caller's responsibility.
pub struct MhfConnection<Io> {
    framed: Framed<Io, MhfTransportCodec>,
}

impl<Io> MhfConnection<Io> {
    pub fn new(io: Io, config: TransportConfig) -> Self {
        Self {
            framed: Framed::new(io, MhfTransportCodec::new(config)),
        }
    }
}

impl<Io> Stream for MhfConnection<Io>
where
    Io: AsyncRead + Unpin,
{
    type Item = Result<Bytes, TransportError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        Pin::new(&mut this.framed).poll_next(context)
    }
}

impl<Io> Sink<Bytes> for MhfConnection<Io>
where
    Io: AsyncWrite + Unpin,
{
    type Error = TransportError;

    fn poll_ready(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.framed).poll_ready(context)
    }

    fn start_send(self: Pin<&mut Self>, payload: Bytes) -> Result<(), Self::Error> {
        let this = self.get_mut();
        Pin::new(&mut this.framed).start_send(payload)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.framed).poll_flush(context)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.framed).poll_close(context)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::duplex;

    use super::MhfConnection;
    use crate::TransportConfig;

    #[tokio::test]
    async fn exchanges_multiple_payloads_bidirectionally() {
        let (client_io, server_io) = duplex(4096);
        let mut client = MhfConnection::new(client_io, TransportConfig::default());
        let mut server = MhfConnection::new(server_io, TransportConfig::default());

        client.send(Bytes::from_static(b"first")).await.unwrap();
        client.send(Bytes::from_static(b"second")).await.unwrap();

        assert_eq!(server.next().await.unwrap().unwrap(), b"first"[..]);
        assert_eq!(server.next().await.unwrap().unwrap(), b"second"[..]);

        server.send(Bytes::from_static(b"response")).await.unwrap();
        assert_eq!(client.next().await.unwrap().unwrap(), b"response"[..]);

        client.close().await.unwrap();
        assert!(server.next().await.is_none());
    }
}
