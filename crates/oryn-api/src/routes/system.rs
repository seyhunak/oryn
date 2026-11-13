use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(healthz))
        .routes(routes!(readyz))
        .routes(routes!(step_library))
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_secs: u64,
}

/// Liveness: the process is serving.
#[utoipa::path(get, path = "/healthz", tag = "system", responses((status = 200, body = Health)))]
async fn healthz(State(s): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: s.started.elapsed().as_secs(),
    })
}

#[derive(Serialize, ToSchema)]
pub struct Ready {
    pub database: bool,
    pub redis: Option<bool>,
    pub rate_limit_backend: &'static str,
}

/// Readiness: dependencies reachable. 503 when the database is down.
#[utoipa::path(get, path = "/readyz", tag = "system", responses((status = 200, body = Ready), (status = 503, body = Ready)))]
async fn readyz(State(s): State<AppState>) -> (StatusCode, Json<Ready>) {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&s.pool)
        .await
        .is_ok();
    let redis = s.limiter.redis_ok().await;
    let code = if db {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(Ready {
            database: db,
            redis,
            rate_limit_backend: s.limiter.backend(),
        }),
    )
}

#[derive(Serialize, ToSchema)]
pub struct StepDef {
    pub pattern: String,
    pub kind: String,
    pub description: String,
    pub example: String,
}

/// The closed library of supported Gherkin step phrasings.
#[utoipa::path(get, path = "/api/v1/step-library", tag = "testing", responses((status = 200, body = Vec<StepDef>)))]
async fn step_library() -> Json<Vec<StepDef>> {
    Json(
        oryn_bdd::step_library()
            .into_iter()
            .map(|d| StepDef {
                pattern: d.pattern.into(),
                kind: d.kind.into(),
                description: d.description.into(),
                example: d.example.into(),
            })
            .collect(),
    )
}
