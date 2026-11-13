"use client";
import { useCurrentProject } from "@/lib/hooks";
import { Empty, ErrorState, Skeleton } from "@/components/ui/states";
import { SuiteEditor } from "@/components/studio/suite-editor";

export default function NewSuitePage() {
  const { project, isPending, error, refetch } = useCurrentProject();
  if (isPending) return <Skeleton className="h-[520px]" />;
  if (error) return <ErrorState error={error} retry={() => refetch()} />;
  if (!project) return <Empty title="No projects yet.">Suites belong to a project. Create one under Projects first.</Empty>;
  return <SuiteEditor projectId={project.id} suiteId={null} />;
}
