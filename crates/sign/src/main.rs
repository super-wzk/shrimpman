use std::error::Error;

use jiff::SignedDuration;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
};
use shrimpman_sign::{SignConfig, SignDatabase, SignServer, SignService, SignServiceContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let sign = load_config()?;
    let database = SignDatabase::connect(&sign.database).await?;
    let discovery = DiscoveryClient::connect(sign.discovery)?;
    let context = SignServiceContext::new(
        sign.auto_sign_up,
        SignedDuration::try_from(sign.session.ttl)?,
        discovery.clone(),
        database.repositories(),
    );
    let service = SignService::new(context)?;
    let advertise_addr = sign.server.advertise_addr.clone();
    let server = SignServer::bind(sign.server, service).await?;
    let instance_id = ServiceInstanceId::new();
    discovery.publish(ServiceInstance::new(
        instance_id,
        ServiceName::new("sign")?,
        ServiceState::Ready,
        Some(advertise_addr),
        (),
    )?)?;

    server.run(shutdown_signal()).await?;
    discovery.withdraw(instance_id)?;

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
                .with_list_parse_key("sign.discovery.endpoints"),
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
