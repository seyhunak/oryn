# 0002: PostgreSQL job queue with SKIP LOCKED leases

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

Executions fan out into one job per scenario. Jobs must be claimed exactly once by one of many workers, survive worker crashes, be cancellable, and be retried as distinct attempts so evidence for each attempt is kept. Execution totals must stay consistent when many jobs complete at once. What should hold the queue?

## Decision drivers

- No additional infrastructure for a self-hosted product; PostgreSQL is already required.
- Transactional consistency between job state, execution totals and job events.
- At-least-once delivery with crash recovery.

## Considered options

1. PostgreSQL table with `FOR UPDATE SKIP LOCKED` claims and time-based leases.
2. Redis lists or streams.
3. A message broker (RabbitMQ, NATS, Kafka).
4. An in-process queue in the API.

## Decision outcome

Option 1, implemented in [`crates/oryn-db/src/execution.rs`](../../crates/oryn-db/src/execution.rs):

- Claim selects the oldest queued job whose kind the worker supports and whose workspace the worker serves, with `FOR UPDATE SKIP LOCKED`, sets `lease_until`, and records `job.claimed`.
- Workers renew the lease every third of `ORYN_JOB_LEASE_SECS` and learn about cancellation from the renewal response.
- A background task in the API reaps expired leases every 10 seconds: the job is marked `timed_out` (or `cancelled` if cancellation was requested) and, if attempts remain, a new job row is inserted for the next attempt.
- Retries are new rows (`attempt` + 1, `retry_of`), never in-place resets, so every attempt keeps its steps and artifacts. The superseded attempt does not count toward execution totals.
- Recomputing execution totals locks the execution row first, so concurrent completions serialise.

`oryn-execution` also contains an `InMemoryQueue` used only in tests.

### Consequences

- Good: one transaction updates a job, inserts its retry and recomputes the execution; no dual-write problems.
- Good: queue state is visible with SQL and survives restarts; tests in `crates/oryn-db/tests/queue.rs` cover claims, leases and retries.
- Bad: workers poll (default every 2 s) instead of receiving pushes, adding latency and load proportional to idle workers.
- Bad: throughput is bounded by PostgreSQL; fine for test jobs (seconds to minutes each), not for high-frequency messaging.
- Neutral: Redis is used only for rate-limit counters and is optional.

## Pros and cons of the other options

- **Redis**: low latency, but a second source of truth whose state must be reconciled with PostgreSQL, and persistence depends on configuration.
- **Message broker**: proper push delivery, but another stateful service to operate and no transactional link to execution rows.
- **In-process**: lost on restart and impossible with more than one API replica.
