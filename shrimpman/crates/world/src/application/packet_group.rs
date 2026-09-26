use std::io::Cursor;

use bytes::Bytes;
use shrimpman_protocol::{CommandPacketDecoder, PayloadDecode, PayloadDecoder};

use super::PacketDecodeError;
use crate::{
    envelope::{LandCommandDecoder, MSG_SYS_END},
    router::LandRouter,
};

type RoutedPacketDecoder = CommandPacketDecoder<LandCommandDecoder, LandRouter>;

/// Routes each packet in a Land group and validates its final MSG_SYS_END.
pub(super) struct LandPacketGroupDecoder {
    routed: RoutedPacketDecoder,
}

impl LandPacketGroupDecoder {
    pub(super) fn new(router: LandRouter) -> Self {
        Self {
            routed: CommandPacketDecoder::new(LandCommandDecoder, router),
        }
    }
}

impl PayloadDecoder for LandPacketGroupDecoder {
    type Inbound = <RoutedPacketDecoder as PayloadDecoder>::Inbound;
    type Error = PacketDecodeError;

    fn decode_next(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<PayloadDecode<Self::Inbound>, Self::Error> {
        let payload_len = payload.get_ref().len();
        let Some(packet_end) = payload_len.checked_sub(size_of::<u16>()) else {
            return Err(PacketDecodeError::MissingEndMarker);
        };

        // 先检查整个分组的结束标记，再允许业务处理器收到其中的首包。
        let end = &payload.get_ref()[packet_end..];
        let actual = u16::from_be_bytes([end[0], end[1]]);
        if actual != MSG_SYS_END {
            return Err(PacketDecodeError::InvalidEndMarker { actual });
        }

        let decoded = match self
            .routed
            .decode_next(payload)
            .map_err(PacketDecodeError::Packet)?
        {
            PayloadDecode::Item(decoded) => decoded,
            PayloadDecode::Complete => return Err(PacketDecodeError::MissingEndMarker),
        };

        if *decoded.command() == MSG_SYS_END {
            let remaining = payload_len as u64 - payload.position();
            if remaining != 0 {
                return Err(PacketDecodeError::EndMarkerNotFinal { remaining });
            }
            return Ok(PayloadDecode::Complete);
        }

        Ok(PayloadDecode::Item(decoded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_packets_after_the_end_marker() {
        let mut decoder = LandPacketGroupDecoder::new(LandRouter::new().unwrap());
        let mut payload = Cursor::new(Bytes::from_static(&[
            0x00, 0x11, 0x00, 0x10, 0x00, 0x11, 0x00, 0x10,
        ]));

        assert!(matches!(
            decoder.decode_next(&mut payload).unwrap(),
            PayloadDecode::Item(_)
        ));
        assert!(matches!(
            decoder.decode_next(&mut payload),
            Err(PacketDecodeError::EndMarkerNotFinal { remaining: 4 })
        ));
    }
}
