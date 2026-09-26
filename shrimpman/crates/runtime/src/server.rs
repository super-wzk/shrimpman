use std::{future::Future, io, net::SocketAddr};

use tokio::{
    net::{TcpListener, TcpStream},
    task::{JoinError, JoinSet},
};

/// 接受连接并独立运行处理任务；正常退出时等待连接完成，监听失败时取消连接。
///
/// 处理器负责连接级日志及协议错误；调用方负责整个退出过程的超时。
pub async fn serve_tcp<Shutdown, Serve, Session>(
    listener: TcpListener,
    shutdown: Shutdown,
    serve: Serve,
) -> io::Result<()>
where
    Shutdown: Future<Output = ()>,
    Serve: Fn(TcpStream, SocketAddr) -> Session,
    Session: Future<Output = ()> + Send + 'static,
{
    let mut sessions = JoinSet::new();
    tokio::pin!(shutdown);

    let result = loop {
        tokio::select! {
            biased;

            // 退出信号优先，避免持续到来的连接推迟停止接入。
            _ = &mut shutdown => break Ok(()),
            Some(result) = sessions.join_next(), if !sessions.is_empty() => {
                report_join_error(result);
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((io, peer_addr)) => { sessions.spawn(serve(io, peer_addr)); }
                    Err(error) => break Err(error),
                }
            }
        }
    };

    drop(listener);
    if let Err(error) = result {
        sessions.shutdown().await;
        return Err(error);
    }

    if !sessions.is_empty() {
        tracing::info!(
            active_connections = sessions.len(),
            "Waiting for active TCP connections"
        );
        while let Some(result) = sessions.join_next().await {
            report_join_error(result);
        }
        tracing::info!("All active TCP connections completed");
    }
    Ok(())
}

fn report_join_error(result: Result<(), JoinError>) {
    if let Err(error) = result {
        tracing::error!(%error, "TCP connection task terminated unexpectedly");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::{Semaphore, oneshot};

    use super::*;

    #[tokio::test]
    async fn shutdown_waits_for_the_active_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let started = Arc::new(Semaphore::new(0));
        let finish = Arc::new(Semaphore::new(0));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = tokio::spawn(serve_tcp(listener, async { shutdown_rx.await.unwrap() }, {
            let started = Arc::clone(&started);
            let finish = Arc::clone(&finish);
            move |_io, _peer_addr| {
                let started = Arc::clone(&started);
                let finish = Arc::clone(&finish);
                async move {
                    started.add_permits(1);
                    finish.acquire().await.unwrap().forget();
                }
            }
        }));
        let _client = TcpStream::connect(address).await.unwrap();
        started.acquire().await.unwrap().forget();
        shutdown_tx.send(()).unwrap();
        tokio::task::yield_now().await;
        assert!(!server.is_finished());

        finish.add_permits(1);
        server.await.unwrap().unwrap();
        assert!(TcpStream::connect(address).await.is_err());
    }
}
