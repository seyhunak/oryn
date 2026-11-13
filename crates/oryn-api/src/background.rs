//! Background maintenance: lease reaper, stale-worker detection, session purge.

use crate::state::AppState;
use std::time::Duration;
use tokio::sync::watch;

pub fn spawn(state: AppState, mut stop: watch::Receiver<bool>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        let mut n: u64 = 0;
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = stop.changed() => break,
            }
            n += 1;
            match oryn_db::execution::reap_expired_leases(&state.pool).await {
                Ok(reaped) => {
                    for (job, status) in reaped {
                        tracing::warn!(%job, ?status, "job lease expired; requeued or failed");
                    }
                }
                Err(e) => tracing::error!(error = %e, "lease reaper failed"),
            }
            if let Err(e) = oryn_db::execution::mark_stale_workers_offline(&state.pool, 90).await {
                tracing::error!(error = %e, "stale worker sweep failed");
            }
            if n % 360 == 0 {
                let _ = oryn_db::identity::purge_expired_sessions(&state.pool).await;
            }
        }
        tracing::info!("background tasks stopped");
    })
}
