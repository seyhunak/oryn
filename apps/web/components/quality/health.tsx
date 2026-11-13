"use client";
import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { duration, percent } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { buttonClass } from "@/components/ui/button";
import { TrendChart } from "@/components/common/trend-chart";
import { MetricFigure, MetricLink } from "./metric";

const DAYS = 30;

function FiguresSkeleton() {
  return (
    <div className="grid grid-cols-2 gap-6 md:grid-cols-5">
      {Array.from({ length: 5 }).map((_, i) => (
        <Skeleton key={i} className="h-20" />
      ))}
    </div>
  );
}

function OverviewFigures({ o, coverage, project }: { o: Schemas["Overview"]; coverage?: Schemas["RequirementCoverage"]; project: string }) {
  const attempts = o.jobs_passed + o.jobs_failed + o.jobs_errored;
  return (
    <div className="grid grid-cols-2 gap-x-6 gap-y-4 border-y border-rule py-4 md:grid-cols-5">
      <MetricFigure label="Pass rate" value={percent(o.pass_rate)} hint={`${o.jobs_passed} of ${attempts} final attempts`} anchor="pass-rate" />
      <MetricFigure label="Failure rate" value={percent(o.failure_rate)} hint={`${o.jobs_failed} failed, ${o.jobs_errored} errored`} anchor="failure-rate" />
      <MetricFigure label="Flaky scenarios" value={o.flaky_scenarios} hint={<Link href={`?tab=flaky&project=${project}`}>See flaky scenarios</Link>} anchor="flakiness" />
      {coverage ? (
        <MetricFigure label="Requirement coverage" value={percent(coverage.coverage_rate)} hint={`${coverage.covered} of ${coverage.requirements} requirements, ${coverage.verified} verified`} anchor="coverage" />
      ) : (
        <MetricFigure label="Executions" value={o.executions} hint={o.executions_active ? `${o.executions_active} active now` : "none active"} />
      )}
      <MetricFigure label="Mean scenario time" value={duration(o.avg_scenario_ms)} hint={`runs average ${duration(o.avg_execution_ms)}`} anchor="mean-duration" />
    </div>
  );
}

