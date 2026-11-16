"use client";
import { useCan } from "@/lib/hooks";
import { PageHeader } from "@/components/ui/page";
import { UrlTabs, useTab } from "@/components/ui/tabs";
import { WorkspaceTab } from "@/components/settings/workspace-tab";
import { MembersTab } from "@/components/settings/members-tab";
import { TeamsTab } from "@/components/settings/teams-tab";
import { TokensTab } from "@/components/settings/tokens-tab";
import { AiTab } from "@/components/settings/ai-tab";
import { ConnectionsTab } from "@/components/settings/connections-tab";
import { AuditTab } from "@/components/settings/audit-tab";
import { AppearanceTab } from "@/components/settings/appearance-tab";
import { Empty } from "@/components/ui/states";

const TABS = [
  { id: "workspace", label: "Workspace" },
  { id: "members", label: "Members" },
  { id: "teams", label: "Teams" },
  { id: "tokens", label: "API tokens" },
  { id: "ai", label: "AI models" },
  { id: "connections", label: "Repository connections" },
  { id: "audit", label: "Audit log" },
  { id: "appearance", label: "Appearance" },
];

export default function SettingsPage() {
  const tab = useTab("workspace");
  const canAudit = useCan("admin");
  return (
    <>
      <PageHeader title="Settings" description="The workspace, who can use it, how machines and models connect to it, what changed, and how ORYN looks for you." />
      <UrlTabs tabs={TABS} />
      {tab === "appearance" ? (
        <AppearanceTab />
      ) : tab === "members" ? (
        <MembersTab />
      ) : tab === "teams" ? (
        <TeamsTab />
      ) : tab === "tokens" ? (
        <TokensTab />
      ) : tab === "ai" ? (
        <AiTab />
      ) : tab === "connections" ? (
        <ConnectionsTab />
      ) : tab === "audit" ? (
        canAudit ? (
          <AuditTab />
        ) : (
          <Empty title="The audit log is visible to admins and owners.">Ask a workspace admin if you need an entry from it.</Empty>
        )
      ) : (
        <WorkspaceTab />
      )}
    </>
  );
}
