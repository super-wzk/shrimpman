use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use binrw::BinWrite;
use shrimpman_protocol::{BinrwOutboundSender, OutboundSendError};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{
    application::InternalError,
    envelope::{LandOutbound, LandPacket},
    response::{ExpectedResponse, RequestEnvelope, RequestHandle, ResponseError, WireResponse},
};

const MAX_REQUEST_SLOTS: usize = 0x200;

/// Request correlation state shared by one connection's receive and handler tasks.
#[derive(Default)]
pub(super) struct ExchangeState {
    slots: Mutex<Vec<RequestSlot>>,
}

impl ExchangeState {
    #[cfg_attr(not(test), allow(dead_code, reason = "server request API"))]
    fn reserve(&self) -> Result<(RequestHandle, oneshot::Receiver<WireResponse>), InternalError> {
        let mut slots = self.slots.lock().expect("World request slots poisoned");
        let index = match slots
            .iter()
            .position(|slot| slot.sender.as_ref().is_none_or(oneshot::Sender::is_closed))
        {
            Some(index) => index,
            None if slots.len() < MAX_REQUEST_SLOTS => {
                slots.push(RequestSlot::default());
                slots.len() - 1
            }
            None => return Err(InternalError::RequestSlotsExhausted),
        };
        let slot = &mut slots[index];
        slot.generation = slot.generation.wrapping_add(1).max(1);
        let handle = RequestHandle::from_slot(index as u16, slot.generation);
        let (sender, receiver) = oneshot::channel();
        slot.sender = Some(sender);

        Ok((handle, receiver))
    }

    pub(super) fn complete(&self, response: WireResponse) -> bool {
        let handle = response.handle();
        let sender = {
            let mut slots = self.slots.lock().expect("World request slots poisoned");
            let Some(slot) = slots.get_mut(handle.slot_index()) else {
                return false;
            };
            if slot.generation != handle.generation() {
                return false;
            }
            slot.sender.take()
        };

        sender.is_some_and(|sender| sender.send(response).is_ok())
    }
}

#[derive(Default)]
struct RequestSlot {
    generation: u16,
    sender: Option<oneshot::Sender<WireResponse>>,
}

/// Connection-scoped Land messaging and correlated request exchange.
pub(crate) struct LandExchange {
    #[cfg_attr(not(test), allow(dead_code, reason = "server request API"))]
    state: Arc<ExchangeState>,
    #[cfg_attr(not(test), allow(dead_code, reason = "server request API"))]
    cancellation: CancellationToken,
    sender: BinrwOutboundSender,
}

