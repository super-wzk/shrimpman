use std::io::Cursor;

use binrw::{BinRead, BinWrite, Endian, binwrite};
use bytes::Bytes;
use shrimpman_protocol::{CommandDecoder, DecodedCommand};

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
pub(crate) trait LandPacket: Send + 'static {
    const OPCODE: u16;
}

#[binwrite]
#[brw(big)]
pub(crate) struct LandOutbound<Packet>
where
    Packet: LandPacket + for<'args> BinWrite<Args<'args> = ()>,
{
    #[bw(calc = Packet::OPCODE)]
    opcode: u16,
    pub(crate) packet: Packet,
    #[bw(calc = MSG_SYS_END)]
    end: u16,
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
