//! An embeddable, in-memory job queue with leases.
//!
//! Semantics match the PostgreSQL queue in `oryn-db`: a claim atomically moves the
//! oldest eligible queued job to `claimed` with a lease; a worker must heartbeat
//! before the lease expires or the job is re-queued (or timed out when attempts
//! are exhausted).

use chrono::{DateTime, Duration, Utc};
use oryn_domain::{JobSpec, JobStatus, TestKind};
use std::collections::HashMap;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::lifecycle::{transition, IllegalTransition};

#[derive(Debug, Clone)]
pub struct QueuedJob {
    pub spec: JobSpec,
    pub kind: TestKind,
    pub status: JobStatus,
    pub worker: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
    pub enqueued_at: DateTime<Utc>,
    pub max_attempts: i32,
}

#[derive(Default)]
pub struct InMemoryQueue {
    jobs: Mutex<HashMap<Uuid, QueuedJob>>,
}

impl InMemoryQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn enqueue(&self, spec: JobSpec, kind: TestKind, max_attempts: i32) -> Uuid {
        let id = spec.job_id;
        self.jobs.lock().await.insert(
            id,
            QueuedJob {
                spec,
                kind,
                status: JobStatus::Queued,
                worker: None,
                lease_until: None,
                enqueued_at: Utc::now(),
                max_attempts,
            },
        );
        id
    }

    /// Claim the oldest queued job this worker can run.
    pub async fn claim(
        &self,
        worker: &str,
        capabilities: &[TestKind],
        lease: Duration,
    ) -> Option<JobSpec> {
        let mut jobs = self.jobs.lock().await;
        let id = jobs
            .values()
            .filter(|j| {
                j.status == JobStatus::Queued
                    && (capabilities.contains(&j.kind)
                        || (j.kind == TestKind::Mixed
                            && capabilities.contains(&TestKind::Browser)
                            && capabilities.contains(&TestKind::Api)))
            })
            .min_by_key(|j| j.enqueued_at)
            .map(|j| j.spec.job_id)?;
        let j = jobs.get_mut(&id)?;
        j.status = JobStatus::Claimed;
        j.worker = Some(worker.to_string());
        j.lease_until = Some(Utc::now() + lease);
        Some(j.spec.clone())
    }

    pub async fn heartbeat(&self, id: Uuid, worker: &str, lease: Duration) -> bool {
        let mut jobs = self.jobs.lock().await;
        match jobs.get_mut(&id) {
            Some(j) if j.worker.as_deref() == Some(worker) && !j.status.is_terminal() => {
                j.lease_until = Some(Utc::now() + lease);
                true
            }
            _ => false,
        }
    }

    pub async fn set_status(&self, id: Uuid, to: JobStatus) -> Result<(), IllegalTransition> {
        let mut jobs = self.jobs.lock().await;
        if let Some(j) = jobs.get_mut(&id) {
            j.status = transition(j.status, to)?;
        }
        Ok(())
    }

    pub async fn status(&self, id: Uuid) -> Option<JobStatus> {
        self.jobs.lock().await.get(&id).map(|j| j.status)
    }

    /// Re-queue (or time out) jobs whose lease expired. Returns affected job ids.
    pub async fn reap_expired(&self, now: DateTime<Utc>) -> Vec<Uuid> {
        let mut jobs = self.jobs.lock().await;
        let mut out = vec![];
        for j in jobs.values_mut() {
            if matches!(j.status, JobStatus::Claimed | JobStatus::Running)
                && j.lease_until.is_some_and(|l| l < now)
            {
                if j.spec.attempt < j.max_attempts {
                    j.spec.attempt += 1;
                    j.status = JobStatus::Queued;
                } else {
                    j.status = JobStatus::TimedOut;
                }
                j.worker = None;
                j.lease_until = None;
                out.push(j.spec.job_id);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oryn_domain::{JobEnvironment, JobOptions, JOB_PROTOCOL};
    use std::sync::Arc;

    fn spec() -> JobSpec {
        JobSpec {
            protocol: JOB_PROTOCOL.into(),
            job_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            scenario_id: None,
            scenario_name: "s".into(),
            attempt: 1,
            steps: vec![],
            environment: JobEnvironment::default(),
            options: JobOptions::default(),
        }
    }

    #[tokio::test]
    async fn concurrent_claims_never_double_claim() {
        let q = Arc::new(InMemoryQueue::new());
        for _ in 0..50 {
            q.enqueue(spec(), TestKind::Api, 2).await;
        }
        let mut handles = vec![];
        for w in 0..10 {
            let q = q.clone();
            handles.push(tokio::spawn(async move {
                let mut got = vec![];
                while let Some(s) = q
                    .claim(&format!("w{w}"), &[TestKind::Api], Duration::seconds(30))
                    .await
                {
                    got.push(s.job_id);
                }
                got
            }));
        }
        let mut all = vec![];
        for h in handles {
            all.extend(h.await.unwrap());
        }
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(n, 50);
        assert_eq!(all.len(), 50);
    }

    #[tokio::test]
    async fn capabilities_and_leases() {
        let q = InMemoryQueue::new();
        let id = q.enqueue(spec(), TestKind::Browser, 2).await;
        assert!(q
            .claim("api-only", &[TestKind::Api], Duration::seconds(1))
            .await
            .is_none());
        assert!(q
            .claim("w", &[TestKind::Browser], Duration::seconds(-1))
            .await
            .is_some());
        assert_eq!(q.reap_expired(Utc::now()).await, vec![id]);
        assert_eq!(q.status(id).await, Some(JobStatus::Queued));
        let again = q
            .claim("w", &[TestKind::Browser], Duration::seconds(-1))
            .await
            .unwrap();
        assert_eq!(again.attempt, 2);
        q.reap_expired(Utc::now()).await;
        assert_eq!(q.status(id).await, Some(JobStatus::TimedOut));
        assert!(q.set_status(id, JobStatus::Running).await.is_err());
    }
}
