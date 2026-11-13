//! HTTP routes. Every route is registered through `utoipa-axum`, so the OpenAPI
//! document at `/api/openapi.json` is generated from the same handlers that serve traffic.

pub mod ai;
pub mod auth;
pub mod executions;
pub mod integrations;
pub mod intel;
pub mod planner;
pub mod projects;
pub mod repos;
pub mod requirements;
pub mod system;
pub mod testing;
pub mod workers;
pub mod workspaces;

use crate::state::AppState;
use axum::Router;
use serde::{Deserialize, Serialize};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;

#[derive(OpenApi)]
#[openapi(
    info(title = "ORYN API", description = "Self-hosted AI quality engineering platform. AI reasons. Rust executes. Evidence proves. Humans govern.", license(name = "Apache-2.0")),
    modifiers(&Security),
    tags(
        (name = "system"), (name = "auth"), (name = "workspaces"), (name = "projects"), (name = "repositories"),
        (name = "requirements"), (name = "testing"), (name = "planner"), (name = "executions"), (name = "workers"),
        (name = "worker-protocol", description = "Used by remote workers; authenticated with a worker token"),
        (name = "intelligence"), (name = "ai"), (name = "integrations")
    )
)]
pub struct ApiDoc;

struct Security;
impl Modify for Security {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let c = openapi.components.get_or_insert_with(Default::default);
        c.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(
                crate::auth::SESSION_COOKIE,
            ))),
        );
        c.add_security_scheme(
            "token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("oryn_pat_…")
                    .build(),
            ),
        );
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Ok {
    pub ok: bool,
}

pub fn ok() -> axum::Json<Ok> {
    axum::Json(Ok { ok: true })
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Paged<T: ToSchema> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

pub fn build(state: AppState) -> (Router, utoipa::openapi::OpenApi) {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(system::routes())
        .merge(auth::routes())
        .merge(workspaces::routes())
        .merge(projects::routes())
        .merge(repos::routes())
        .merge(requirements::routes())
        .merge(testing::routes())
        .merge(planner::routes())
        .merge(executions::routes())
        .merge(workers::routes())
        .merge(intel::routes())
        .merge(ai::routes())
        .merge(integrations::routes())
        .split_for_parts();
    (router.with_state(state), api)
}
