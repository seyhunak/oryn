"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Circle, Play, Plus } from "lucide-react";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { keys, useCan, useCurrentProject } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { PageHeader } from "@/components/ui/page";
import { Empty, ErrorState, QueryState, TableSkeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { UrlTabs, useTab } from "@/components/ui/tabs";
import { Button, buttonClass } from "@/components/ui/button";
import { ProjectPicker } from "@/components/common/project-picker";
import { RunDialog } from "@/components/studio/run-dialog";
import { RecordDialog, RecordingsSection } from "@/components/studio/recordings";
import { StepDefinitionsSection } from "@/components/studio/step-definitions";

export default function StudioPage() {
  const { project, isPending, error, refetch } = useCurrentProject();
  const canWrite = useCan("engineer");
  const tab = useTab("suites");
  const [recording, setRecording] = useState(false);
  const pid = project?.id ?? "";
  const router = useRouter();
  const scenarioId = useSearchParams().get("scenario");
  // `/studio?scenario=<id>` opens that scenario's suite with the scenario selected.
  const linked = useQuery({
    queryKey: keys.project(pid, "scenario", scenarioId),
    enabled: !!pid && !!scenarioId,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/scenarios/{scenario_id}", { params: { path: { id: pid, scenario_id: scenarioId! } } })),
  });
  const target = linked.data?.scenario.suite_id;
  useEffect(() => {
    if (target && scenarioId) router.replace(`/studio/${target}?project=${pid}&scenario=${scenarioId}`);
  }, [target, scenarioId, pid, router]);

  return (
    <>
      <PageHeader
        title="Test Studio"
        description="Gherkin suites for this project. Scenarios run only once a person has approved them."
        actions={
          canWrite && pid ? (
            <>
              <Button size="lg" onClick={() => setRecording(true)}>
                <Circle aria-hidden className="size-4" /> Record
              </Button>
              <Link href={`/studio/new?project=${pid}`} className={buttonClass("primary", "lg")}>
                <Plus aria-hidden className="size-4" /> New suite
              </Link>
            </>
          ) : null
        }
      />
      <div className="mb-4 flex flex-wrap items-center gap-4">
        <ProjectPicker />
      </div>
      {scenarioId && linked.error ? (
        <p role="alert" className="mb-4 text-sm text-error">
          The linked scenario could not be opened: <code>{errorMessage(linked.error)}</code>
        </p>
      ) : null}
      {isPending ? (
        <TableSkeleton />
      ) : error ? (
        <ErrorState error={error} retry={() => refetch()} />
      ) : !project ? (
        <Empty title="No projects yet." action={<Link href="/projects" className={buttonClass()}>Create a project</Link>}>
          Suites belong to a project. Create one first.
        </Empty>
      ) : (
        <>
          <UrlTabs
            tabs={[
              { id: "suites", label: "Suites" },
              { id: "recordings", label: "Recordings" },
              { id: "steps", label: "Custom steps" },
            ]}
          />
          {tab === "recordings" ? (
            <RecordingsSection projectId={pid} onRecord={() => setRecording(true)} />
          ) : tab === "steps" ? (
            <StepDefinitionsSection projectId={pid} />
          ) : (
            <SuitesTable projectId={pid} />
          )}
          <RecordDialog projectId={pid} open={recording} onOpenChange={setRecording} />
        </>
      )}
    </>
  );
}

function SuitesTable({ projectId }: { projectId: string }) {
  const canRun = useCan("engineer");
  const [running, setRunning] = useState<{ id: string; name: string } | null>(null);
  const suites = useQuery({
    queryKey: keys.project(projectId, "suites"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/suites", { params: { path: { id: projectId } } })),
  });
  return (
    <>
      <QueryState
        query={suites}
        empty={
          <Empty title="No suites yet." action={canRun ? <Link href={`/studio/new?project=${projectId}`} className={buttonClass()}>New suite</Link> : undefined}>
            Write a feature in Gherkin, record a flow, or approve a plan from the AI Test Planner.
          </Empty>
        }
      >
        {(d) => (
          <Table>
            <thead>
              <tr>
                <Th>Suite</Th>
                <Th>Tags</Th>
                <Th numeric>Scenarios</Th>
                <Th numeric>Approved</Th>
                <Th>Last run</Th>
                <Th>Updated</Th>
                {canRun ? <Th className="w-0" /> : null}
              </tr>
            </thead>
            <tbody>
              {d.map((s) => (
                <Tr key={s.id}>
                  <Td label="Suite">
                    <Link href={`/studio/${s.id}?project=${projectId}`} className="font-medium no-underline hover:underline">
                      {s.name}
                    </Link>
                    {s.path ? <div className="mono text-xs text-muted">{s.path}</div> : null}
                    {s.parse_error ? <div className="mono line-clamp-2 text-xs text-error">{s.parse_error}</div> : null}
                  </Td>
                  <Td label="Tags">
                    <div className="flex flex-wrap gap-1">
                      {s.tags.length ? s.tags.map((t) => <Tag key={t}>@{t}</Tag>) : <span className="text-muted">none</span>}
                    </div>
                  </Td>
                  <Td label="Scenarios" numeric mono>
                    {s.scenario_count}
                  </Td>
                  <Td label="Approved" numeric mono className={s.approved_count < s.scenario_count ? "text-ink-2" : undefined}>
                    {s.approved_count}/{s.scenario_count}
                  </Td>
                  <Td label="Last run">{s.last_status ? <StatusPill status={s.last_status} /> : <StatusPill status="not_run" />}</Td>
                  <Td label="Updated" className="text-ink-2">
                    {relative(s.updated_at)}
                  </Td>
                  {canRun ? (
                    <Td>
                      <Button size="sm" onClick={() => setRunning({ id: s.id, name: s.name })} aria-label={`Run ${s.name}`}>
                        <Play aria-hidden className="size-3.5" /> Run
                      </Button>
                    </Td>
                  ) : null}
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
      <RunDialog projectId={projectId} suite={running} open={!!running} onOpenChange={(o) => !o && setRunning(null)} />
    </>
  );
}
