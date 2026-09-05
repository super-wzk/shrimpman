use std::{io::Cursor, sync::OnceLock};

use binrw::BinRead;
use bytes::Bytes;
use shrimpman_protocol::{PacketDecoder, RouteResolver, RouteTable, RouteTableBuildError};
use thiserror::Error;

use registration::{LandErasedHandler, LandHandlerDecoder};

use crate::{
    envelope::MSG_SYS_END,
    response::{MSG_SYS_ACK, WireResponse},
};

mod registration;

pub(crate) use registration::LandRouteRegistration;

type Routes = RouteTable<u16, (), LandPacketDecoder>;

const MSG_SYS_NOP: u16 = 0x0011;

pub(crate) enum LandInbound {
    Handler(LandErasedHandler),
    Response(WireResponse),
    Control,
}

#[derive(Clone, Copy)]
pub(crate) enum LandPacketDecoder {
    Handler(LandHandlerDecoder),
    Response,
    Control,
}

impl PacketDecoder<()> for LandPacketDecoder {
    type Output = LandInbound;
    type Error = binrw::Error;

    fn decode(
        &self,
        metadata: &(),
        payload: &mut Cursor<Bytes>,
    ) -> Result<Self::Output, Self::Error> {
        match self {
            Self::Handler(decoder) => decoder.decode(metadata, payload).map(LandInbound::Handler),
            Self::Response => WireResponse::read_be(payload).map(LandInbound::Response),
            Self::Control => Ok(LandInbound::Control),
        }
    }
}

/// An invalid distributed Land route table.
#[derive(Debug, Error)]
pub enum LandRouterBuildError {
    #[error("Land opcode {opcode:#06x} is registered more than once")]
    AmbiguousOpcode { opcode: u16 },
}

/// An error resolving a registered Land route.
#[derive(Debug, Error)]
pub enum LandRouteError {
    #[error("unsupported Land opcode {opcode:#06x}")]
    Unsupported { opcode: u16 },
}

/// Resolves Land opcodes to their registered packet decoders.
#[derive(Clone, Copy)]
pub(crate) struct LandRouter {
    routes: &'static Routes,
}

impl LandRouter {
    pub(crate) fn new() -> Result<Self, LandRouterBuildError> {
        static ROUTES: OnceLock<Routes> = OnceLock::new();

        let routes = if let Some(routes) = ROUTES.get() {
            routes
        } else {
            let routes = build_routes(inventory::iter::<LandRouteRegistration>)?;
            ROUTES.get_or_init(|| routes)
        };
        Ok(Self { routes })
    }
}

impl RouteResolver<u16, ()> for LandRouter {
    type Target = LandPacketDecoder;
    type Error = LandRouteError;

    fn resolve(&self, opcode: &u16, (): &()) -> Result<Self::Target, Self::Error> {
        self.routes
            .resolve(opcode, &())
            .copied()
            .ok_or(LandRouteError::Unsupported { opcode: *opcode })
    }
}

fn build_routes(
    registrations: impl IntoIterator<Item = &'static LandRouteRegistration>,
) -> Result<Routes, LandRouterBuildError> {
    let entries = registrations.into_iter().map(|registration| {
        (
            registration.opcode,
            (),
            LandPacketDecoder::Handler(registration.decoder),
        )
    });
    let built_in_entries = [
        (MSG_SYS_END, (), LandPacketDecoder::Control),
        (MSG_SYS_NOP, (), LandPacketDecoder::Control),
        (MSG_SYS_ACK, (), LandPacketDecoder::Response),
    ];
    let entries = built_in_entries.into_iter().chain(entries);

    RouteTable::build(entries).map_err(|error| match error {
        RouteTableBuildError::EmptySelector { .. } => {
            unreachable!("the unit route selector always matches")
        }
        RouteTableBuildError::Conflict { key: opcode, .. } => {
            LandRouterBuildError::AmbiguousOpcode { opcode }
        }
    })
}

#[cfg(test)]
mod tests {
    use binrw::BinRead;
    use shrimpman_protocol::Handler;

    use super::*;
    use crate::{
        application::{InternalError, WorldSessionContext},
        envelope::LandPacket,
        exchange::LandExchange,
    };

    #[derive(BinRead)]
    struct Request;

    impl LandPacket for Request {
        const OPCODE: u16 = 0x1234;
    }

    struct HandlerImpl;

    impl Handler<WorldSessionContext, LandExchange> for HandlerImpl {
        type Inbound = Request;
        type Error = InternalError;

        async fn handle(
            &self,
            _context: WorldSessionContext,
            _inbound: Self::Inbound,
            _exchange: LandExchange,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    static REGISTRATION: LandRouteRegistration = LandRouteRegistration::new(&HandlerImpl);

    #[test]
    fn rejects_duplicate_opcode_registrations() {
        let error = match build_routes([&REGISTRATION, &REGISTRATION]) {
            Ok(_) => panic!("duplicate Land opcodes were accepted"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            LandRouterBuildError::AmbiguousOpcode { opcode: 0x1234 }
        ));
    }
}
