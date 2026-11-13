"use client";
import { useEffect } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useProjects, useWsId } from "@/lib/hooks";
import { PageHeader } from "@/components/ui/page";
import { Empty, ErrorState, TableSkeleton } from "@/components/ui/states";
import { UrlTabs, useTab } from "@/components/ui/tabs";
import { ProjectPicker } from "@/components/common/project-picker";
import { HealthTab } from "@/components/quality/health";
import { FailuresTab, FlakyTab } from "@/components/quality/flaky-failures";
import { RecommendationsTab } from "@/components/quality/recommendations";
import { RegressionTab } from "@/components/quality/regression";

const TABS = [
  { id: "health", label: "Health" },
  { id: "flaky", label: "Flaky" },
  { id: "failures", label: "Failures" },
  { id: "recommendations", label: "Recommendations" },
  { id: "regression", label: "Regression" },
];

export default function QualityPage() {
  const ws = useWsId();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const tab = useTab("health");
  const projects = useProjects();
  const param = sp.get("project");

  // Workspace-wide by default: make the default explicit in the URL so the picker and the data agree.
  useEffect(() => {
    if (!param) {
      const q = new URLSearchParams(sp.toString());
      q.set("project", "all");
      router.replace(`${path}?${q.toString()}`, { scroll: false });
    }
  }, [param, sp, router, path]);

  const list = projects.data ?? [];
  const project = param && param !== "all" ? (list.find((p) => p.id === param) ?? null) : null;
  const projectId = project?.id ?? null;
  const needsProject = tab === "recommendations" || tab === "regression";

  let body: React.ReactNode;
  if (projects.isPending || !param) body = <TableSkeleton />;
  else if (projects.error) body = <ErrorState error={projects.error} retry={() => projects.refetch()} />;
  else if (list.length === 0) body = <Empty title="No projects yet.">Quality figures appear after a project has recorded runs.</Empty>;
  else if (param !== "all" && !project) body = <Empty title="That project is not in this workspace.">Choose another project above.</Empty>;
  else if (needsProject && !projectId)
    body = (
      <Empty title="Choose a project.">
        {tab === "recommendations" ? "Recommendations are raised per project." : "Regression selection works on one project's scenarios and repository analysis."} Pick one in the project selector above.
      </Empty>
    );
  else if (tab === "flaky") body = <FlakyTab ws={ws} projectId={projectId} />;
  else if (tab === "failures") body = <FailuresTab ws={ws} projectId={projectId} />;
  else if (tab === "recommendations" && projectId) body = <RecommendationsTab projectId={projectId} />;
  else if (tab === "regression" && projectId) body = <RegressionTab key={projectId} projectId={projectId} />;
  else body = <HealthTab ws={ws} projectId={projectId} />;

  return (
    <>
      <PageHeader title="Quality Intelligence" description="Pass rate, flakiness, coverage and failures from recorded runs. Every figure links to how it is calculated." />
      <div className="mb-4 flex flex-wrap items-center gap-4">
        <ProjectPicker allowAll />
      </div>
      <UrlTabs tabs={TABS} />
      {body}
    </>
  );
}
