use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
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

impl<Io> MhfConnection<Io>
where
    Io: AsyncRead + AsyncWrite + Unpin,
{
    /// Receives and decrypts the next MHF transport payload.
    ///
    /// Returns `None` after a clean end of stream.
    pub async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        self.framed.next().await.transpose()
    }

    /// Encrypts, queues, and flushes one MHF transport payload.
    pub async fn send(&mut self, payload: Bytes) -> Result<(), TransportError> {
        self.framed.send(payload).await
    }

    /// Flushes pending output and closes the underlying byte stream.
    pub async fn close(mut self) -> Result<(), TransportError> {
        self.framed.close().await
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
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

        assert_eq!(
            server.recv().await.unwrap(),
            Some(Bytes::from_static(b"first"))
        );
        assert_eq!(
            server.recv().await.unwrap(),
            Some(Bytes::from_static(b"second"))
        );

        server.send(Bytes::from_static(b"response")).await.unwrap();
        assert_eq!(
            client.recv().await.unwrap(),
            Some(Bytes::from_static(b"response"))
        );

        client.close().await.unwrap();
        assert_eq!(server.recv().await.unwrap(), None);
    }
}
