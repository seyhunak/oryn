//! Job lifecycle state machine.
//!
//! ```text
//! queued ──claim──▶ claimed ──start──▶ running ──▶ passed | failed | error | timed_out
//!   │                 │                   │
//!   └──cancel─────────┴──────cancel───────┴──▶ cancelled
//! claimed/running ──lease expired──▶ queued (retry) | timed_out
//! ```

use oryn_domain::JobStatus;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("illegal job transition {from} -> {to}")]
pub struct IllegalTransition {
    pub from: JobStatus,
    pub to: JobStatus,
}

pub fn can_transition(from: JobStatus, to: JobStatus) -> bool {
    use JobStatus::*;
    match (from, to) {
        (Queued, Claimed) | (Queued, Cancelled) => true,
        (Claimed, Running) | (Claimed, Queued) | (Claimed, Cancelled) | (Claimed, TimedOut) => true,
        (Claimed, Passed | Failed | Error) => true, // fast jobs may report without a separate start
        (Running, Passed | Failed | Error | Cancelled | TimedOut | Queued) => true,
        _ => false,
    }
}

pub fn transition(from: JobStatus, to: JobStatus) -> Result<JobStatus, IllegalTransition> {
    if can_transition(from, to) {
        Ok(to)
    } else {
        Err(IllegalTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use JobStatus::*;

    #[test]
    fn legal_and_illegal() {
        assert!(transition(Queued, Claimed).is_ok());
        assert!(transition(Running, Passed).is_ok());
        assert!(transition(Passed, Running).is_err());
        assert!(transition(Cancelled, Queued).is_err());
        assert!(transition(Queued, Passed).is_err());
        for t in JobStatus::ALL.iter().filter(|s| s.is_terminal()) {
            for to in JobStatus::ALL {
                assert!(!can_transition(*t, *to), "{t} is terminal");
            }
        }
    }
}
