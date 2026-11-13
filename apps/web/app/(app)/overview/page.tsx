"use client";
import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys, useProjects, useWorkspace } from "@/lib/hooks";
import { duration, percent, relative, shortId } from "@/lib/format";
import { PageHeader, Section, Stat } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { TrendChart } from "@/components/common/trend-chart";
import { buttonClass } from "@/components/ui/button";

export default function OverviewPage() {
  const { ws } = useWorkspace();
  const id = ws?.id ?? "";
  const overview = useQuery({ queryKey: keys.ws(id, "overview"), enabled: !!id, queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/overview", { params: { path: { ws: id }, query: { days: 30 } } })) });
  const trends = useQuery({ queryKey: keys.ws(id, "trends", 14), enabled: !!id, queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/trends", { params: { path: { ws: id }, query: { days: 14 } } })) });
  const runs = useQuery({
    queryKey: keys.ws(id, "executions", "recent"),
    enabled: !!id,
    refetchInterval: 10_000,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/executions", { params: { path: { ws: id }, query: { per_page: 8 } } })),
  });
  const failures = useQuery({ queryKey: keys.ws(id, "failures"), enabled: !!id, queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/failures", { params: { path: { ws: id }, query: { limit: 6 } } })) });
  const projects = useProjects();

  if (projects.data && projects.data.length === 0) {
    return (
      <>
        <PageHeader title="Overview" description={`Quality across ${ws?.name ?? "this workspace"}.`} />
        <Empty
          title="No projects yet."
          action={
            <Link href="/projects?new=1" className={buttonClass("primary")}>
              Create a project
            </Link>
          }
        >
          A project holds the application under test: its repository, requirements, environments and suites. Metrics appear here after the first run.
        </Empty>
      </>
    );
  }

  return (
    <>
      <PageHeader title="Overview" description="The last 30 days across every project you can see. Every figure comes from recorded runs." />
      <div className="flex flex-col gap-10">
        <QueryState
          query={overview}
          skeleton={
            <div className="grid grid-cols-2 gap-6 md:grid-cols-5">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-14" />
              ))}
            </div>
          }
        >
          {(o) => (
            <div className="grid grid-cols-2 gap-x-6 gap-y-4 border-y border-rule py-4 md:grid-cols-5">
              <Stat label="Pass rate" value={percent(o.pass_rate)} hint={`${o.jobs_passed} of ${o.jobs_passed + o.jobs_failed + o.jobs_errored} final attempts`} href="/quality" />
              <Stat label="Executions" value={o.executions} hint={o.executions_active ? `${o.executions_active} active now` : "none active"} href="/executions" />
              <Stat label="Approved scenarios" value={o.scenarios_approved} hint={`of ${o.scenarios} in ${o.suites} suites`} href="/studio" />
              <Stat label="Flaky scenarios" value={o.flaky_scenarios} hint="passed on retry or flipped" href="/quality?tab=flaky" />
              <Stat label="Mean scenario time" value={duration(o.avg_scenario_ms)} hint={`runs average ${duration(o.avg_execution_ms)}`} />
            </div>
          )}
        </QueryState>

        <div className="grid grid-cols-1 gap-10 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
          <Section title="Recent executions" actions={<Link href="/executions" className={buttonClass("ghost", "sm")}>All executions</Link>}>
            <QueryState query={runs} isEmpty={(d) => d.items.length === 0} empty={<Empty title="No executions yet.">Approve scenarios in Test Studio, then run them.</Empty>}>
              {(d) => (
                <Table>
                  <thead>
                    <tr>
                      <Th>Run</Th>
                      <Th>Status</Th>
                      <Th numeric>Passed</Th>
                      <Th numeric>Duration</Th>
                      <Th>Started</Th>
                    </tr>
                  </thead>
                  <tbody>
                    {d.items.map((e) => (
                      <Tr key={e.id}>
                        <Td label="Run">
                          <Link href={`/executions/${e.id}`} className="font-medium no-underline hover:underline">
                            {e.title}
                          </Link>
                          <span className="mono ml-2 text-xs text-muted">{shortId(e.id)}</span>
                          <div className="text-xs text-muted">{e.project_name}</div>
                        </Td>
                        <Td label="Status">
                          <StatusPill status={e.status} />
                        </Td>
                        <Td label="Passed" numeric mono>
                          {e.passed}/{e.total}
                        </Td>
                        <Td label="Duration" numeric mono>
                          {duration(e.duration_ms)}
                        </Td>
                        <Td label="Started" className="text-ink-2">
                          {relative(e.started_at ?? e.created_at)}
                        </Td>
                      </Tr>
                    ))}
                  </tbody>
                </Table>
              )}
            </QueryState>
          </Section>

          <Section title="Latest failures" actions={<Link href="/quality?tab=failures" className={buttonClass("ghost", "sm")}>All failures</Link>}>
            <QueryState query={failures} empty={<Empty title="No failures recorded." />}>
              {(list) => (
                <ul className="flex flex-col">
                  {list.map((f) => (
                    <li key={f.job_id} className="flex flex-col gap-1 border-b border-rule py-3">
                      <div className="flex items-center justify-between gap-3">
                        <Link href={`/executions/${f.execution_id}/jobs/${f.job_id}`} className="min-w-0 truncate font-medium no-underline hover:underline">
                          {f.scenario_name}
                        </Link>
                        <StatusPill status={f.status} />
                      </div>
                      {f.message ? <code className="line-clamp-2 text-xs break-words text-ink-2">{f.message}</code> : null}
                      <span className="text-xs text-muted">
                        {f.project_name}, {relative(f.finished_at)}
                        {f.investigated ? ", investigated" : ""}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </QueryState>
          </Section>
        </div>

        <Section title="Outcomes per day" description="Final attempts only; retried attempts are not double-counted.">
          <QueryState query={trends} isEmpty={(d) => d.every((p) => p.passed + p.failed + p.errored === 0)} empty={<Empty title="No finished jobs in the last 14 days." />} skeleton={<Skeleton className="h-56" />}>
            {(d) => <TrendChart data={d} />}
          </QueryState>
        </Section>
      </div>
    </>
  );
}
