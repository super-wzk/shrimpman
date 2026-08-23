use std::error::Error;

use shrimpman_discovery::server::{DiscoveryConfig, DiscoveryServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = shrimpman_config::load()?;
    let discovery = DiscoveryConfig::try_from(&config)?;
    let server = DiscoveryServer::bind(discovery.server).await?;

    server.run(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(terminate) => terminate,
            Err(error) => {
                eprintln!("failed to listen for SIGTERM: {error}");
                return;
            }
        };

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    eprintln!("failed to listen for Ctrl-C: {error}");
                }
            }
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("failed to listen for Ctrl-C: {error}");
    }
}
