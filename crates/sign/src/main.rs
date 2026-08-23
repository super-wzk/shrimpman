use std::error::Error;

use shrimpman_sign::{SignConfig, SignServer, SignService, SignServiceContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = load_config()?;
    let service = SignService::new(SignServiceContext)?;
    let server = SignServer::bind(config.server, service).await?;

    server.run(shutdown_signal()).await?;

    Ok(())
}

fn load_config() -> Result<SignConfig, config::ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(config::Environment::with_prefix("SHRIMPMAN").separator("__"))
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
