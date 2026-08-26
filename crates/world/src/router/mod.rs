use std::sync::OnceLock;

use shrimpman_protocol::{RouteResolver, RouteTable, RouteTableBuildError};
use thiserror::Error;

use registration::LandHandlerDecoder;

mod registration;

pub(crate) use registration::LandPacketRegistration;

type Routes = RouteTable<u16, (), LandHandlerDecoder>;

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
            let routes = build_routes(inventory::iter::<LandPacketRegistration>)?;
            ROUTES.get_or_init(|| routes)
        };
        Ok(Self { routes })
    }
}

impl RouteResolver<u16, ()> for LandRouter {
    type Target = LandHandlerDecoder;
    type Error = LandRouteError;

    fn resolve(&self, opcode: &u16, (): &()) -> Result<Self::Target, Self::Error> {
        self.routes
            .resolve(opcode, &())
            .copied()
            .ok_or(LandRouteError::Unsupported { opcode: *opcode })
    }
}

fn build_routes(
    registrations: impl IntoIterator<Item = &'static LandPacketRegistration>,
) -> Result<Routes, LandRouterBuildError> {
    let entries = registrations
        .into_iter()
        .map(|registration| (registration.opcode, (), registration.decoder));

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
    use shrimpman_domain::world::LandKey;
    use shrimpman_protocol::{BinrwOutboundSender, Handler};

    use super::*;
    use crate::{application::InternalError, envelope::LandPacket};

    #[derive(BinRead)]
    struct Request;

    impl LandPacket for Request {
        const OPCODE: u16 = 0x1234;
    }

    struct HandlerImpl;

    impl Handler<LandKey, BinrwOutboundSender> for HandlerImpl {
        type Inbound = Request;
        type Error = InternalError;

        async fn handle(
            &self,
            _land: LandKey,
            _inbound: Self::Inbound,
            _outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    static REGISTRATION: LandPacketRegistration = LandPacketRegistration::new(&HandlerImpl);

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
