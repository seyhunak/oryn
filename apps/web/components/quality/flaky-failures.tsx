"use client";
import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { dateTime, percent, relative, shortId } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { MetricLink } from "./metric";

export function FlakyTab({ ws, projectId }: { ws: string; projectId: string | null }) {
  const flaky = useQuery({
    queryKey: keys.ws(ws, "flaky", projectId ?? "all"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/flaky", { params: { path: { ws }, query: { project_id: projectId ?? undefined } } })),
  });
  return (
    <Section
      title="Flaky scenarios"
      description={
        <>
          Scenarios whose final outcome flipped between consecutive runs, or that failed and then passed on retry in the same execution. <MetricLink anchor="flakiness" />
        </>
      }
    >
      <QueryState query={flaky} empty={<Empty title="No flaky scenarios in this window.">A scenario appears here when its outcome flips between runs or it passes only on retry.</Empty>}>
        {(rows) => (
          <Table>
            <thead>
              <tr>
                <Th>Scenario</Th>
                <Th numeric>Runs</Th>
                <Th numeric>Passes</Th>
                <Th numeric>Failures</Th>
                <Th numeric>Flips</Th>
                <Th numeric>Passed on retry</Th>
                <Th numeric>Flakiness</Th>
                <Th>Last run</Th>
              </tr>
            </thead>
            <tbody>
              {rows.map((f) => (
                <Tr key={f.scenario_id}>
                  <Td label="Scenario" className="max-w-[28rem]">
                    <span className="inline-flex flex-wrap items-center gap-2">
                      <StatusPill status="flaky" />
                      {f.scenario_name}
                    </span>
                  </Td>
                  <Td label="Runs" numeric mono>
                    {f.runs}
                  </Td>
                  <Td label="Passes" numeric mono>
                    {f.passes}
                  </Td>
                  <Td label="Failures" numeric mono className={f.failures ? "text-error" : undefined}>
                    {f.failures}
                  </Td>
                  <Td label="Flips" numeric mono>
                    {f.flips}
                  </Td>
                  <Td label="Passed on retry" numeric mono>
                    {f.passed_on_retry}
                  </Td>
                  <Td label="Flakiness" numeric mono>
                    {percent(f.flakiness)}
                  </Td>
                  <Td label="Last run" className="text-ink-2">
                    <span title={dateTime(f.last_run_at)}>{relative(f.last_run_at)}</span>
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
    </Section>
  );
}

export function FailuresTab({ ws, projectId }: { ws: string; projectId: string | null }) {
  const failures = useQuery({
    queryKey: keys.ws(ws, "failures", 50, projectId ?? "all"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/failures", { params: { path: { ws }, query: { limit: 50, project_id: projectId ?? undefined } } })),
  });
  return (
    <Section title="Recent failures" description="The 50 most recent failed or errored final attempts. Open one to see its steps, logs and screenshots.">
      <QueryState query={failures} empty={<Empty title="No failures recorded.">Failed final attempts appear here with a link to their evidence.</Empty>}>
        {(rows) => (
          <Table>
            <thead>
              <tr>
                <Th>Scenario</Th>
                <Th>Status</Th>
                <Th>Message</Th>
                <Th>Run</Th>
                <Th>Investigated</Th>
                <Th>Finished</Th>
              </tr>
            </thead>
            <tbody>
              {rows.map((f) => (
                <Tr key={f.job_id}>
                  <Td label="Scenario" className="max-w-[22rem]">
                    <Link href={`/executions/${f.execution_id}/jobs/${f.job_id}`} className="font-medium no-underline hover:underline">
                      {f.scenario_name}
                    </Link>
                    <div className="text-xs text-muted">{f.project_name}</div>
                  </Td>
                  <Td label="Status">
                    <StatusPill status={f.status} />
                  </Td>
                  <Td label="Message" className="max-w-[28rem]">
                    {f.message ? <code className="line-clamp-2 text-xs break-words text-ink-2">{f.message}</code> : <span className="text-muted">no message</span>}
                  </Td>
                  <Td label="Run" mono className="text-xs">
                    <Link href={`/executions/${f.execution_id}`} className="text-ink-2">
                      {shortId(f.execution_id)}
                    </Link>
                  </Td>
                  <Td label="Investigated" className="text-ink-2">
                    {f.investigated ? "Yes" : "No"}
                  </Td>
                  <Td label="Finished" className="text-ink-2">
                    <span title={dateTime(f.finished_at)}>{relative(f.finished_at)}</span>
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
    </Section>
  );
}
