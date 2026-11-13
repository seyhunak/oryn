"use client";
import Link from "next/link";
import { use, useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RotateCw, Square } from "lucide-react";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { useCan } from "@/lib/hooks";
import { useExecutionStream } from "@/lib/live";
import { dateTime, duration, shortId, time } from "@/lib/format";
import { PageHeader, Meta, Section } from "@/components/ui/page";
import { ErrorState, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { EvidenceLabel } from "@/components/ui/evidence-label";

const ACTIVE = ["queued", "running"];

export default function ExecutionPage({ params }: { params: Promise<{ eid: string }> }) {
  const { eid } = use(params);
  const qc = useQueryClient();
  const router = useRouter();
  const canRun = useCan("engineer");
  const [watched, setWatched] = useState(false);
  const detail = useQuery({ queryKey: ["execution", eid], queryFn: () => unwrap(api.GET("/api/v1/executions/{eid}", { params: { path: { eid } } })) });
  const exe = detail.data?.execution;
  const active = !!exe && ACTIVE.includes(exe.status);
  useExecutionStream(eid, active, () => setWatched(true));

  const cancel = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/executions/{eid}/cancel", { params: { path: { eid } } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["execution", eid] }),
  });
  const rerun = useMutation({
    mutationFn: (failed_only: boolean) => unwrap(api.POST("/api/v1/executions/{eid}/rerun", { params: { path: { eid } }, body: { failed_only } })),
    onSuccess: (e) => router.push(`/executions/${e.id}`),
  });

  if (detail.isPending) return <Skeleton className="h-40" />;
  if (detail.error || !exe) return <ErrorState error={detail.error} retry={() => detail.refetch()} />;
  const d = detail.data!;
  const finals = d.jobs.filter((j) => !j.superseded);
  const firstFailure = finals.find((j) => j.status === "failed" || j.status === "error");
  const workers = Array.from(new Set(finals.map((j) => j.worker_name).filter(Boolean)));

  return (
    <>
      <PageHeader
        crumbs={[{ href: "/executions", label: "Executions" }]}
        title={exe.title}
        description={
          <>
            {exe.project_name} on {exe.environment_name}, {exe.browser}. Started {dateTime(exe.started_at ?? exe.created_at)}.
          </>
        }
        actions={
          canRun ? (
            active ? (
              <Button variant="danger" size="lg" loading={cancel.isPending} onClick={() => cancel.mutate()}>
                <Square aria-hidden className="size-4" /> Cancel run
              </Button>
            ) : (
              <>
                {exe.failed + exe.errored > 0 ? (
                  <Button size="lg" loading={rerun.isPending && rerun.variables === true} onClick={() => rerun.mutate(true)}>
                    Re-run failures
                  </Button>
                ) : null}
                <Button variant="primary" size="lg" loading={rerun.isPending && rerun.variables === false} onClick={() => rerun.mutate(false)}>
                  <RotateCw aria-hidden className="size-4" /> Run again
                </Button>
              </>
            )
          ) : null
        }
      />
      {cancel.error || rerun.error ? <p role="alert" className="mb-4 text-sm text-error">{errorMessage(cancel.error ?? rerun.error)}</p> : null}
      <div aria-live="polite" className="visually-hidden">
        {exe.status === "running" ? `${exe.passed + exe.failed + exe.errored} of ${exe.total} finished` : `Execution ${exe.status}`}
      </div>

      <div className="grid grid-cols-1 gap-8 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="order-2 flex min-w-0 flex-col gap-8 min-[72rem]:order-1">
          {exe.status === "failed" || exe.status === "error" ? (
            <div className="flex flex-col gap-1 rounded-[var(--radius-card)] bg-ink p-4 text-paper">
              <p className="font-display text-[1.2rem] font-bold">
                {exe.failed + exe.errored} of {exe.total} scenarios did not pass.
              </p>
              {firstFailure ? (
                <p className="text-paper-3">
                  First: <Link className="text-paper" href={`/executions/${eid}/jobs/${firstFailure.id}`}>{firstFailure.scenario_name}</Link>
                  {firstFailure.error ? ` — ${firstFailure.error}` : ""}
                </p>
              ) : null}
            </div>
          ) : null}

          <Section title="Scenarios" description={`${exe.passed} passed, ${exe.failed} failed, ${exe.errored} errored, ${exe.cancelled} cancelled of ${exe.total}.`}>
            <Table>
              <thead>
                <tr>
                  <Th>Scenario</Th>
                  <Th>Status</Th>
                  <Th>Kind</Th>
                  <Th numeric>Attempt</Th>
                  <Th>Worker</Th>
                  <Th numeric>Duration</Th>
                </tr>
              </thead>
              <tbody>
                {finals.map((j) => (
                  <Tr key={j.id}>
                    <Td label="Scenario">
                      <Link href={`/executions/${eid}/jobs/${j.id}`} className="font-medium no-underline hover:underline">
                        {j.scenario_name}
                      </Link>
                      {j.error ? <div className="line-clamp-1 text-xs text-error">{j.error}</div> : null}
                    </Td>
                    <Td label="Status">
                      <StatusPill status={j.status} />
                    </Td>
                    <Td label="Kind">
                      <Tag>{j.kind}</Tag>
                    </Td>
                    <Td label="Attempt" numeric mono>
                      {j.attempt}/{j.max_attempts}
                    </Td>
                    <Td label="Worker" className="text-ink-2">
                      {j.worker_name ?? "–"}
                    </Td>
                    <Td label="Duration" numeric mono>
                      {duration(j.duration_ms)}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          </Section>

          {d.investigations.length ? (
            <Section title="Investigations" description="Deterministic diagnosis from recorded evidence. AI interpretation, when present, is labelled.">
              <ul className="flex flex-col gap-3">
                {d.investigations.map((i) => (
                  <li key={i.id} className="panel flex flex-col gap-1 p-4">
                    <div className="flex flex-wrap items-center gap-2">
                      <Tag>{i.category.replace(/_/g, " ")}</Tag>
                      <span className="mono text-xs text-muted">confidence {Math.round(i.confidence * 100)}%</span>
                    </div>
                    <p>{i.summary}</p>
                    <Link href={`/executions/${eid}/jobs/${i.job_id}#investigation`} className="text-sm">
                      Open evidence
                    </Link>
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}
        </div>

        <aside className="order-1 flex flex-col gap-6 min-[72rem]:order-2">
          <EvidenceLabel
            title={`Run ${shortId(exe.id)}`}
            lines={[
              [exe.environment_name, exe.browser],
              [time(exe.started_at ?? exe.created_at), duration(exe.duration_ms)],
              [`${exe.passed}/${exe.total} passed`, exe.trigger],
            ]}
            verified={active ? null : exe.status === "passed"}
            verifiedAt={time(exe.finished_at)}
            by={workers.join(", ") || null}
            failure={firstFailure ? `${firstFailure.scenario_name}: ${firstFailure.error ?? firstFailure.status}` : null}
            animate={watched}
          />
          <QueryState query={{ ...detail, data: d }} skeleton={null}>
            {() => (
              <Meta
                items={[
                  { label: "Status", value: <StatusPill status={exe.status} /> },
                  { label: "Project", value: <Link href={`/projects/${exe.project_id}`}>{exe.project_name}</Link> },
                  { label: "Triggered by", value: exe.triggered_by_name ?? exe.trigger },
                  { label: "Commit", value: exe.commit_sha ? <code>{exe.commit_sha.slice(0, 10)}</code> : "–" },
                  { label: "Branch", value: exe.branch ?? "–" },
                  { label: "Re-run of", value: exe.rerun_of ? <Link href={`/executions/${exe.rerun_of}`} className="mono">{shortId(exe.rerun_of)}</Link> : "–" },
                  { label: "Finished", value: dateTime(exe.finished_at) },
                  { label: "Artifacts", value: <span className="mono">{d.artifacts.length}</span> },
                ]}
              />
            )}
          </QueryState>
          {exe.rerun_of ? (
            <Link href={`/executions/compare?base=${exe.rerun_of}&head=${exe.id}`} className="text-sm">
              Compare with the original run
            </Link>
          ) : null}
        </aside>
      </div>
    </>
  );
}
