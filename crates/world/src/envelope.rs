use std::io::Cursor;

use binrw::{BinRead, BinWrite, Endian, binwrite};
use bytes::Bytes;
use shrimpman_protocol::{BinrwOutboundSender, CommandDecoder, DecodedCommand, OutboundSendError};

pub(crate) const MSG_SYS_END: u16 = 0x0010;

/// Decodes the big-endian opcode prefix of a Land packet.
pub(crate) struct LandCommandDecoder;

impl CommandDecoder for LandCommandDecoder {
    type Command = u16;
    type Metadata = ();
    type Error = binrw::Error;

    fn decode_command(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<DecodedCommand<Self::Command, Self::Metadata>, Self::Error> {
        let opcode = u16::read_options(payload, Endian::Big, ())?;

        Ok(DecodedCommand::new(opcode, ()))
    }
}

/// Associates a typed Land packet body with its wire opcode.
#[allow(dead_code, reason = "used by Land packet types")]
pub(crate) trait LandPacket: Send + 'static {
    const OPCODE: u16;
}

#[binwrite]
#[brw(big)]
#[allow(dead_code, reason = "constructed when a Land handler sends a response")]
struct LandOutbound<Packet>
where
    Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>,
{
    #[bw(calc = Packet::OPCODE)]
    opcode: u16,
    packet: Packet,
    #[bw(calc = MSG_SYS_END)]
    end: u16,
}

/// Adds the Land command envelope before delegating to the shared binrw sender.
#[allow(dead_code, reason = "used by Land packet handlers")]
pub(crate) trait LandOutboundSender {
    async fn send_packet<Packet>(&self, packet: Packet) -> Result<(), OutboundSendError>
    where
        Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>;

    async fn send_packet_and_flush<Packet>(&self, packet: Packet) -> Result<(), OutboundSendError>
    where
        Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>;
}

impl LandOutboundSender for BinrwOutboundSender {
    async fn send_packet<Packet>(&self, packet: Packet) -> Result<(), OutboundSendError>
    where
        Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>,
    {
        self.send(LandOutbound { packet }).await
    }

    async fn send_packet_and_flush<Packet>(&self, packet: Packet) -> Result<(), OutboundSendError>
    where
        Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>,
    {
        self.send_and_flush(LandOutbound { packet }).await
    }
}

#[cfg(test)]
mod tests {
    use shrimpman_protocol::CommandDecoder;

    use super::*;

    #[test]
    fn separates_a_big_endian_opcode_from_its_body() {
        let mut payload = Cursor::new(Bytes::from_static(&[0x12, 0x34, 0x07]));

        let (opcode, ()) = LandCommandDecoder
            .decode_command(&mut payload)
            .unwrap()
            .into_parts();

        assert_eq!(opcode, 0x1234);
        assert_eq!(payload.position(), 2);
    }
}
