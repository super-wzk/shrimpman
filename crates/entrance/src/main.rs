use std::error::Error;

use shrimpman_discovery::client::DiscoveryClient;
use shrimpman_entrance::{EntranceConfig, EntranceServer, EntranceService, EntranceServiceContext};
use shrimpman_lease_kv::LeaseKvClient;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let entrance = load_config()?;
    init_tracing(&entrance.logging.filter)?;
    info!("Starting Entrance service");

    info!(
        listen_addr = %entrance.server.listen_addr,
        advertise_addr = %entrance.server.advertise_addr,
        "Loaded Entrance configuration"
    );

    let lease_kv = LeaseKvClient::connect(entrance.lease_kv)?;
    let discovery = DiscoveryClient::new(lease_kv);
    let context = EntranceServiceContext::new(discovery);
    let service = EntranceService::new(context)?;
    let advertise_addr = entrance.server.advertise_addr.clone();
    let server = EntranceServer::bind(entrance.server, service).await?;
    let listen_addr = server.local_addr()?;
    info!(%listen_addr, %advertise_addr, "Entrance server is listening");

    server.run(shutdown_signal()).await?;
    info!("Entrance server stopped");

    Ok(())
}

fn init_tracing(config_filter: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let filter = match std::env::var(EnvFilter::DEFAULT_ENV) {
        Ok(filter) => filter,
        Err(std::env::VarError::NotPresent) => config_filter.to_owned(),
        Err(error) => return Err(error.into()),
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(filter)?)
        .try_init()?;
    Ok(())
}

fn load_config() -> Result<EntranceConfig, config::ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(
            config::Environment::with_prefix("SHRIMPMAN")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("entrance.lease_kv.endpoints"),
        )
        .build()?;

    EntranceConfig::try_from(&config)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(terminate) => terminate,
            Err(error) => {
                error!(%error, "Failed to listen for SIGTERM");
                return;
            }
        };

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                match result {
                    Ok(()) => info!("Received Ctrl-C; shutting down"),
                    Err(error) => error!(%error, "Failed to listen for Ctrl-C"),
                }
            }
            _ = terminate.recv() => info!("Received SIGTERM; shutting down"),
        }
    }

    #[cfg(not(unix))]
    match tokio::signal::ctrl_c().await {
        Ok(()) => info!("Received Ctrl-C; shutting down"),
        Err(error) => error!(%error, "Failed to listen for Ctrl-C"),
    }
}
