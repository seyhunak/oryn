//! Outbound notifications (webhooks and Slack) with SSRF protection and HMAC signatures.

use crate::state::AppState;
use oryn_db::governance::Integration;
use serde_json::{json, Value};
use std::time::Instant;
use uuid::Uuid;

/// Validate an outbound URL for a friendly error at configuration time: https (http
/// only on private networks), and every resolved address allowed. Delivery itself
/// goes through `AppState::outbound`, which re-checks and pins the address.
pub async fn check_outbound(raw: &str, allow_private: bool) -> Result<url::Url, String> {
    let u = url::Url::parse(raw).map_err(|e| format!("invalid URL: {e}"))?;
    match u.scheme() {
        "https" => {}
        "http" if allow_private => {}
        _ => return Err("only https URLs are allowed".into()),
    }
    oryn_auth::net::check_url_host(&u, allow_private).await?;
    Ok(u)
}

pub async fn deliver(
    s: &AppState,
    i: &Integration,
    event: &str,
    payload: &Value,
) -> (Option<i32>, Option<String>) {
    let secret = match &i.secret_encrypted {
        Some(x) => match s.open(x) {
            Ok(v) => Some(v),
            Err(_) => return (None, Some("stored secret could not be decrypted".into())),
        },
        None => None,
    };
    let (target, body, signed) = match i.kind.as_str() {
        "webhook" => {
            let Some(url) = i.config["url"].as_str() else {
                return (None, Some("webhook has no url".into()));
            };
            (url.to_string(), payload.clone(), true)
        }
        "slack" => {
            let Some(url) = secret.clone() else {
                return (None, Some("Slack integration has no webhook URL".into()));
            };
            (url, json!({"text": slack_text(payload)}), false)
        }
        other => {
            return (
                None,
                Some(format!(
                    "delivery for {other} integrations is not implemented"
                )),
            )
        }
    };
    let url = match check_outbound(&target, s.cfg.allow_private_network).await {
        Ok(u) => u,
        Err(e) => return (None, Some(e)),
    };
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    let mut rb = s
        .outbound
        .post(url)
        .header("content-type", "application/json")
        .header("x-oryn-event", event)
        .header("user-agent", "oryn-webhooks/1");
    if signed {
        if let Some(sec) = &secret {
            rb = rb.header(
                "x-oryn-signature",
                format!("sha256={}", oryn_auth::sign_hmac(sec.as_bytes(), &bytes)),
            );
        }
    }
    match rb.body(bytes).send().await {
        Ok(r) => {
            let code = r.status().as_u16() as i32;
            (
                Some(code),
                if r.status().is_success() {
                    None
                } else {
                    Some(format!("HTTP {code}"))
                },
            )
        }
        Err(e) => {
            // Surface the cause (for example a refused private address) from the chain.
            let mut msg = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(c) = src {
                msg.push_str(&format!(": {c}"));
                src = c.source();
            }
            (None, Some(msg))
        }
    }
}

fn slack_text(p: &Value) -> String {
    let e = &p["execution"];
    format!(
        "{} *{}* {} on {} — {} passed, {} failed, {} errored of {}. {}",
        if e["status"] == "passed" {
            "✅"
        } else {
            "❌"
        },
        e["title"].as_str().unwrap_or("Execution"),
        e["status"].as_str().unwrap_or(""),
        e["environment_name"].as_str().unwrap_or(""),
        e["passed"],
        e["failed"],
        e["errored"],
        e["total"],
        p["url"].as_str().unwrap_or("")
    )
}

/// Fire `execution.finished` (and `execution.failed` when it did not pass).
pub fn execution_finished(s: AppState, eid: Uuid) {
    tokio::spawn(async move {
        let Ok(e) = oryn_db::execution::get_execution(&s.pool, eid).await else {
            return;
        };
        let payload = json!({
            "event": "execution.finished",
            "execution": e,
            "url": format!("{}/executions/{}", s.cfg.public_url, eid),
        });
        let mut events = vec!["execution.finished"];
        if e.status != "passed" {
            events.push("execution.failed");
        }
        let mut seen = std::collections::HashSet::new();
        for ev in events {
            let Ok(list) = oryn_db::governance::integrations_for_event(
                &s.pool,
                e.workspace_id,
                e.project_id,
                ev,
            )
            .await
            else {
                continue;
            };
            for i in list {
                if !seen.insert(i.id) {
                    continue;
                }
                let start = Instant::now();
                let (status, err) = deliver(&s, &i, ev, &payload).await;
                if let Some(er) = &err {
                    tracing::warn!(integration = %i.id, error = %er, "notification failed");
                }
                let _ = oryn_db::governance::record_delivery(
                    &s.pool,
                    i.id,
                    ev,
                    status,
                    err.as_deref(),
                    start.elapsed().as_millis() as i64,
                )
                .await;
            }
        }
    });
}
