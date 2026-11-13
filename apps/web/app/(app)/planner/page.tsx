"use client";
import Link from "next/link";
import { useState } from "react";
import { useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, unwrap } from "@/lib/api/client";
import { keys, useCan, useCurrentProject } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { PageHeader } from "@/components/ui/page";
import { Empty, ErrorState, QueryState, TableSkeleton } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button, buttonClass } from "@/components/ui/button";
import { ProjectPicker } from "@/components/common/project-picker";
import { NewPlanDialog } from "@/components/planner/new-plan-dialog";
import { SourceTag } from "@/components/planner/source-tag";

export default function PlannerPage() {
  const { project, isPending, error, refetch } = useCurrentProject();
  const canWrite = useCan("engineer");
  const [creating, setCreating] = useState(false);
  const requirement = useSearchParams().get("requirement");
  const [dismissed, setDismissed] = useState(false);
  // A link from a requirement opens the dialog with that requirement preselected.
  const open = creating || (!!requirement && !dismissed && canWrite);
  const pid = project?.id ?? "";
  const plans = useQuery({
    queryKey: keys.project(pid, "plans"),
    enabled: !!pid,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/plans", { params: { path: { id: pid } } })),
  });

  return (
    <>
      <PageHeader
        title="AI Test Planner"
        description="Proposes scenarios from requirements and detected routes. Nothing runs and nothing is added to suites until a person approves the plan."
        actions={
          canWrite && pid ? (
            <Button variant="primary" size="lg" onClick={() => setCreating(true)}>
              <Plus aria-hidden className="size-4" /> New plan
            </Button>
          ) : null
        }
      />
      <div className="mb-4 flex flex-wrap items-center gap-4">
        <ProjectPicker />
      </div>
      {isPending ? (
        <TableSkeleton />
      ) : error ? (
        <ErrorState error={error} retry={() => refetch()} />
      ) : !project ? (
        <Empty title="No projects yet." action={<Link href="/projects" className={buttonClass()}>Create a project</Link>}>
          Plans belong to a project. Create one first.
        </Empty>
      ) : (
        <QueryState
          query={plans}
          empty={
            <Empty title="No plans yet." action={canWrite ? <Button onClick={() => setCreating(true)}>New plan</Button> : undefined}>
              A plan proposes scenarios for review. The heuristic planner works without any AI.
            </Empty>
          }
        >
          {(d) => (
            <Table>
              <thead>
                <tr>
                  <Th>Plan</Th>
                  <Th>Source</Th>
                  <Th>Status</Th>
                  <Th numeric>Items</Th>
                  <Th>Created</Th>
                </tr>
              </thead>
              <tbody>
                {d.map((p) => (
                  <Tr key={p.id}>
                    <Td label="Plan">
                      <Link href={`/planner/${p.id}?project=${pid}`} className="font-medium no-underline hover:underline">
                        {p.title}
                      </Link>
                      {p.objective ? <div className="line-clamp-1 text-xs text-ink-2">{p.objective}</div> : null}
                    </Td>
                    <Td label="Source">
                      <SourceTag source={p.source} />
                    </Td>
                    <Td label="Status">
                      <StatusPill status={p.status} label={p.status === "draft" ? "awaiting review" : undefined} />
                    </Td>
                    <Td label="Items" numeric mono>
                      {p.item_count}
                    </Td>
                    <Td label="Created" className="text-ink-2">
                      {relative(p.created_at)}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          )}
        </QueryState>
      )}
      {pid ? <NewPlanDialog
          projectId={pid}
          open={open}
          requirement={requirement}
          onOpenChange={(o) => {
            setCreating(o);
            if (!o) setDismissed(true);
          }}
        /> : null}
    </>
  );
}
