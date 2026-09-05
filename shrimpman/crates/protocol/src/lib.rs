//! Service-independent protocol streaming and handler abstractions.

#![warn(unreachable_pub)]

mod binrw;
mod codec;
mod handler;
mod outbound;
mod packet;
mod router;

pub use binrw::{BinrwHandlerDecoder, BinrwOutbound, BinrwOutboundSender};
pub use codec::{
    CommandDecoder, CommandPacketDecodeError, CommandPacketDecoder, DecodedCommand, DecodedPacket,
    ErasedPacket, PacketDecoder,
};
pub use handler::{DispatchError, DispatchMode, Dispatcher, ErasedHandler, Handler};
pub use outbound::{OutboundReceiver, OutboundSendError, OutboundSender, outbound_channel};
pub use packet::{EncodePayload, PacketError, PacketStream, PayloadDecode, PayloadDecoder};
pub use router::{RouteResolver, RouteSelector, RouteTable, RouteTableBuildError};
