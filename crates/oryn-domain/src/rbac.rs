use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{ParseEnumError, Role};

string_enum!(
    /// Fine-grained permissions enforced by the API on every request.
    Permission {
        WorkspaceRead => "workspace:read",
        WorkspaceManage => "workspace:manage",
        MembersManage => "members:manage",
        ProjectRead => "project:read",
        ProjectWrite => "project:write",
        ProjectDelete => "project:delete",
        TestWrite => "test:write",
        TestApprove => "test:approve",
        ExecutionRun => "execution:run",
        ExecutionCancel => "execution:cancel",
        WorkerManage => "worker:manage",
        AiUse => "ai:use",
        AiConfigure => "ai:configure",
        IntegrationManage => "integration:manage",
        SecretsManage => "secrets:manage",
        AuditRead => "audit:read"
    }
);

impl Role {
    /// The static role → permission matrix. Documented in docs/SECURITY.md.
    pub fn permissions(&self) -> &'static [Permission] {
        use Permission::*;
        match self {
            Role::Owner => Permission::ALL,
            Role::Admin => &[
                WorkspaceRead,
                MembersManage,
                ProjectRead,
                ProjectWrite,
                ProjectDelete,
                TestWrite,
                TestApprove,
                ExecutionRun,
                ExecutionCancel,
                WorkerManage,
                AiUse,
                AiConfigure,
                IntegrationManage,
                SecretsManage,
                AuditRead,
            ],
            Role::Engineer => &[
                WorkspaceRead,
                ProjectRead,
                ProjectWrite,
                TestWrite,
                TestApprove,
                ExecutionRun,
                ExecutionCancel,
                AiUse,
            ],
            Role::Viewer => &[WorkspaceRead, ProjectRead],
        }
    }

    pub fn can(&self, permission: Permission) -> bool {
        self.permissions().contains(&permission)
    }

    /// Numeric rank: higher is more privileged.
    pub fn rank(&self) -> u8 {
        match self {
            Role::Owner => 4,
            Role::Admin => 3,
            Role::Engineer => 2,
            Role::Viewer => 1,
        }
    }

    /// Whether a member with this role may assign `target` to someone.
    /// Nobody can grant a role above their own, and only owners can create owners.
    pub fn can_assign(&self, target: Role) -> bool {
        self.can(Permission::MembersManage) && target.rank() <= self.rank()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix() {
        assert!(Role::Owner.can(Permission::WorkspaceManage));
        assert!(!Role::Admin.can(Permission::WorkspaceManage));
        assert!(Role::Engineer.can(Permission::ExecutionRun));
        assert!(!Role::Engineer.can(Permission::SecretsManage));
        assert!(!Role::Viewer.can(Permission::ExecutionRun));
        assert!(Role::Viewer.can(Permission::ProjectRead));
    }

    #[test]
    fn assignment() {
        assert!(Role::Owner.can_assign(Role::Owner));
        assert!(Role::Admin.can_assign(Role::Engineer));
        assert!(!Role::Admin.can_assign(Role::Owner));
        assert!(!Role::Engineer.can_assign(Role::Viewer));
    }
}
