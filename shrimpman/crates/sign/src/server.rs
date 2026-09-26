use std::{future::Future, io, net::SocketAddr};

use shrimpman_runtime::serve_tcp;
use tokio::net::TcpListener;
use tracing::Instrument;

use crate::{SignServerConfig, SignService};

/// Accepts TCP connections and dispatches them to the Sign service.
pub struct SignServer {
    listener: TcpListener,
    service: SignService,
}

impl SignServer {
    /// Binds the configured listener around an initialized Sign service.
    pub async fn bind(config: SignServerConfig, service: SignService) -> io::Result<Self> {
        let listener = TcpListener::bind(config.listen_addr).await?;

        Ok(Self { listener, service })
    }

    /// Returns the listener's effective local address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves connections until the shutdown future completes or the listener fails.
    ///
    /// After accepting stops, waits for active connections to finish.
    pub async fn run<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future<Output = ()>,
    {
        let Self { listener, service } = self;
        serve_tcp(listener, shutdown, move |io, peer_addr| {
            let service = service.clone();
            let span = tracing::info_span!("sign_connection", %peer_addr);
            async move {
                tracing::debug!("Accepted Sign connection");
                match service.serve_connection(io).await {
                    Ok(()) => tracing::debug!("Closed Sign connection"),
                    Err(error) => tracing::warn!(%error, "Sign connection failed"),
                }
            }
            .instrument(span)
        })
        .instrument(tracing::info_span!("sign_server"))
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::future;

    use crate::SignServiceContext;

    use super::*;

    #[tokio::test]
    async fn binds_the_configured_listener() {
        let config = SignServerConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            advertise_addr: "127.0.0.1:53312".to_owned(),
        };
        let listen_addr = config.listen_addr;
        let service = SignService::new(SignServiceContext::for_test(true).await).unwrap();
        let server = SignServer::bind(config, service).await.unwrap();
        let local_addr = server.local_addr().unwrap();

        assert_eq!(local_addr.ip(), listen_addr.ip());
        assert_ne!(local_addr.port(), listen_addr.port());
        server.run(future::ready(())).await.unwrap();
    }
}
