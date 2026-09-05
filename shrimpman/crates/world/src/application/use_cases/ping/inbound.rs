use binrw::BinRead;

use crate::{envelope::LandPacket, response::RequestHandle};

const MSG_SYS_PING: u16 = 0x0017;

/// Checks whether a Land connection is still responsive.
#[derive(BinRead)]
pub(super) struct PingRequest {
    pub(super) request_handle: RequestHandle,
}

impl LandPacket for PingRequest {
    const OPCODE: u16 = MSG_SYS_PING;
}
