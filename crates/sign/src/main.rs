use std::error::Error;

use jiff::SignedDuration;
use shrimpman_sign::{SignConfig, SignDatabase, SignServer, SignService, SignServiceContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let sign = load_config()?;
    let database = SignDatabase::connect(&sign.database).await?;
    let context = SignServiceContext::new(
        sign.auto_sign_up,
        SignedDuration::try_from(sign.session.ttl)?,
        database.account_repository(),
        database.character_repository(),
        database.mezeporta_festival_repository(),
        database.sign_session_repository(),
        database.sign_in_notice_repository(),
    );
    let service = SignService::new(context)?;
    let server = SignServer::bind(sign.server, service).await?;

    server.run(shutdown_signal()).await?;

    Ok(())
}

fn load_config() -> Result<SignConfig, config::ConfigError> {
    let config = shrimpman_config::load()?;

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
