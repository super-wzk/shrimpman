use tracing::{error, info};

/// 等待 Ctrl-C 或 Unix SIGTERM；信号监听失败时也启动退出流程。
pub async fn shutdown_signal() {
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
            result = tokio::signal::ctrl_c() => report_ctrl_c(result),
            _ = terminate.recv() => info!("Received SIGTERM; shutting down"),
        }
    }

    #[cfg(not(unix))]
    report_ctrl_c(tokio::signal::ctrl_c().await);
}

fn report_ctrl_c(result: std::io::Result<()>) {
    match result {
        Ok(()) => info!("Received Ctrl-C; shutting down"),
        Err(error) => error!(%error, "Failed to listen for Ctrl-C"),
    }
}
