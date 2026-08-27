use std::sync::OnceLock;

use shrimpman_protocol::{RouteResolver, RouteTable, RouteTableBuildError};
use thiserror::Error;

use crate::{Command, envelope::ClientVersion};

use registration::SignHandlerDecoder;

mod registration;
mod selector;

pub(crate) use registration::SignRouteRegistration;
pub(crate) use selector::VersionSelector;

type Routes = RouteTable<&'static str, VersionSelector, SignHandlerDecoder>;

/// An invalid distributed Sign route table.
#[derive(Debug, Error)]
pub enum SignRouterBuildError {
    #[error("Sign command {command} has no matching client versions")]
    EmptyVersionSelector { command: &'static str },

    #[error("Sign command {command} has ambiguous registrations at client version {version:03}")]
    AmbiguousVersion { command: &'static str, version: u16 },
}

/// An error resolving a registered Sign route.
#[derive(Debug, Error)]
pub enum SignRouteError {
    #[error(
        "unsupported Sign route for command {} and client version {version:03}",
        .command.as_str()
    )]
    Unsupported { command: Command, version: u16 },
}

/// Resolves Sign commands to their registered packet decoders.
#[derive(Clone, Copy)]
pub(crate) struct SignRouter {
    routes: &'static Routes,
}

impl SignRouter {
    /// Builds the shared route table from distributed packet registrations.
    pub(crate) fn new() -> Result<Self, SignRouterBuildError> {
        static ROUTES: OnceLock<Routes> = OnceLock::new();

        let routes = if let Some(routes) = ROUTES.get() {
            routes
        } else {
            let routes = build_routes(inventory::iter::<SignRouteRegistration>)?;
            ROUTES.get_or_init(|| routes)
        };
        Ok(Self { routes })
    }
}

impl RouteResolver<Command, ClientVersion> for SignRouter {
    type Target = SignHandlerDecoder;
    type Error = SignRouteError;

    fn resolve(
        &self,
        command: &Command,
        version: &ClientVersion,
    ) -> Result<Self::Target, Self::Error> {
        self.routes
            .resolve(command.as_str(), version)
            .copied()
            .ok_or_else(|| SignRouteError::Unsupported {
                command: command.clone(),
                version: version.number(),
            })
    }
}

fn build_routes(
    registrations: impl IntoIterator<Item = &'static SignRouteRegistration>,
) -> Result<Routes, SignRouterBuildError> {
    let entries = registrations.into_iter().flat_map(|registration| {
        registration
            .commands
            .iter()
            .copied()
            .map(move |command| (command, registration.versions.clone(), registration.decoder))
    });

    RouteTable::build(entries).map_err(|error| match error {
        RouteTableBuildError::EmptySelector { key: command } => {
            SignRouterBuildError::EmptyVersionSelector { command }
        }
        RouteTableBuildError::Conflict {
            key: command,
            conflict: version,
        } => SignRouterBuildError::AmbiguousVersion {
            command,
            version: version.number(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::BinRead;
    use bytes::Bytes;
    use shrimpman_protocol::{BinrwOutboundSender, DispatchMode, Handler, PacketDecoder};

    use super::*;
    use crate::{InternalError, SignSessionContext};

    const COMMANDS: &[&str] = &["SIGN:"];
    const V041: ClientVersion = ClientVersion::new(41);
    const V100: ClientVersion = ClientVersion::new(100);

    #[derive(BinRead)]
    struct Versioned;

    struct LegacyHandler;
    struct ModernHandler;
    static LEGACY_HANDLER: LegacyHandler = LegacyHandler;
    static MODERN_HANDLER: ModernHandler = ModernHandler;

    impl Handler<SignSessionContext, BinrwOutboundSender> for LegacyHandler {
        type Inbound = Versioned;
        type Error = InternalError;

        async fn handle(
            &self,
            _context: SignSessionContext,
            _inbound: Self::Inbound,
            _outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl Handler<SignSessionContext, BinrwOutboundSender> for ModernHandler {
        type Inbound = Versioned;
        type Error = InternalError;

        const MODE: DispatchMode = DispatchMode::Concurrent;

        async fn handle(
            &self,
            _context: SignSessionContext,
            _inbound: Self::Inbound,
            _outbound: BinrwOutboundSender,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    static LEGACY: SignRouteRegistration =
        SignRouteRegistration::new(COMMANDS, VersionSelector::Before(V100), &LEGACY_HANDLER);
    static MODERN: SignRouteRegistration =
        SignRouteRegistration::new(COMMANDS, VersionSelector::From(V100), &MODERN_HANDLER);
    static FALLBACK: SignRouteRegistration =
        SignRouteRegistration::new(COMMANDS, VersionSelector::Any, &LEGACY_HANDLER);
    static EXACT: SignRouteRegistration =
        SignRouteRegistration::new(COMMANDS, VersionSelector::Exact(V100), &MODERN_HANDLER);

    #[test]
    fn selects_implementations_by_version_range() {
        let routes = build_routes([&LEGACY, &MODERN]).unwrap();

        for version in [V041, ClientVersion::new(99)] {
            assert_eq!(decode_mode(&routes, version), DispatchMode::Ordered);
        }

        for version in [V100, ClientVersion::new(999)] {
            assert_eq!(decode_mode(&routes, version), DispatchMode::Concurrent);
        }
    }

    #[test]
    fn exact_version_overrides_fallback() {
        let routes = build_routes([&FALLBACK, &EXACT]).unwrap();

        assert_eq!(decode_mode(&routes, V041), DispatchMode::Ordered);
        assert_eq!(decode_mode(&routes, V100), DispatchMode::Concurrent);
    }

    #[test]
    fn rejects_overlapping_selectors_with_the_same_precedence() {
        static FROM_041: SignRouteRegistration = SignRouteRegistration::new(
            COMMANDS,
            VersionSelector::From(ClientVersion::new(41)),
            &LEGACY_HANDLER,
        );
        static BEFORE_100: SignRouteRegistration = SignRouteRegistration::new(
            COMMANDS,
            VersionSelector::Before(ClientVersion::new(100)),
            &MODERN_HANDLER,
        );

        let error = match build_routes([&FROM_041, &BEFORE_100]) {
            Ok(_) => panic!("overlapping version selectors were accepted"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            SignRouterBuildError::AmbiguousVersion {
                command: "SIGN:",
                version: 41,
            }
        ));
    }

    fn decode_mode(routes: &Routes, version: ClientVersion) -> DispatchMode {
        let decoder = routes.resolve("SIGN:", &version).copied().unwrap();
        let mut payload = Cursor::new(Bytes::new());

        decoder.decode(&version, &mut payload).unwrap().mode()
    }
}
