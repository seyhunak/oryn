"use client";
import Link from "next/link";
import { useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { ArrowRight } from "lucide-react";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCurrentProject, useWsId } from "@/lib/hooks";
import { dateTime, duration, shortId } from "@/lib/format";
import { PageHeader } from "@/components/ui/page";
import { Empty, ErrorState, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Select } from "@/components/ui/field";
import { ProjectPicker } from "@/components/common/project-picker";

type Change = "fixed" | "regressed" | "still failing" | "unchanged" | "only in base" | "only in head";
const BAD = ["failed", "error", "timed_out"];

function changeOf(r: Schemas["ComparisonRow"]): Change {
  if (!r.base_status) return "only in head";
  if (!r.head_status) return "only in base";
  const b = BAD.includes(r.base_status);
  const h = BAD.includes(r.head_status);
  if (b && r.head_status === "passed") return "fixed";
  if (r.base_status === "passed" && h) return "regressed";
  if (b && h) return "still failing";
  return "unchanged";
}

function ChangeLabel({ c }: { c: Change }) {
  if (c === "fixed") return <StatusPill status="passed" label="fixed" />;
  if (c === "regressed") return <StatusPill status="failed" label="regressed" />;
  if (c === "still failing") return <StatusPill status="failed" label="still failing" />;
  if (c === "unchanged") return <span className="text-sm text-muted">unchanged</span>;
  return <Tag>{c}</Tag>;
}

function useExecution(eid: string | null) {
  return useQuery({
    queryKey: ["execution", eid],
    enabled: !!eid,
    queryFn: () => unwrap(api.GET("/api/v1/executions/{eid}", { params: { path: { eid: eid! } } })),
  });
}

