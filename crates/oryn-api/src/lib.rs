//! ORYN control plane.

pub mod auth;
pub mod background;
pub mod config;
pub mod error;
pub mod routes;
pub mod services;
pub mod state;

use axum::http::{header, HeaderName, HeaderValue, Method};
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

pub use state::AppState;

const DOCS_HTML: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><title>ORYN API reference</title>
<meta name="viewport" content="width=device-width, initial-scale=1"></head><body>
<redoc spec-url="/api/openapi.json"></redoc>
<script src="https://cdn.jsdelivr.net/npm/redoc@2.1.5/bundles/redoc.standalone.js"></script></body></html>"#;

/// Build the full HTTP application.
pub fn app(state: AppState) -> Router {
    let (api, openapi) = routes::build(state.clone());
    let openapi = std::sync::Arc::new(openapi);
    let x_request_id = HeaderName::from_static("x-request-id");

    let cors = if state.cfg.cors_origins.is_empty() {
        CorsLayer::new()
    } else {
        let origins: Vec<HeaderValue> = state
            .cfg
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_credentials(true)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers([
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                HeaderName::from_static(auth::CSRF_HEADER),
            ])
    };

    api.route("/api/openapi.json", get(move || { let o = openapi.clone(); async move { Json((*o).clone()) } }))
        .route("/api/docs", get(|| async { Html(DOCS_HTML) }))
        .layer(RequestBodyLimitLayer::new(state.cfg.max_upload_bytes))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(cors)
        .layer(TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
            let id = req.headers().get("x-request-id").and_then(|v| v.to_str().ok()).unwrap_or("");
            tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id = %id)
        }))
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .layer(SetSensitiveRequestHeadersLayer::new([header::AUTHORIZATION, header::COOKIE]))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(axum::http::StatusCode::REQUEST_TIMEOUT, Duration::from_secs(300)))
}