impl LandExchange {
    pub(super) fn new(
        sender: BinrwOutboundSender,
        state: Arc<ExchangeState>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            state,
            cancellation,
            sender,
        }
    }

    #[cfg_attr(not(test), allow(dead_code, reason = "queued packet API"))]
    pub(super) async fn send_packet<Packet>(&self, packet: Packet) -> Result<(), OutboundSendError>
    where
        Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>,
    {
        self.sender.send(LandOutbound { packet }).await
    }

    pub(super) async fn send_packet_and_flush<Packet>(
        &self,
        packet: Packet,
    ) -> Result<(), OutboundSendError>
    where
        Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>,
    {
        self.sender.send_and_flush(LandOutbound { packet }).await
    }

    /// Sends a command and waits up to five seconds for its matching MSG_SYS_ACK response.
    ///
    /// Protocol failures are returned to the caller in the inner result. The outer result is
    /// reserved for failures in the exchange itself.
    #[cfg_attr(not(test), allow(dead_code, reason = "server request API"))]
    pub(super) async fn request<Want>(
        &self,
        payload: impl LandPacket + for<'args> BinWrite<Args<'args> = ()>,
    ) -> Result<Result<Want::Output, ResponseError>, InternalError>
    where
        Want: ExpectedResponse,
    {
        self.request_with_timeout::<Want>(payload, Duration::from_secs(5))
            .await
    }

    /// Sends a command and waits for its matching MSG_SYS_ACK response.
    ///
    /// Protocol failures are returned to the caller in the inner result. The outer result is
    /// reserved for failures in the exchange itself.
    #[cfg_attr(not(test), allow(dead_code, reason = "server request API"))]
    pub(super) async fn request_with_timeout<Want>(
        &self,
        payload: impl LandPacket + for<'args> BinWrite<Args<'args> = ()>,
        timeout: Duration,
    ) -> Result<Result<Want::Output, ResponseError>, InternalError>
    where
        Want: ExpectedResponse,
    {
        let (handle, response) = self.state.reserve()?;
        let request = RequestEnvelope { handle, payload };
        let exchange = async {
            self.send_packet_and_flush(request).await?;
            response.await.map_err(|_| InternalError::ConnectionClosed)
        };

        let response = tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => return Err(InternalError::ConnectionClosed),
            response = tokio::time::timeout(timeout, exchange) => {
                let response = response.map_err(|_| InternalError::RequestTimedOut)?;
                response?
            }
        };
        let (_, error, data) = response.into_parts();
        if !error.is_success() {
            return Ok(Err(error));
        }

        Want::decode(data)
            .map(Ok)
            .map_err(InternalError::ResponseDecode)
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroUsize, sync::Arc};

    use binrw::BinWrite;
    use futures_util::sink;
    use shrimpman_protocol::{BinrwOutbound, outbound_channel};

    use super::*;
    use crate::response::{Buffered, CommandResponse, ResponseBody};

    #[test]
    fn limits_concurrent_requests() {
        let state = ExchangeState::default();
        let _receivers = (0..MAX_REQUEST_SLOTS)
            .map(|_| state.reserve().unwrap().1)
            .collect::<Vec<_>>();

        assert!(matches!(
            state.reserve(),
            Err(InternalError::RequestSlotsExhausted)
        ));
    }

    #[test]
    fn rejects_a_stale_response_after_reusing_a_slot() {
        let state = ExchangeState::default();
        let (stale_handle, stale_response) = state.reserve().unwrap();
        drop(stale_response);
        let (current_handle, mut current_response) = state.reserve().unwrap();

        assert_eq!(stale_handle.slot_index(), current_handle.slot_index());
        assert_ne!(stale_handle.generation(), current_handle.generation());
        assert!(!state.complete(CommandResponse::success(
            stale_handle,
            ResponseBody::Value(1),
        )));
        assert!(state.complete(CommandResponse::success(
            current_handle,
            ResponseBody::Value(2),
        )));

        let response = current_response.try_recv().unwrap();
        let (_, _, ResponseBody::Value(value)) = response.into_parts() else {
            panic!("slot received an unexpected buffered response");
        };
        assert_eq!(value, 2);
    }

    #[derive(BinWrite)]
    struct TestRequest(u8);

    impl LandPacket for TestRequest {
        const OPCODE: u16 = 0x1234;
    }

    #[tokio::test]
    async fn times_out_and_releases_the_request_slot() {
        let (sender, receiver) = outbound_channel::<BinrwOutbound>(NonZeroUsize::MIN);
        let state = Arc::new(ExchangeState::default());
        let exchange = LandExchange::new(
            BinrwOutboundSender::big_endian(sender),
            Arc::clone(&state),
            CancellationToken::new(),
        );
        let writer = tokio::spawn(async move {
            receiver.forward_to(&mut sink::drain()).await.unwrap();
        });

        let result = exchange
            .request_with_timeout::<Buffered<u16>>(TestRequest(7), Duration::from_millis(10))
            .await;

        assert!(matches!(result, Err(InternalError::RequestTimedOut)));
        let (handle, _) = state.reserve().unwrap();
        assert_eq!(handle.slot_index(), 0);
        drop(exchange);
        writer.await.unwrap();
    }
}
