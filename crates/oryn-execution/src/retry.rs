//! Retry policy.
//!
//! By default ORYN retries only *infrastructure* errors (adapter unreachable,
//! worker lost, timeout) — never assertion failures, so a genuine defect is not
//! hidden by a lucky second attempt. Retrying failures is opt-in, and every
//! attempt is recorded, which is what flaky-test detection reads.

use oryn_domain::JobStatus;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RetryPolicy {
    /// Total attempts including the first (1 = no retry).
    pub max_attempts: i32,
    pub retry_on_failure: bool,
    pub backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 2,
            retry_on_failure: false,
            backoff_ms: 1_000,
        }
    }
}

impl RetryPolicy {
    pub fn should_retry(&self, status: JobStatus, attempt: i32) -> bool {
        if attempt >= self.max_attempts {
            return false;
        }
        match status {
            JobStatus::Error | JobStatus::TimedOut => true,
            JobStatus::Failed => self.retry_on_failure,
            _ => false,
        }
    }

    /// Exponential backoff: backoff_ms * 2^(attempt-1), capped at 60 s.
    pub fn delay(&self, attempt: i32) -> Duration {
        let exp = (attempt.max(1) - 1).min(10) as u32;
        Duration::from_millis((self.backoff_ms.saturating_mul(2u64.pow(exp))).min(60_000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions() {
        let p = RetryPolicy::default();
        assert!(p.should_retry(JobStatus::Error, 1));
        assert!(!p.should_retry(JobStatus::Error, 2));
        assert!(!p.should_retry(JobStatus::Failed, 1));
        assert!(!p.should_retry(JobStatus::Passed, 1));
        let p2 = RetryPolicy {
            retry_on_failure: true,
            max_attempts: 3,
            backoff_ms: 100,
        };
        assert!(p2.should_retry(JobStatus::Failed, 2));
        assert_eq!(p2.delay(1).as_millis(), 100);
        assert_eq!(p2.delay(3).as_millis(), 400);
    }
}
