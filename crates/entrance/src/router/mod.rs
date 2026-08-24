use std::sync::OnceLock;

use shrimpman_protocol::{RouteResolver, RouteTable, RouteTableBuildError};
use thiserror::Error;

use crate::Command;

use registration::EntranceHandlerDecoder;

mod registration;

pub(crate) use registration::EntrancePacketRegistration;

type Routes = RouteTable<&'static str, (), EntranceHandlerDecoder>;

/// An invalid distributed Entrance route table.
#[derive(Debug, Error)]
pub enum EntranceRouterBuildError {
    #[error("Entrance command {command} is registered more than once")]
    AmbiguousCommand { command: &'static str },
}

/// An error resolving a registered Entrance route.
#[derive(Debug, Error)]
pub enum EntranceRouteError {
    #[error("unsupported Entrance route for command {}", .command.as_str())]
    Unsupported { command: Command },
}

/// Resolves Entrance commands to their registered packet decoders.
#[derive(Clone, Copy)]
pub(crate) struct EntranceRouter {
    routes: &'static Routes,
}

impl EntranceRouter {
    /// Builds the shared route table from distributed packet registrations.
    pub(crate) fn new() -> Result<Self, EntranceRouterBuildError> {
        static ROUTES: OnceLock<Routes> = OnceLock::new();

        let routes = if let Some(routes) = ROUTES.get() {
            routes
        } else {
            let routes = build_routes(inventory::iter::<EntrancePacketRegistration>)?;
            ROUTES.get_or_init(|| routes)
        };
        Ok(Self { routes })
    }
}

impl RouteResolver<Command, ()> for EntranceRouter {
    type Target = EntranceHandlerDecoder;
    type Error = EntranceRouteError;

    fn resolve(&self, command: &Command, (): &()) -> Result<Self::Target, Self::Error> {
        self.routes
            .resolve(command.as_str(), &())
            .copied()
            .ok_or_else(|| EntranceRouteError::Unsupported {
                command: command.clone(),
            })
    }
}

fn build_routes(
    registrations: impl IntoIterator<Item = &'static EntrancePacketRegistration>,
) -> Result<Routes, EntranceRouterBuildError> {
    let entries = registrations.into_iter().flat_map(|registration| {
        registration
            .commands
            .iter()
            .copied()
            .map(move |command| (command, (), registration.decoder))
    });

    RouteTable::build(entries).map_err(|error| match error {
        RouteTableBuildError::EmptySelector { .. } => {
            unreachable!("the unit route selector always matches")
        }
        RouteTableBuildError::Conflict { key: command, .. } => {
            EntranceRouterBuildError::AmbiguousCommand { command }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::BinRead;
    use bytes::Bytes;
    use shrimpman_protocol::{
        BinrwOutboundSender, CommandDecoder, DispatchMode, Handler, PacketDecoder, RouteResolver,
    };

    use super::*;
    use crate::{EntranceSessionContext, InternalError, envelope::EntranceCommandDecoder};

    #[derive(BinRead)]
    struct Request(u8);

    struct FirstHandler;
    static FIRST_HANDLER: FirstHandler = FirstHandler;

    impl Handler<EntranceSessionContext, BinrwOutboundSender> for FirstHandler {
        type Inbound = Request;
        type Error = InternalError;

        async fn handle(
            &self,
            _context: EntranceSessionContext,
            inbound: Self::Inbound,
            _outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            let Request(_value) = inbound;
            Ok(())
        }
    }

    static FIRST: EntrancePacketRegistration =
        EntrancePacketRegistration::new(&["ALL+"], &FIRST_HANDLER);

    inventory::submit! {
        EntrancePacketRegistration::new(
            &["TEST-COMMAND"],
            &FIRST_HANDLER,
        )
    }

    #[test]
    fn resolves_null_terminated_commands_from_distributed_registrations() {
        let mut payload = Cursor::new(Bytes::from_static(b"TEST-COMMAND\0\x07"));
        let (command, metadata) = EntranceCommandDecoder
            .decode_command(&mut payload)
            .unwrap()
            .into_parts();
        let handler = EntranceRouter::new()
            .unwrap()
            .resolve(&command, &metadata)
            .unwrap()
            .decode(&metadata, &mut payload)
            .unwrap();

        assert_eq!(command.as_str(), "TEST-COMMAND");
        assert_eq!(handler.mode(), DispatchMode::Ordered);
        assert_eq!(payload.position(), 14);
    }

    #[test]
    fn reports_unsupported_commands() {
        let mut payload = Cursor::new(Bytes::from_static(b"UNKNOWN\0"));
        let (command, metadata) = EntranceCommandDecoder
            .decode_command(&mut payload)
            .unwrap()
            .into_parts();
        let error = match EntranceRouter::new().unwrap().resolve(&command, &metadata) {
            Ok(_) => panic!("an unsupported Entrance command was resolved"),
            Err(error) => error,
        };

        assert!(matches!(error, EntranceRouteError::Unsupported { .. }));
    }

    #[test]
    fn adapts_registered_handlers_to_packet_decoders() {
        let routes = build_routes([&FIRST]).unwrap();
        let decoder = routes.resolve("ALL+", &()).copied().unwrap();
        let mut payload = Cursor::new(Bytes::from_static(&[7]));
        let handler = decoder.decode(&(), &mut payload).unwrap();

        assert_eq!(handler.mode(), DispatchMode::Ordered);
        assert_eq!(payload.position(), 1);
    }

    #[test]
    fn rejects_duplicate_command_registrations() {
        let error = match build_routes([&FIRST, &FIRST]) {
            Ok(_) => panic!("duplicate Entrance commands were accepted"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            EntranceRouterBuildError::AmbiguousCommand { command: "ALL+" }
        ));
    }
}
