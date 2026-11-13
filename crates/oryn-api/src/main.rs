use oryn_api::{app, background, config::Config, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _telemetry = oryn_observability::init("oryn-api");
    let cfg = Config::from_env()?;
    let pool = oryn_db::connect(&cfg.database_url, 20).await?;
    oryn_db::migrate(&pool).await?;
    tracing::info!("database migrated");
    let state = AppState::new(cfg.clone(), pool).await?;

    let (tx, rx) = tokio::sync::watch::channel(false);
    let bg = background::spawn(state.clone(), rx);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!(addr = %cfg.bind, "ORYN API listening");
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        tracing::info!("shutting down");
        let _ = tx.send(true);
    })
    .await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), bg).await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
