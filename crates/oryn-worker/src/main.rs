use oryn_worker::{Worker, WorkerConfig};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _telemetry = oryn_observability::init("oryn-worker")?;
    let cfg = WorkerConfig::from_env()?;
    // The control plane may still be starting (compose, k8s): retry the first connection.
    let mut wait = std::time::Duration::from_secs(1);
    let worker = loop {
        match Worker::connect(cfg.clone()).await {
            Ok(w) => break w,
            Err(e) => {
                tracing::warn!(error = %e, retry_in = ?wait, "cannot reach the control plane yet");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(std::time::Duration::from_secs(30));
            }
        }
    };
    let shutdown = CancellationToken::new();
    let s = shutdown.clone();
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("signal handler");
            tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
        }
        #[cfg(not(unix))]
        let _ = ctrl_c.await;
        tracing::info!("shutdown requested");
        s.cancel();
    });
    worker.run(shutdown).await
}