/** Choose base and head from recent executions of one project. */
function Chooser({ base, head, projectId }: { base: string | null; head: string | null; projectId: string | null }) {
  const ws = useWsId();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const [b, setB] = useState(base ?? "");
  const [h, setH] = useState(head ?? "");
  const runs = useQuery({
    queryKey: keys.ws(ws, "executions", { project_id: projectId, per_page: 50, compare: true }),
    enabled: !!ws && !!projectId,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/executions", { params: { path: { ws }, query: { project_id: projectId ?? undefined, per_page: 50 } } })),
  });
  const items = runs.data?.items ?? [];
  const label = (e: Schemas["Execution"]) => `${e.title} · ${shortId(e.id)} · ${e.status} · ${dateTime(e.started_at ?? e.created_at)}`;
  if (runs.isPending && projectId) return <Skeleton className="h-16" />;
  if (runs.error) return <ErrorState error={runs.error} retry={() => runs.refetch()} />;
  if (projectId && items.length < 2) return <Empty title="This project has fewer than two executions.">Run a suite twice to compare outcomes between runs.</Empty>;
  return (
    <form
      className="flex flex-wrap items-end gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        const q = new URLSearchParams(sp.toString());
        q.set("base", b);
        q.set("head", h);
        router.push(`${path}?${q.toString()}`);
      }}
    >
      <Field label="Base (earlier run)" className="w-full max-w-md">
        {(id) => (
          <Select id={id} value={b} onChange={(e) => setB(e.target.value)} required>
            <option value="">Choose an execution</option>
            {items.map((e) => (
              <option key={e.id} value={e.id}>
                {label(e)}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <Field label="Head (later run)" className="w-full max-w-md">
        {(id) => (
          <Select id={id} value={h} onChange={(e) => setH(e.target.value)} required>
            <option value="">Choose an execution</option>
            {items.map((e) => (
              <option key={e.id} value={e.id}>
                {label(e)}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <div className="flex min-h-[calc(36px+1lh+4px)] flex-col">
        <Button type="submit" variant={base && head ? "secondary" : "primary"} disabled={!b || !h || b === h}>
          Compare
        </Button>
      </div>
    </form>
  );
}

function RunHeader({ q, role }: { q: ReturnType<typeof useExecution>; role: string }) {
  if (q.isPending) return <Skeleton className="h-12 w-64" />;
  if (q.error || !q.data) return <span className="text-sm text-error">{role} could not be loaded.</span>;
  const e = q.data.execution;
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-xs text-muted">{role}</span>
      <Link href={`/executions/${e.id}`} className="font-medium no-underline hover:underline">
        {e.title} <span className="mono text-xs text-muted">{shortId(e.id)}</span>
      </Link>
      <span className="flex flex-wrap items-center gap-2 text-xs text-ink-2">
        <StatusPill status={e.status} />
        <span className="mono">{dateTime(e.started_at ?? e.created_at)}</span>
        <span>
          {e.environment_name}, {e.browser}
        </span>
      </span>
    </div>
  );
}

function StatusCell({ status, ms, eid, job }: { status?: string | null; ms?: number | null; eid: string; job?: string | null }) {
  if (!status) return <span className="text-muted">not run</span>;
  const pill = <StatusPill status={status} />;
  return (
    <span className="inline-flex items-center gap-2">
      {job ? (
        <Link href={`/executions/${eid}/jobs/${job}`} className="no-underline" aria-label={`Open evidence, ${status}`}>
          {pill}
        </Link>
      ) : (
        pill
      )}
      <span className="mono text-xs text-ink-2 tabular-nums">{duration(ms)}</span>
    </span>
  );
}

export default function ComparePage() {
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const base = sp.get("base");
  const head = sp.get("head");
  const changedOnly = sp.get("changed") === "1";
  const baseQ = useExecution(base);
  const headQ = useExecution(head);
  const { project } = useCurrentProject();
  const projectId = baseQ.data?.execution.project_id ?? headQ.data?.execution.project_id ?? project?.id ?? null;
  const ready = !!base && !!head;
  const rows = useQuery({
    queryKey: ["executions", "compare", base, head],
    enabled: ready,
    queryFn: () => unwrap(api.GET("/api/v1/executions/compare", { params: { query: { base: base!, head: head! } } })),
  });

  const setChanged = (on: boolean) => {
    const q = new URLSearchParams(sp.toString());
    if (on) q.set("changed", "1");
    else q.delete("changed");
    router.replace(`${path}?${q.toString()}`, { scroll: false });
  };

  return (
    <>
      <PageHeader crumbs={[{ href: "/executions", label: "Executions" }]} title="Compare executions" description="Final outcome of each scenario in two runs, side by side. Open a status to see that attempt's evidence." />
      <div className="flex flex-col gap-6">
        {ready ? (
          <div className="flex flex-col items-start gap-4 border-y border-rule py-4 md:flex-row md:items-center">
            <RunHeader q={baseQ} role="Base" />
            <ArrowRight aria-hidden className="hidden size-4 shrink-0 text-muted md:block" />
            <RunHeader q={headQ} role="Head" />
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-4">
            <ProjectPicker />
          </div>
        )}

        <details className="group" open={!ready}>
          <summary className="cursor-pointer text-sm font-medium text-ink-2 hover:text-ink">{ready ? "Change the runs being compared" : "Choose two runs"}</summary>
          <div className="pt-4">
            <Chooser key={`${projectId}-${base}-${head}`} base={base} head={head} projectId={projectId} />
          </div>
        </details>

        {ready ? (
          <QueryState query={rows} empty={<Empty title="These runs have no scenarios in common or no jobs.">Pick two runs of the same suite.</Empty>}>
            {(list) => {
              const withChange = list.map((r) => ({ r, c: changeOf(r) }));
              const counts = withChange.reduce<Record<string, number>>((acc, x) => ((acc[x.c] = (acc[x.c] ?? 0) + 1), acc), {});
              const shown = changedOnly ? withChange.filter((x) => x.c !== "unchanged") : withChange;
              const order: Change[] = ["regressed", "still failing", "fixed", "only in head", "only in base", "unchanged"];
              shown.sort((a, b) => order.indexOf(a.c) - order.indexOf(b.c));
              return (
                <div className="flex flex-col gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-4">
                    <p className="text-sm text-ink-2 tabular-nums">
                      {list.length} scenarios: {order.filter((c) => counts[c]).map((c) => `${counts[c]} ${c}`).join(", ")}.
                    </p>
                    <Checkbox label="Show changes only" checked={changedOnly} onChange={(e) => setChanged(e.target.checked)} />
                  </div>
                  {shown.length === 0 ? (
                    <Empty title="No scenario changed outcome between these runs." />
                  ) : (
                    <Table>
                      <thead>
                        <tr>
                          <Th>Scenario</Th>
                          <Th>Base</Th>
                          <Th>Head</Th>
                          <Th>Change</Th>
                        </tr>
                      </thead>
                      <tbody>
                        {shown.map(({ r, c }) => (
                          <Tr key={r.scenario_id ?? r.scenario_name}>
                            <Td label="Scenario" className="max-w-[28rem]">
                              {r.scenario_name}
                            </Td>
                            <Td label="Base">
                              <StatusCell status={r.base_status} ms={r.base_duration_ms} eid={base!} job={r.base_job_id} />
                            </Td>
                            <Td label="Head">
                              <StatusCell status={r.head_status} ms={r.head_duration_ms} eid={head!} job={r.head_job_id} />
                            </Td>
                            <Td label="Change">
                              <ChangeLabel c={c} />
                            </Td>
                          </Tr>
                        ))}
                      </tbody>
                    </Table>
                  )}
                </div>
              );
            }}
          </QueryState>
        ) : null}
      </div>
    </>
  );
}
