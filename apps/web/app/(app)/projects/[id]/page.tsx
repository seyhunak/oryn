"use client";
import { use } from "react";
import { PageHeader } from "@/components/ui/page";
import { ErrorState, Skeleton } from "@/components/ui/states";
import { Tag } from "@/components/ui/status";
import { UrlTabs, useTab } from "@/components/ui/tabs";
import { ProjectChip, useProjectDetail } from "@/components/projects/common";
import { OverviewTab } from "@/components/projects/overview-tab";
import { RequirementsTab } from "@/components/projects/requirements-tab";
import { TraceabilityTab } from "@/components/projects/traceability-tab";
import { RepositoryTab } from "@/components/projects/repository-tab";
import { EnvironmentsTab } from "@/components/projects/environments-tab";
import { TestDataTab } from "@/components/projects/test-data-tab";
import { AccessTab } from "@/components/projects/access-tab";
import { SettingsTab } from "@/components/projects/settings-tab";

const TABS = [
  { id: "overview", label: "Overview" },
  { id: "requirements", label: "Requirements" },
  { id: "traceability", label: "Traceability" },
  { id: "repository", label: "Repository" },
  { id: "environments", label: "Environments" },
  { id: "test-data", label: "Test data" },
  { id: "access", label: "Access" },
  { id: "settings", label: "Settings" },
];

export default function ProjectPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  const detail = useProjectDetail(id);
  const tab = useTab("overview");
  const crumbs = [{ href: "/projects", label: "Projects" }];

  if (detail.isPending) {
    return (
      <div className="flex flex-col gap-4">
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-8 w-72" />
        <Skeleton className="h-4 w-96" />
        <Skeleton className="mt-6 h-40" />
      </div>
    );
  }
  if (detail.error || !detail.data) {
    return (
      <>
        <PageHeader title="Project" crumbs={crumbs} />
        <ErrorState error={detail.error} retry={() => detail.refetch()} />
      </>
    );
  }
  const p = detail.data;

  return (
    <>
      <PageHeader
        crumbs={crumbs}
        title={
          <span className="inline-flex items-center gap-3">
            <ProjectChip name={p.name} color={p.color} size="lg" />
            {p.name}
          </span>
        }
        description={p.description || undefined}
        actions={
          <>
            <Tag>{p.visibility === "private" ? "Private" : "Workspace"}</Tag>
            <Tag>Your role: {p.role}</Tag>
          </>
        }
      />
      <UrlTabs tabs={TABS} />
      {tab === "requirements" ? (
        <RequirementsTab project={p} />
      ) : tab === "traceability" ? (
        <TraceabilityTab project={p} />
      ) : tab === "repository" ? (
        <RepositoryTab project={p} />
      ) : tab === "environments" ? (
        <EnvironmentsTab project={p} />
      ) : tab === "test-data" ? (
        <TestDataTab project={p} />
      ) : tab === "access" ? (
        <AccessTab project={p} />
      ) : tab === "settings" ? (
        <SettingsTab project={p} />
      ) : (
        <OverviewTab project={p} />
      )}
    </>
  );
}
