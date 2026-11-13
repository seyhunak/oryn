"use client";
import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { dateTime, duration, percent } from "@/lib/format";
import { Meta, Section, Stat } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { buttonClass } from "@/components/ui/button";
import type { ProjectDetail } from "./common";

const DAYS = 30;

export function OverviewTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const quality = useQuery({
    queryKey: keys.project(id, "quality", DAYS),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/quality", { params: { path: { id }, query: { days: DAYS } } })),
  });

  return (
    <div className="grid grid-cols-1 gap-10 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
      <div className="flex min-w-0 flex-col gap-10">
        <Section title={`Last ${DAYS} days`} description="Final attempts only. Every figure comes from recorded runs of this project.">
          <QueryState
            query={quality}
            skeleton={
              <div className="grid grid-cols-2 gap-6 md:grid-cols-3">
                {Array.from({ length: 6 }).map((_, i) => (
                  <Skeleton key={i} className="h-14" />
                ))}
              </div>
            }
          >
            {(q) => {
              const o = q.overview;
              const c = q.coverage;
              const attempts = o.jobs_passed + o.jobs_failed + o.jobs_errored;
              return (
                <div className="grid grid-cols-2 gap-x-6 gap-y-4 border-y border-rule py-4 md:grid-cols-3">
                  <Stat label="Pass rate" value={percent(o.pass_rate)} hint={`${o.jobs_passed} of ${attempts} final attempts`} href={`/quality?project=${id}`} />
                  <Stat label="Executions" value={o.executions} hint={o.executions_active ? `${o.executions_active} active now` : "none active"} href={`/executions?project=${id}`} />
                  <Stat label="Approved scenarios" value={o.scenarios_approved} hint={`of ${o.scenarios} in ${o.suites} suites`} href={`/studio?project=${id}`} />
                  <Stat label="Requirements covered" value={`${c.covered} of ${c.requirements}`} hint={c.coverage_rate == null ? "no requirements yet" : `${percent(c.coverage_rate)} linked to an approved scenario`} href={`/projects/${id}?tab=traceability`} />
                  <Stat label="Requirements verified" value={c.verified} hint={c.failing ? `${c.failing} with a failing scenario` : "none failing"} href={`/projects/${id}?tab=traceability`} />
                  <Stat label="Flaky scenarios" value={o.flaky_scenarios} hint={`mean scenario time ${duration(o.avg_scenario_ms)}`} href={`/quality?project=${id}&tab=flaky`} />
                </div>
              );
            }}
          </QueryState>
        </Section>

        <Section
          title="Scenarios with failures"
          description="Ordered by failed final attempts in the window."
          actions={
            <Link href={`/executions?project=${id}`} className={buttonClass("ghost", "sm")}>
              All executions
            </Link>
          }
        >
          <QueryState
            query={quality}
            isEmpty={(q) => q.scenarios.every((s) => s.failures === 0)}
            empty={<Empty title={`No failed scenarios in the last ${DAYS} days.`}>Failures recorded by executions of this project appear here.</Empty>}
          >
            {(q) => (
              <Table>
                <thead>
                  <tr>
                    <Th>Scenario</Th>
                    <Th>Suite</Th>
                    <Th numeric>Failed</Th>
                    <Th numeric>Runs</Th>
                    <Th>Latest</Th>
                  </tr>
                </thead>
                <tbody>
                  {[...q.scenarios]
                    .filter((s) => s.failures > 0)
                    .sort((a, b) => b.failures - a.failures)
                    .slice(0, 8)
                    .map((s) => (
                      <Tr key={s.scenario_id}>
                        <Td label="Scenario" className="max-w-[40ch]">
                          <span className="line-clamp-2">{s.scenario_name}</span>
                        </Td>
                        <Td label="Suite" className="text-ink-2">
                          {s.suite_name}
                        </Td>
                        <Td label="Failed" numeric mono>
                          {s.failures}
                        </Td>
                        <Td label="Runs" numeric mono>
                          {s.runs}
                        </Td>
                        <Td label="Latest">{s.last_status ? <StatusPill status={s.last_status} /> : <span className="text-muted">not run</span>}</Td>
                      </Tr>
                    ))}
                </tbody>
              </Table>
            )}
          </QueryState>
        </Section>
      </div>

      <aside className="order-first flex flex-col gap-6 min-[72rem]:order-none">
        <div className="panel flex flex-col gap-4 p-4">
          <h3>Project</h3>
          <Meta
            items={[
              { label: "Slug", value: <span className="mono">{project.slug}</span> },
              { label: "Visibility", value: project.visibility === "private" ? "Private" : "Workspace" },
              { label: "Your role", value: project.role },
              { label: "Created", value: <span className="mono">{dateTime(project.created_at)}</span> },
              { label: "Updated", value: <span className="mono">{dateTime(project.updated_at)}</span> },
            ]}
          />
        </div>
        <nav aria-label="Project shortcuts" className="flex flex-col gap-2">
          <Link href={`/studio?project=${id}`} className={buttonClass("secondary", "md", "justify-start")}>
            Open in Test Studio
          </Link>
          <Link href={`/executions?project=${id}`} className={buttonClass("secondary", "md", "justify-start")}>
            Executions of this project
          </Link>
          <Link href={`/planner?project=${id}`} className={buttonClass("secondary", "md", "justify-start")}>
            Plan tests with the AI Test Planner
          </Link>
        </nav>
      </aside>
    </div>
  );
}
