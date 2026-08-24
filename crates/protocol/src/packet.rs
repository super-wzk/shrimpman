use std::{
    io::Cursor,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_util::{Sink, SinkExt, Stream};
use thiserror::Error;

/// The result of decoding at most one service item from a transport payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadDecode<T> {
    Item(T),
    Complete,
}

/// Converts transport payloads into service items.
pub trait PayloadDecoder {
    type Inbound;
    type Error;

    /// Decodes at most one item and advances `payload` past its bytes.
    fn decode_next(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<PayloadDecode<Self::Inbound>, Self::Error>;
}

/// Converts an outbound service value into a transport payload.
pub trait EncodePayload {
    type Error;

    fn encode(self) -> Result<Bytes, Self::Error>;
}

/// An error from the underlying transport or payload processing.
#[derive(Debug, Error)]
pub enum PacketError<TransportError, PayloadError> {
    #[error("transport error: {0}")]
    Transport(#[source] TransportError),

    #[error("payload error: {0}")]
    Payload(#[source] PayloadError),

    #[error("payload decoder produced an item without consuming bytes")]
    DecoderDidNotAdvance,

    #[error("payload decoder moved to byte {position} past payload length {payload_len}")]
    CursorOutOfBounds { position: u64, payload_len: u64 },

    #[error("payload decoder left {remaining} trailing bytes")]
    TrailingPayload { remaining: u64 },
}

/// A typed stream and sink layered over a payload-oriented transport.
pub struct PacketStream<Transport, Decoder> {
    transport: Transport,
    decoder: Decoder,
    current_payload: Option<Cursor<Bytes>>,
    terminated: bool,
}

impl<Transport, Decoder> PacketStream<Transport, Decoder> {
    pub fn new(transport: Transport, decoder: Decoder) -> Self {
        Self {
            transport,
            decoder,
            current_payload: None,
            terminated: false,
        }
    }

    /// Flushes and closes the underlying payload transport.
    pub async fn close(&mut self) -> Result<(), <Transport as Sink<Bytes>>::Error>
    where
        Transport: Sink<Bytes> + Unpin,
    {
        SinkExt::<Bytes>::close(&mut self.transport).await
    }
}

impl<Transport, Decoder, TransportError> Stream for PacketStream<Transport, Decoder>
where
    Transport: Stream<Item = Result<Bytes, TransportError>> + Unpin,
    Decoder: PayloadDecoder + Unpin,
{
    type Item = Result<Decoder::Inbound, PacketError<TransportError, Decoder::Error>>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.terminated {
            return Poll::Ready(None);
        }

        loop {
            if let Some(payload) = &mut this.current_payload {
                let payload_len = payload.get_ref().len() as u64;
                let before = payload.position();

                if before == payload_len {
                    this.current_payload = None;
                    continue;
                }

                let decoded = match this.decoder.decode_next(payload) {
                    Ok(decoded) => decoded,
                    Err(error) => {
                        this.terminated = true;
                        return Poll::Ready(Some(Err(PacketError::Payload(error))));
                    }
                };
                let after = payload.position();

                if after > payload_len {
                    this.terminated = true;
                    return Poll::Ready(Some(Err(PacketError::CursorOutOfBounds {
                        position: after,
                        payload_len,
                    })));
                }

                match decoded {
                    PayloadDecode::Item(item) if after > before => {
                        return Poll::Ready(Some(Ok(item)));
                    }
                    PayloadDecode::Item(_) => {
                        this.terminated = true;
                        return Poll::Ready(Some(Err(PacketError::DecoderDidNotAdvance)));
                    }
                    PayloadDecode::Complete if after == payload_len => {
                        this.current_payload = None;
                    }
                    PayloadDecode::Complete => {
                        this.terminated = true;
                        return Poll::Ready(Some(Err(PacketError::TrailingPayload {
                            remaining: payload_len - after,
                        })));
                    }
                }

                continue;
            }

            match Pin::new(&mut this.transport).poll_next(context) {
                Poll::Ready(Some(Ok(payload))) if payload.is_empty() => continue,
                Poll::Ready(Some(Ok(payload))) => {
                    this.current_payload = Some(Cursor::new(payload));
                }
                Poll::Ready(Some(Err(error))) => {
                    this.terminated = true;
                    return Poll::Ready(Some(Err(PacketError::Transport(error))));
                }
                Poll::Ready(None) => {
                    this.terminated = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<Transport, Decoder, Outbound, TransportError> Sink<Outbound>
    for PacketStream<Transport, Decoder>
where
    Transport: Sink<Bytes, Error = TransportError> + Unpin,
    Decoder: Unpin,
    Outbound: EncodePayload,
{
    type Error = PacketError<TransportError, Outbound::Error>;

    fn poll_ready(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.transport)
            .poll_ready(context)
            .map_err(PacketError::Transport)
    }

    fn start_send(self: Pin<&mut Self>, outbound: Outbound) -> Result<(), Self::Error> {
        let this = self.get_mut();
        let payload = outbound.encode().map_err(PacketError::Payload)?;
        Pin::new(&mut this.transport)
            .start_send(payload)
            .map_err(PacketError::Transport)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.transport)
            .poll_flush(context)
            .map_err(PacketError::Transport)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.transport)
            .poll_close(context)
            .map_err(PacketError::Transport)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, convert::Infallible, io::Read, rc::Rc};

    use futures_util::{SinkExt, StreamExt, sink, stream};

    use super::*;

    struct ByteDecoder {
        decode_count: Rc<Cell<usize>>,
    }

    impl PayloadDecoder for ByteDecoder {
        type Inbound = u8;
        type Error = std::io::Error;

        fn decode_next(
            &mut self,
            payload: &mut Cursor<Bytes>,
        ) -> Result<PayloadDecode<Self::Inbound>, Self::Error> {
            let mut byte = [0];
            payload.read_exact(&mut byte)?;
            self.decode_count.set(self.decode_count.get() + 1);
            Ok(PayloadDecode::Item(byte[0]))
        }
    }

    struct Byte(u8);

    impl EncodePayload for Byte {
        type Error = Infallible;

        fn encode(self) -> Result<Bytes, Self::Error> {
            Ok(Bytes::copy_from_slice(&[self.0]))
        }
    }

    #[tokio::test]
    async fn decodes_only_one_item_per_poll() {
        let decode_count = Rc::new(Cell::new(0));
        let transport = stream::iter([Ok::<_, Infallible>(Bytes::from_static(&[1, 2]))]);
        let decoder = ByteDecoder {
            decode_count: Rc::clone(&decode_count),
        };
        let mut packets = PacketStream::new(transport, decoder);

        assert_eq!(packets.next().await.unwrap().unwrap(), 1);
        assert_eq!(decode_count.get(), 1);

        assert_eq!(packets.next().await.unwrap().unwrap(), 2);
        assert_eq!(decode_count.get(), 2);
        assert!(packets.next().await.is_none());
    }

    #[tokio::test]
    async fn encodes_items_into_payload_sink() {
        let decoder = ByteDecoder {
            decode_count: Rc::new(Cell::new(0)),
        };
        let mut packets = PacketStream::new(sink::drain(), decoder);

        packets.send(Byte(7)).await.unwrap();
    }

    #[tokio::test]
    async fn closes_the_transport_without_an_outbound_packet_type() {
        let transport = sink::drain::<Bytes>();
        let decoder = ByteDecoder {
            decode_count: Rc::new(Cell::new(0)),
        };
        let mut packets = PacketStream::new(transport, decoder);

        packets.close().await.unwrap();
    }

    struct StalledDecoder;

    impl PayloadDecoder for StalledDecoder {
        type Inbound = ();
        type Error = Infallible;

        fn decode_next(
            &mut self,
            _payload: &mut Cursor<Bytes>,
        ) -> Result<PayloadDecode<Self::Inbound>, Self::Error> {
            Ok(PayloadDecode::Item(()))
        }
    }

    #[tokio::test]
    async fn rejects_decoders_that_do_not_advance() {
        let transport = stream::iter([Ok::<_, Infallible>(Bytes::from_static(&[1]))]);
        let mut packets = PacketStream::new(transport, StalledDecoder);

        assert!(matches!(
            packets.next().await,
            Some(Err(PacketError::DecoderDidNotAdvance))
        ));
        assert!(packets.next().await.is_none());
    }
}
