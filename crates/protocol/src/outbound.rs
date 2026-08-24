use std::num::NonZeroUsize;

use futures_util::{Sink, SinkExt};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

/// Creates a bounded channel between handlers and a connection's writer.
pub fn outbound_channel<Outbound>(
    capacity: NonZeroUsize,
) -> (OutboundSender<Outbound>, OutboundReceiver<Outbound>) {
    let (sender, receiver) = mpsc::channel(capacity.get());
    (OutboundSender { sender }, OutboundReceiver { receiver })
}

/// Sends outbound values to the writer that owns the connection.
pub struct OutboundSender<Outbound> {
    sender: mpsc::Sender<PendingOutbound<Outbound>>,
}

impl<Outbound> Clone for OutboundSender<Outbound> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<Outbound> OutboundSender<Outbound> {
    /// Waits for queue capacity and returns after the value has been enqueued.
    pub async fn send(&self, outbound: Outbound) -> Result<(), OutboundSendError> {
        self.enqueue(PendingOutbound {
            outbound,
            flush: None,
        })
        .await
    }

    /// Waits until the writer has flushed this value to the transport.
    pub async fn send_and_flush(&self, outbound: Outbound) -> Result<(), OutboundSendError> {
        let (flush, completed) = oneshot::channel();
        self.enqueue(PendingOutbound {
            outbound,
            flush: Some(flush),
        })
        .await?;
        completed.await.map_err(|_| OutboundSendError)
    }

    async fn enqueue(&self, outbound: PendingOutbound<Outbound>) -> Result<(), OutboundSendError> {
        self.sender
            .send(outbound)
            .await
            .map_err(|_| OutboundSendError)
    }
}

/// Receives outbound values for the writer that owns the connection.
pub struct OutboundReceiver<Outbound> {
    receiver: mpsc::Receiver<PendingOutbound<Outbound>>,
}

impl<Outbound> OutboundReceiver<Outbound> {
    /// Forwards and flushes every queued value until all senders close.
    pub async fn forward_to<SinkType>(mut self, sink: &mut SinkType) -> Result<(), SinkType::Error>
    where
        SinkType: Sink<Outbound> + Unpin,
    {
        while let Some(PendingOutbound { outbound, flush }) = self.receiver.recv().await {
            sink.send(outbound).await?;

            if let Some(flush) = flush {
                let _ = flush.send(());
            }
        }

        Ok(())
    }
}

struct PendingOutbound<Outbound> {
    outbound: Outbound,
    flush: Option<oneshot::Sender<()>>,
}

/// The connection writer is no longer available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("outbound channel is closed")]
pub struct OutboundSendError;

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    use super::*;

    #[derive(Default)]
    struct LifecycleSink {
        flushes: usize,
        closes: usize,
    }

    impl Sink<u8> for LifecycleSink {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, _value: u8) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.get_mut().flushes += 1;
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.get_mut().closes += 1;
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn queued_send_returns_before_delivery() {
        let (sender, mut receiver) = outbound_channel(NonZeroUsize::MIN);

        sender.send(7).await.unwrap();
        let pending = receiver.receiver.recv().await.unwrap();

        assert_eq!(pending.outbound, 7);
        assert!(pending.flush.is_none());
    }

    #[tokio::test]
    async fn flushed_send_waits_for_writer_completion() {
        let (sender, mut receiver) = outbound_channel(NonZeroUsize::MIN);
        let send = tokio::spawn(async move { sender.send_and_flush(7).await });
        let pending = receiver.receiver.recv().await.unwrap();

        assert_eq!(pending.outbound, 7);
        assert!(!send.is_finished());

        pending.flush.unwrap().send(()).unwrap();
        send.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn forwarding_flushes_values_without_closing_the_sink() {
        let (sender, receiver) = outbound_channel(NonZeroUsize::MIN);
        sender.send(7).await.unwrap();
        drop(sender);
        let mut sink = LifecycleSink::default();

        receiver.forward_to(&mut sink).await.unwrap();

        assert_eq!(sink.flushes, 1);
        assert_eq!(sink.closes, 0);
    }
}
