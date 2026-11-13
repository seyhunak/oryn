"use client";
import Link from "next/link";
import { use } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCurrentProject } from "@/lib/hooks";
import { dateTime, duration, relative, shortId } from "@/lib/format";
import { Meta, PageHeader, Section } from "@/components/ui/page";
import { Empty, ErrorState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";

function stepType(step: unknown): string | null {
  return step && typeof step === "object" && "type" in step && typeof (step as { type: unknown }).type === "string" ? (step as { type: string }).type : null;
}

function diagnosticsOf(s: Schemas["Scenario"]): Schemas["Diagnostic"][] {
  return Array.isArray(s.diagnostics) ? (s.diagnostics as Schemas["Diagnostic"][]) : [];
}

export default function ScenarioPage({ params }: { params: Promise<{ scenarioId: string }> }) {
  const { scenarioId } = use(params);
  const { project, isPending, error, refetch } = useCurrentProject();
  const pid = project?.id ?? "";
  const detail = useQuery({
    queryKey: keys.project(pid, "scenario", scenarioId),
    enabled: !!pid,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/scenarios/{scenario_id}", { params: { path: { id: pid, scenario_id: scenarioId } } })),
  });

  if (isPending || (pid && detail.isPending)) return <Skeleton className="h-40" />;
  if (error) return <ErrorState error={error} retry={() => refetch()} />;
  if (!project) return <Empty title="No projects yet.">Scenarios belong to a project.</Empty>;
  if (detail.error || !detail.data) return <ErrorState error={detail.error} retry={() => detail.refetch()} />;
  const { scenario: s, steps, requirements, history } = detail.data;
  const diags = diagnosticsOf(s);

  return (
    <>
      <PageHeader
        crumbs={[
          { href: `/studio?project=${pid}`, label: "Test Studio" },
          { href: `/studio/${s.suite_id}?project=${pid}`, label: s.suite_name },
        ]}
        title={s.name}
        description={
          <>
            {s.kind.charAt(0).toUpperCase() + s.kind.slice(1)} scenario in {s.suite_name}, line <span className="mono">{s.line}</span>.{" "}
            {s.status === "approved" ? "Approved to run." : s.status === "archived" ? "Archived; it does not run." : "Draft; it runs only after approval."}
          </>
        }
      />
      <div className="grid grid-cols-1 gap-8 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="order-2 flex min-w-0 flex-col gap-8 min-[72rem]:order-1">
          {diags.length ? (
            <Section title="Unsupported steps" description="These steps do not match the step library, so the scenario cannot be approved or run.">
              <ul className="flex flex-col border-t border-rule">
                {diags.map((d, i) => (
                  <li key={i} className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2 border-b border-rule py-2 text-sm">
                    <span className="mono text-muted">line {d.line}</span>
                    <span className="text-error">{d.message}</span>
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}

          <Section title="Steps" description="As compiled by the engine. The action column is what the runner executes.">
            {steps.length === 0 ? (
              <Empty title="No compiled steps.">Fix the unsupported steps in the suite editor and save.</Empty>
            ) : (
              <Table>
                <thead>
                  <tr>
                    <Th numeric>#</Th>
                    <Th>Step</Th>
                    <Th>Action</Th>
                  </tr>
                </thead>
                <tbody>
                  {steps.map((st) => (
                    <Tr key={st.position}>
                      <Td label="#" numeric mono className="text-muted">
                        {st.position + 1}
                      </Td>
                      <Td label="Step" mono>
                        <span className="font-medium text-accent-deep">{st.keyword}</span> {st.text}
                      </Td>
                      <Td label="Action">{stepType(st.step) ? <Tag>{stepType(st.step)!.replace(/_/g, " ")}</Tag> : <span className="text-muted">unsupported</span>}</Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </Section>

          <Section title="Recent runs" description="Final status per execution, newest first.">
            {history.length === 0 ? (
              <Empty title="Not run yet.">{s.status === "approved" ? "Run its suite from Test Studio." : "Approve it, then run its suite from Test Studio."}</Empty>
            ) : (
              <Table>
                <thead>
                  <tr>
                    <Th>Run</Th>
                    <Th>Status</Th>
                    <Th numeric>Attempt</Th>
                    <Th>Environment</Th>
                    <Th>Commit</Th>
                    <Th numeric>Duration</Th>
                    <Th>Finished</Th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((h) => (
                    <Tr key={h.job_id}>
                      <Td label="Run">
                        <Link href={`/executions/${h.execution_id}/jobs/${h.job_id}`} className="mono no-underline hover:underline">
                          {shortId(h.execution_id)}
                        </Link>
                        {h.superseded ? <span className="ml-2 text-xs text-muted">retried</span> : null}
                      </Td>
                      <Td label="Status">
                        <StatusPill status={h.status} />
                      </Td>
                      <Td label="Attempt" numeric mono>
                        {h.attempt}
                      </Td>
                      <Td label="Environment" className="text-ink-2">
                        {h.environment_name}
                      </Td>
                      <Td label="Commit" mono className="text-ink-2">
                        {h.commit_sha ? h.commit_sha.slice(0, 7) : "–"}
                      </Td>
                      <Td label="Duration" numeric mono>
                        {duration(h.duration_ms)}
                      </Td>
                      <Td label="Finished" className="text-ink-2">
                        {relative(h.finished_at)}
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </Section>
        </div>

        <aside className="order-1 flex min-w-0 flex-col gap-6 min-[72rem]:order-2">
          <Meta
            items={[
              { label: "Status", value: <StatusPill status={s.status} /> },
              { label: "Executable", value: s.executable ? "Yes, every step compiled" : <span className="text-error">No, unsupported steps</span> },
              { label: "Last run", value: s.last_status ? <StatusPill status={s.last_status} /> : "Not run" },
              { label: "Kind", value: s.kind },
              { label: "Origin", value: s.origin },
              { label: "Suite", value: <Link href={`/studio/${s.suite_id}?project=${pid}`}>{s.suite_name}</Link> },
              ...(s.outline ? [{ label: "Outline", value: `${s.outline}${s.example_index != null ? `, example ${s.example_index + 1}` : ""}` }] : []),
              { label: "Created", value: <span className="mono text-xs">{dateTime(s.created_at)}</span> },
              { label: "Updated", value: <span className="mono text-xs">{dateTime(s.updated_at)}</span> },
              { label: "ID", value: <span className="mono text-xs break-all">{s.id}</span> },
            ]}
          />
          <div className="flex flex-col gap-2">
            <h3>Tags</h3>
            {s.tags.length ? (
              <div className="flex flex-wrap gap-1">
                {s.tags.map((t) => (
                  <Tag key={t}>@{t}</Tag>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted">No tags.</p>
            )}
          </div>
          <div className="flex flex-col gap-2">
            <h3>Requirements</h3>
            {requirements.length ? (
              <ul className="flex flex-col gap-1 text-sm">
                {requirements.map((r) => (
                  <li key={r.id}>
                    <span className="mono font-medium">{r.key}</span> <span className="text-ink-2">{r.title}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-sm text-muted">Not linked to a requirement.</p>
            )}
          </div>
        </aside>
      </div>
    </>
  );
}