function Trends({ ws, projectId }: { ws: string; projectId: string | null }) {
  const trends = useQuery({
    queryKey: keys.ws(ws, "trends", DAYS, projectId ?? "all"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/trends", { params: { path: { ws }, query: { days: DAYS, project_id: projectId ?? undefined } } })),
  });
  return (
    <Section title="Outcomes per day" description={<>Final attempts over the last {DAYS} days; retried attempts are not double-counted. <MetricLink anchor="pass-rate" /></>}>
      <QueryState query={trends} isEmpty={(d) => d.every((p) => p.passed + p.failed + p.errored === 0)} empty={<Empty title={`No finished jobs in the last ${DAYS} days.`}>Run a suite from Test Studio to start the series.</Empty>} skeleton={<Skeleton className="h-56" />}>
        {(d) => <TrendChart data={d} />}
      </QueryState>
    </Section>
  );
}

/** Workspace-wide health: overview figures and the daily outcome series. */
function WorkspaceHealth({ ws }: { ws: string }) {
  const overview = useQuery({
    queryKey: keys.ws(ws, "overview", DAYS),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/overview", { params: { path: { ws }, query: { days: DAYS } } })),
  });
  return (
    <div className="flex flex-col gap-10">
      <QueryState query={overview} skeleton={<FiguresSkeleton />}>
        {(o) => (
          <div className="flex flex-col gap-2">
            <OverviewFigures o={o} project="all" />
            <p className="text-xs text-muted">
              Last {o.window_days} days across {o.projects} projects, {o.scenarios} scenarios. Choose a project for per-scenario statistics, requirement coverage and recommendations.
            </p>
          </div>
        )}
      </QueryState>
      <Trends ws={ws} projectId={null} />
    </div>
  );
}

function ProjectHealth({ ws, projectId }: { ws: string; projectId: string }) {
  const quality = useQuery({
    queryKey: keys.project(projectId, "quality", DAYS),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/quality", { params: { path: { id: projectId }, query: { days: DAYS } } })),
  });
  return (
    <div className="flex flex-col gap-10">
      <QueryState query={quality} skeleton={<FiguresSkeleton />}>
        {(q) => <OverviewFigures o={q.overview} coverage={q.coverage} project={projectId} />}
      </QueryState>

      <Trends ws={ws} projectId={projectId} />

      {quality.data ? (
        <>
          <CoverageTable c={quality.data.coverage} />
          <ScenarioTable rows={quality.data.scenarios} />
          {quality.data.recommendations.length > 0 ? (
            <Section title="Open recommendations" actions={<Link href={`?tab=recommendations&project=${projectId}`} className={buttonClass("ghost", "sm")}>All recommendations</Link>}>
              <ul className="flex flex-col">
                {quality.data.recommendations.slice(0, 3).map((r) => (
                  <li key={r.id} className="border-b border-rule py-3">
                    <p className="font-medium">{r.title}</p>
                    <p className="line-clamp-2 text-sm text-ink-2">{r.body}</p>
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}
        </>
      ) : null}
    </div>
  );
}

function CoverageTable({ c }: { c: Schemas["RequirementCoverage"] }) {
  return (
    <Section
      title="Requirement coverage"
      description={
        <>
          {c.covered} of {c.requirements} requirements are linked to an approved, executable scenario; {c.verified} have every linked scenario passing on its latest run, {c.failing} have at least one failing. <MetricLink anchor="coverage" />
        </>
      }
    >
      {c.rows.length === 0 ? (
        <Empty title="No requirements in this project.">Add requirements in the project and link scenarios to them to measure coverage.</Empty>
      ) : (
        <Table>
          <thead>
            <tr>
              <Th>Requirement</Th>
              <Th>Priority</Th>
              <Th numeric>Scenarios</Th>
              <Th numeric>Executable</Th>
              <Th numeric>Passing</Th>
              <Th numeric>Failing</Th>
              <Th numeric>Never run</Th>
            </tr>
          </thead>
          <tbody>
            {c.rows.map((r) => (
              <Tr key={r.requirement_id}>
                <Td label="Requirement">
                  <span className="mono mr-2 text-xs text-muted">{r.key}</span>
                  {r.title}
                </Td>
                <Td label="Priority" className="text-ink-2">
                  {r.priority}
                </Td>
                <Td label="Scenarios" numeric mono>
                  {r.scenarios}
                </Td>
                <Td label="Executable" numeric mono>
                  {r.executable_scenarios}
                </Td>
                <Td label="Passing" numeric mono>
                  {r.passing}
                </Td>
                <Td label="Failing" numeric mono className={r.failing ? "text-error" : undefined}>
                  {r.failing}
                </Td>
                <Td label="Never run" numeric mono className="text-ink-2">
                  {r.never_run}
                </Td>
              </Tr>
            ))}
          </tbody>
        </Table>
      )}
    </Section>
  );
}

function ScenarioTable({ rows }: { rows: Schemas["ScenarioStat"][] }) {
  return (
    <Section
      title="Scenarios"
      description={
        <>
          Final-attempt outcomes per scenario over the last {DAYS} days. <MetricLink anchor="pass-rate" /> <MetricLink anchor="mean-duration" />
        </>
      }
    >
      {rows.length === 0 ? (
        <Empty title="No scenario has run in this window.">Run a suite from Test Studio to collect statistics.</Empty>
      ) : (
        <Table>
          <thead>
            <tr>
              <Th>Scenario</Th>
              <Th>Suite</Th>
              <Th numeric>Runs</Th>
              <Th numeric>Passes</Th>
              <Th numeric>Failures</Th>
              <Th numeric>Pass rate</Th>
              <Th numeric>Mean duration</Th>
              <Th>Last status</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => (
              <Tr key={s.scenario_id}>
                <Td label="Scenario" className="max-w-[28rem]">
                  {s.scenario_name}
                </Td>
                <Td label="Suite" className="text-ink-2">
                  {s.suite_name}
                </Td>
                <Td label="Runs" numeric mono>
                  {s.runs}
                </Td>
                <Td label="Passes" numeric mono>
                  {s.passes}
                </Td>
                <Td label="Failures" numeric mono className={s.failures ? "text-error" : undefined}>
                  {s.failures}
                </Td>
                <Td label="Pass rate" numeric mono>
                  {s.runs ? percent(s.passes / s.runs, 0) : "–"}
                </Td>
                <Td label="Mean duration" numeric mono>
                  {duration(s.avg_duration_ms)}
                </Td>
                <Td label="Last status">{s.last_status ? <StatusPill status={s.last_status} /> : <span className="text-muted">not run</span>}</Td>
              </Tr>
            ))}
          </tbody>
        </Table>
      )}
    </Section>
  );
}

export function HealthTab({ ws, projectId }: { ws: string; projectId: string | null }) {
  return projectId ? <ProjectHealth ws={ws} projectId={projectId} /> : <WorkspaceHealth ws={ws} />;
}
