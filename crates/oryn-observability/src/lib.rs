//! Observability bootstrap for ORYN binaries.
//!
//! * `RUST_LOG` controls filtering (default `info`).
//! * `ORYN_LOG_FORMAT=json` switches to JSON structured logs (default: human-readable).
//! * `OTEL_EXPORTER_OTLP_ENDPOINT` (e.g. `http://localhost:4318`) enables OTLP/HTTP trace export.
//!
//! Sensitive values must never be logged: callers pass IDs, not payloads.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Guard that flushes and shuts down the tracer provider on drop.
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(p) = self.provider.take() {
            let _ = p.shutdown();
        }
    }
}

pub fn init(service_name: &'static str) -> anyhow::Result<TelemetryGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("ORYN_LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false);

    let fmt_layer = if json {
        tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .with_target(true)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_target(true)
            .compact()
            .boxed()
    };

    let mut provider = None;
    let otel_layer = match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => {
            use opentelemetry_otlp::WithExportConfig;
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(format!("{}/v1/traces", endpoint.trim_end_matches('/')))
                .build()?;
            let p = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    opentelemetry_sdk::Resource::builder()
                        .with_service_name(service_name)
                        .build(),
                )
                .build();
            let tracer = p.tracer(service_name);
            provider = Some(p);
            Some(tracing_opentelemetry::layer().with_tracer(tracer))
        }
        _ => None,
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .try_init()?;
    Ok(TelemetryGuard { provider })
}
