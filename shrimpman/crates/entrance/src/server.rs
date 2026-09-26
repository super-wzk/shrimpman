use std::{future::Future, io, net::SocketAddr, sync::Arc};

use shrimpman_runtime::serve_tcp;
use tokio::net::TcpListener;
use tracing::Instrument;

use crate::{EntranceServerConfig, EntranceService};

/// Accepts TCP connections and dispatches them to the Entrance service.
pub struct EntranceServer {
    listener: TcpListener,
    service: Arc<EntranceService>,
}

impl EntranceServer {
    /// Binds the configured listener around an initialized Entrance service.
    pub async fn bind(config: EntranceServerConfig, service: EntranceService) -> io::Result<Self> {
        let listener = TcpListener::bind(config.listen_addr).await?;

        Ok(Self {
            listener,
            service: Arc::new(service),
        })
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
            let service = Arc::clone(&service);
            let span = tracing::info_span!("entrance_connection", %peer_addr);
            async move {
                tracing::debug!("Accepted Entrance connection");
                match service.serve_connection(io).await {
                    Ok(()) => tracing::debug!("Closed Entrance connection"),
                    Err(error) => tracing::warn!(%error, "Entrance connection failed"),
                }
            }
            .instrument(span)
        })
        .instrument(tracing::info_span!("entrance_server"))
        .await
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::{SinkExt, StreamExt};
    use shrimpman_discovery::client::DiscoveryClient;
    use shrimpman_lease_kv::LeaseKvClient;
    use shrimpman_transport::MhfConnection;
    use tokio::{io::AsyncWriteExt, net::TcpStream, sync::oneshot};

    use super::*;
    use crate::EntranceServiceContext;

    fn service() -> EntranceService {
        EntranceService::new(EntranceServiceContext::new(DiscoveryClient::new(
            LeaseKvClient::connect(Default::default()).unwrap(),
        )))
        .unwrap()
    }

    #[tokio::test]
    async fn binds_and_closes_unsupported_connections() {
        let config = EntranceServerConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            advertise_addr: "127.0.0.1:53310".to_owned(),
        };
        let listen_addr = config.listen_addr;
        let server = EntranceServer::bind(config, service()).await.unwrap();
        let local_addr = server.local_addr().unwrap();

        assert_eq!(local_addr.ip(), listen_addr.ip());
        assert_ne!(local_addr.port(), listen_addr.port());

        let (shutdown, shutdown_signal) = oneshot::channel();
        let running = tokio::spawn(server.run(async move {
            let _ = shutdown_signal.await;
        }));
        let mut client_io = TcpStream::connect(local_addr).await.unwrap();
        client_io.write_all(&[0; 8]).await.unwrap();
        let mut client = MhfConnection::new(client_io);
        client.send(Bytes::from_static(b"UNKNOWN\0")).await.unwrap();

        assert!(client.next().await.is_none());

        shutdown.send(()).unwrap();
        running.await.unwrap().unwrap();
    }
}
