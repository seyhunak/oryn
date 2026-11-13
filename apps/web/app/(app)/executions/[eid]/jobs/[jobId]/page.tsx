"use client";
import Link from "next/link";
import { use } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { useCan } from "@/lib/hooks";
import { bytes, dateTime, duration, shortId, time } from "@/lib/format";
import { PageHeader, Meta, Section } from "@/components/ui/page";
import { Empty, ErrorState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { UrlTabs, useTab } from "@/components/ui/tabs";
import { EvidenceLabel } from "@/components/ui/evidence-label";
import { InvestigationView } from "@/components/common/investigation";
import { cn } from "@/lib/cn";

type Log = Schemas["LogLine"];
type Net = Schemas["NetworkEvent"];

export default function JobPage({ params }: { params: Promise<{ eid: string; jobId: string }> }) {
  const { eid, jobId } = use(params);
  const canRun = useCan("engineer");
  const tab = useTab("steps");
  const q = useQuery({
    queryKey: ["job", jobId],
    queryFn: () => unwrap(api.GET("/api/v1/jobs/{job_id}", { params: { path: { job_id: jobId } } })),
    refetchInterval: (query) => (["queued", "claimed", "running"].includes(query.state.data?.job.status ?? "") ? 3000 : false),
  });
  if (q.isPending) return <Skeleton className="h-40" />;
  if (q.error || !q.data) return <ErrorState error={q.error} retry={() => q.refetch()} />;
  const { job, steps, artifacts, attempts, investigations } = q.data;
  const logs = (Array.isArray(q.data.logs) ? q.data.logs : []) as Log[];
  const network = (Array.isArray(q.data.network) ? q.data.network : []) as Net[];
  const failed = steps.find((s) => s.status === "failed" || s.status === "error");
  const finished = !["queued", "claimed", "running"].includes(job.status);
  const shots = artifacts.filter((a) => a.content_type.startsWith("image/"));

  return (
    <>
      <PageHeader
        crumbs={[
          { href: "/executions", label: "Executions" },
          { href: `/executions/${eid}`, label: `Run ${shortId(eid)}` },
        ]}
        title={job.scenario_name}
        description={`Attempt ${job.attempt} of ${job.max_attempts}, ${job.kind} scenario${job.worker_name ? `, run by ${job.worker_name}` : ""}.`}
      />
      <div className="grid grid-cols-1 gap-8 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="order-2 min-w-0 min-[72rem]:order-1">
          <UrlTabs
            tabs={[
              { id: "steps", label: "Steps", count: steps.length },
              { id: "investigation", label: "Investigation", count: investigations.length },
              { id: "evidence", label: "Artifacts", count: artifacts.length },
              { id: "logs", label: "Logs", count: logs.length },
              { id: "network", label: "Network", count: network.length },
            ]}
          />
          {tab === "steps" ? (
            steps.length ? (
              <ol className="flex flex-col">
                {steps.map((s) => (
                  <li key={s.id} className={cn("grid grid-cols-[2rem_minmax(0,1fr)_auto] gap-3 border-b border-rule py-3", s.status === "failed" && "bg-error-soft")}>
                    <span className="mono pt-0.5 text-right text-xs text-muted">{s.position + 1}</span>
                    <div className="flex min-w-0 flex-col gap-1">
                      <p className="mono text-sm break-words">
                        <span className="font-medium text-ink">{s.keyword}</span> {s.text}
                      </p>
                      {s.message ? <p className="text-sm break-words text-ink-2">{s.message}</p> : null}
                      {s.expected != null || s.actual != null ? (
                        <dl className="mono grid grid-cols-[5rem_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
                          <dt className="text-muted">expected</dt>
                          <dd className="m-0 break-words">{s.expected ?? "–"}</dd>
                          <dt className="text-muted">actual</dt>
                          <dd className={cn("m-0 break-words", s.status === "failed" && "text-error")}>{s.actual ?? "–"}</dd>
                        </dl>
                      ) : null}
                      {s.artifact_name ? <span className="text-xs text-muted">evidence: {s.artifact_name}</span> : null}
                    </div>
                    <div className="flex flex-col items-end gap-1">
                      <StatusPill status={s.status} />
                      <span className="mono text-xs text-muted">{duration(s.duration_ms)}</span>
                    </div>
                  </li>
                ))}
              </ol>
            ) : (
              <Empty title={finished ? "No steps were recorded." : "Steps appear when the worker reports."}>{job.error ? <code className="text-xs">{job.error}</code> : null}</Empty>
            )
          ) : null}

          {tab === "investigation" ? (
            <div id="investigation">
              {job.status === "passed" && !investigations.length ? (
                <Empty title="This scenario passed. Nothing to investigate." />
              ) : (
                <InvestigationView inv={investigations[0]} jobId={job.id} canRun={canRun && finished} />
              )}
            </div>
          ) : null}

          {tab === "evidence" ? (
            artifacts.length ? (
              <div className="flex flex-col gap-6">
                {shots.length ? (
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,280px),1fr))] gap-4">
                    {shots.map((a) => (
                      <a key={a.id} href={`/api/v1/artifacts/${a.id}/content`} target="_blank" rel="noreferrer" className="flex flex-col gap-1 no-underline">
                        {/* eslint-disable-next-line @next/next/no-img-element */}
                        <img src={`/api/v1/artifacts/${a.id}/content`} alt={`Screenshot ${a.name}`} loading="lazy" className="aspect-video w-full rounded-[var(--radius-input)] border border-rule object-cover object-top" />
                        <span className="mono truncate text-xs text-ink-2">{a.name}</span>
                      </a>
                    ))}
                  </div>
                ) : null}
                <Table>
                  <thead>
                    <tr>
                      <Th>Artifact</Th>
                      <Th>Kind</Th>
                      <Th numeric>Size</Th>
                      <Th>SHA-256</Th>
                    </tr>
                  </thead>
                  <tbody>
                    {artifacts.map((a) => (
                      <Tr key={a.id}>
                        <Td label="Artifact">
                          <a href={`/api/v1/artifacts/${a.id}/content`} target="_blank" rel="noreferrer" className="mono">
                            {a.name}
                          </a>
                        </Td>
                        <Td label="Kind">
                          <Tag>{a.kind}</Tag>
                        </Td>
                        <Td label="Size" numeric mono>
                          {bytes(a.size_bytes)}
                        </Td>
                        <Td label="SHA-256" mono className="text-xs text-muted">
                          {a.sha256.slice(0, 16)}…
                        </Td>
                      </Tr>
                    ))}
                  </tbody>
                </Table>
              </div>
            ) : (
              <Empty title="No artifacts were uploaded for this attempt." />
            )
          ) : null}

          {tab === "logs" ? (
            logs.length ? (
              <pre className="max-h-[70vh] overflow-auto rounded-[var(--radius-input)] bg-paper-2 p-3 text-xs leading-relaxed">
                {logs.map((l, i) => (
                  <div key={i} className={cn(l.level === "error" && "text-error")}>
                    <span className="text-muted">{time(l.timestamp)}</span> <span className="text-ink-2">{l.source}</span> {l.level !== "info" ? `${l.level} ` : ""}
                    {l.message}
                  </div>
                ))}
              </pre>
            ) : (
              <Empty title="No log lines were captured." />
            )
          ) : null}

          {tab === "network" ? (
            network.length ? (
              <Table>
                <thead>
                  <tr>
                    <Th>Method</Th>
                    <Th>URL</Th>
                    <Th numeric>Status</Th>
                  </tr>
                </thead>
                <tbody>
                  {network.map((n, i) => (
                    <Tr key={i}>
                      <Td label="Method" mono>
                        {n.method}
                      </Td>
                      <Td label="URL" mono className="max-w-[40rem] text-xs break-all">
                        {n.url}
                        {n.failure ? <div className="text-error">{n.failure}</div> : null}
                      </Td>
                      <Td label="Status" numeric mono className={(n.status ?? 0) >= 400 || n.failure ? "text-error" : undefined}>
                        {n.status ?? "–"}
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            ) : (
              <Empty title="No network requests were recorded." />
            )
          ) : null}
        </div>

        <aside className="order-1 flex flex-col gap-6 min-[72rem]:order-2">
          <EvidenceLabel
            title={job.scenario_name}
            lines={[
              [job.kind, job.browser_version ?? "–"],
              [time(job.started_at), duration(job.duration_ms)],
              [`${steps.filter((s) => s.status === "passed").length}/${steps.length} steps`, `attempt ${job.attempt}`],
            ]}
            verified={finished ? job.status === "passed" : null}
            verifiedAt={time(job.finished_at)}
            by={job.worker_name}
            failure={failed ? `${failed.keyword} ${failed.text}` : job.error}
          />
          <Meta
            items={[
              { label: "Status", value: <StatusPill status={job.status} /> },
              { label: "Started", value: dateTime(job.started_at) },
              { label: "Finished", value: dateTime(job.finished_at) },
              { label: "Job", value: <code className="text-xs">{job.id}</code> },
            ]}
          />
          {attempts.length ? (
            <Section title="Earlier attempts">
              <ul className="flex flex-col gap-1 text-sm">
                {attempts.map((a) => (
                  <li key={a.id} className="flex items-center justify-between gap-2">
                    <Link href={`/executions/${eid}/jobs/${a.id}`}>Attempt {a.attempt}</Link>
                    <StatusPill status={a.status} />
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}
        </aside>
      </div>
    </>
  );
}
