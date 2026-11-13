use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ParseEnumError;

/// Declares a string-backed enum with `as_str`, `FromStr`, `Display` and serde snake_case.
#[macro_export]
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(&self) -> &'static str {
                match self { $($name::$variant => $s),+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = ParseEnumError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($s => Ok($name::$variant),)+
                    _ => Err(ParseEnumError { kind: stringify!($name), value: s.to_string() }),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum!(
    /// Workspace role. Ordered from most to least privileged.
    Role { Owner => "owner", Admin => "admin", Engineer => "engineer", Viewer => "viewer" }
);

string_enum!(
    /// Lifecycle of a test execution (a run of one or more scenarios).
    ExecutionStatus {
        Queued => "queued", Running => "running", Passed => "passed",
        Failed => "failed", Error => "error", Cancelled => "cancelled"
    }
);

impl ExecutionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Passed | Self::Failed | Self::Error | Self::Cancelled
        )
    }
}

string_enum!(
    /// Lifecycle of a single job (one scenario attempt) on the execution fabric.
    JobStatus {
        Queued => "queued", Claimed => "claimed", Running => "running", Passed => "passed",
        Failed => "failed", Error => "error", Cancelled => "cancelled", TimedOut => "timed_out"
    }
);

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Passed | Self::Failed | Self::Error | Self::Cancelled | Self::TimedOut
        )
    }
}

string_enum!(
    /// Result of one executed step.
    StepStatus { Passed => "passed", Failed => "failed", Skipped => "skipped", Error => "error" }
);

string_enum!(
    /// Governance status of a scenario: AI-generated scenarios start as drafts and
    /// must be approved before they can execute (unless the project opts out).
    ScenarioStatus { Draft => "draft", Approved => "approved", Archived => "archived" }
);

string_enum!(
    /// What a scenario exercises.
    TestKind { Browser => "browser", Api => "api", Mixed => "mixed" }
);

string_enum!(
    Browser { Chromium => "chromium", Firefox => "firefox", Webkit => "webkit" }
);

string_enum!(
    WorkerStatus { Online => "online", Offline => "offline", Draining => "draining", Revoked => "revoked" }
);

string_enum!(
    /// Classification produced by the failure investigator.
    FailureCategory {
        ApplicationDefect => "application_defect", TestDefect => "test_defect",
        EnvironmentFailure => "environment_failure", DependencyFailure => "dependency_failure",
        AuthenticationFailure => "authentication_failure", FlakyTest => "flaky_test", Unknown => "unknown"
    }
);

string_enum!(
    ArtifactKind {
        Screenshot => "screenshot", Video => "video", Trace => "trace", Log => "log",
        Console => "console", Network => "network", Response => "response"
    }
);

string_enum!(
    ApprovalStatus { Pending => "pending", Approved => "approved", Rejected => "rejected" }
);

string_enum!(
    TestPlanStatus { Draft => "draft", Approved => "approved", Rejected => "rejected" }
);

string_enum!(
    RequirementKind {
        Requirement => "requirement", UserStory => "user_story",
        AcceptanceCriteria => "acceptance_criteria", Document => "document", ApiSpec => "api_spec"
    }
);

string_enum!(
    RepositorySource { Github => "github", Gitlab => "gitlab", Git => "git", Zip => "zip", Local => "local" }
);

string_enum!(
    AiProviderKind {
        Openai => "openai", Anthropic => "anthropic", Gemini => "gemini",
        AzureOpenai => "azure_openai", Ollama => "ollama", OpenaiCompatible => "openai_compatible",
        Mock => "mock"
    }
);

string_enum!(
    IntegrationKind {
        Github => "github", Gitlab => "gitlab", Jira => "jira", Slack => "slack",
        GithubActions => "github_actions", GitlabCi => "gitlab_ci", Webhook => "webhook"
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn roundtrip_all_enums() {
        for r in Role::ALL {
            assert_eq!(Role::from_str(r.as_str()).unwrap(), *r);
        }
        for s in JobStatus::ALL {
            assert_eq!(JobStatus::from_str(s.as_str()).unwrap(), *s);
        }
        assert!(FailureCategory::from_str("nope").is_err());
        assert_eq!(
            serde_json::to_string(&JobStatus::TimedOut).unwrap(),
            "\"timed_out\""
        );
    }

    #[test]
    fn terminal_states() {
        assert!(JobStatus::TimedOut.is_terminal());
        assert!(!JobStatus::Claimed.is_terminal());
        assert!(ExecutionStatus::Cancelled.is_terminal());
        assert!(!ExecutionStatus::Running.is_terminal());
    }
}
