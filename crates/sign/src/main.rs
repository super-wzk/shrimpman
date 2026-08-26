use std::error::Error;

use jiff::SignedDuration;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
    selector::RoundRobinSelector,
};
use shrimpman_lease_kv::LeaseKvClient;
use shrimpman_sign::{SignConfig, SignDatabase, SignServer, SignService, SignServiceContext};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: ServiceName = ServiceName::from_static("sign");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let sign = load_config()?;
    init_tracing(&sign.logging.filter)?;
    info!("Starting Sign service");

    info!(
        auto_sign_up = sign.auto_sign_up,
        listen_addr = %sign.server.listen_addr,
        advertise_addr = %sign.server.advertise_addr,
        "Loaded Sign configuration"
    );

    let database = SignDatabase::connect(&sign.database).await?;
    info!("Connected to Sign database");

    let lease_kv = LeaseKvClient::connect(sign.lease_kv)?;
    let discovery = DiscoveryClient::new(lease_kv);
    let context = SignServiceContext::new(
        sign.auto_sign_up,
        SignedDuration::try_from(sign.session.ttl)?,
        discovery.clone(),
        RoundRobinSelector::new(),
        database.repositories(),
    );
    let service = SignService::new(context)?;
    let advertise_addr = sign.server.advertise_addr.clone();
    let server = SignServer::bind(sign.server, service).await?;
    let listen_addr = server.local_addr()?;
    info!(%listen_addr, %advertise_addr, "Sign server is ready");

    let instance_id = ServiceInstanceId::new();
    discovery.publish(ServiceInstance::new(
        instance_id,
        SERVICE_NAME,
        ServiceState::Ready,
        Some(advertise_addr),
        (),
    )?)?;

    server.run(shutdown_signal()).await?;
    info!("Sign server stopped");

    discovery.withdraw(instance_id)?;

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

fn load_config() -> Result<SignConfig, config::ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(
            config::Environment::with_prefix("SHRIMPMAN")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("sign.lease_kv.endpoints"),
        )
        .build()?;

    SignConfig::try_from(&config)
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
