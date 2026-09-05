use std::{future::Future, io, net::SocketAddr};

use axum::Router;
use tokio::net::TcpListener;

use super::{Config, router};
use crate::SignService;

/// Serves the JSON Sign API over HTTP.
pub struct Server {
    listener: TcpListener,
    router: Router,
}

impl Server {
    /// Binds the configured HTTP listener around an initialized Sign service.
    pub async fn bind(config: Config, service: SignService) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(config.listen_addr).await?,
            router: router(service),
        })
    }

    /// Returns the listener's effective local address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves requests until shutdown.
    pub async fn run<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future<Output = ()> + Send + 'static,
    {
        axum::serve(self.listener, self.router)
            .with_graceful_shutdown(shutdown)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::future;

    use super::*;
    use crate::SignServiceContext;

    #[tokio::test]
    async fn binds_the_configured_listener() {
        let config = Config {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
        };
        let service = SignService::new(SignServiceContext::for_test(true).await).unwrap();
        let server = Server::bind(config, service).await.unwrap();

        assert_ne!(server.local_addr().unwrap().port(), 0);
        server.run(future::ready(())).await.unwrap();
    }
}
