"use client";
import Link from "next/link";
import { use, useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { keys, useCan, useWorkspace, useWsId } from "@/lib/hooks";
import { dateTime, duration, relative, shortId } from "@/lib/format";
import { Meta, PageHeader, Section } from "@/components/ui/page";
import { Empty, ErrorState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/dialog";

export default function WorkerPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  const ws = useWsId();
  const { me } = useWorkspace();
  const qc = useQueryClient();
  const router = useRouter();
  const isAdmin = useCan("admin");
  const [confirm, setConfirm] = useState(false);
  const detail = useQuery({
    queryKey: keys.ws(ws, "workers", id),
    enabled: !!ws,
    refetchInterval: 10_000,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/workers/{id}", { params: { path: { ws, id } } })),
  });
  const revoke = useMutation({
    mutationFn: () => unwrap(api.DELETE("/api/v1/workspaces/{ws}/workers/{id}", { params: { path: { ws, id } } })),
    onSuccess: async () => {
      setConfirm(false);
      await qc.invalidateQueries({ queryKey: keys.ws(ws, "workers") });
      router.push("/workers");
    },
  });

  if (detail.isPending)
    return (
      <div className="flex flex-col gap-4">
        <Skeleton className="h-10 w-64" />
        <Skeleton className="h-40" />
      </div>
    );
  if (detail.error || !detail.data) return <ErrorState error={detail.error} retry={() => detail.refetch()} />;
  const { worker: w, capabilities, recent_jobs } = detail.data;
  const shared = !w.workspace_id;
  const canRevoke = isAdmin && !w.revoked_at && (!shared || !!me?.user.is_instance_admin);

  return (
    <>
      <PageHeader
        crumbs={[{ href: "/workers", label: "Workers" }]}
        title={
          <span className="inline-flex flex-wrap items-center gap-3">
            {w.name} <StatusPill status={w.status} />
          </span>
        }
        description={shared ? "Shared worker: it serves every workspace on this instance. Only this workspace's jobs are listed." : "Workspace worker: it runs this workspace's jobs only."}
        actions={
          canRevoke ? (
            <Button variant="danger" size="lg" onClick={() => setConfirm(true)}>
              Revoke
            </Button>
          ) : null
        }
      />
      <div className="grid grid-cols-1 gap-10 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="order-2 flex flex-col gap-10 min-[72rem]:order-1">
          <Section title="Capabilities" description="Reported by the worker at its last heartbeat. Jobs are only claimed for available capabilities.">
            {capabilities.length === 0 ? (
              <Empty title="No capabilities reported.">The worker reports its capabilities when it starts.</Empty>
            ) : (
              <Table>
                <thead>
                  <tr>
                    <Th>Capability</Th>
                    <Th>Version</Th>
                    <Th>Availability</Th>
                  </tr>
                </thead>
                <tbody>
                  {capabilities.map((c) => (
                    <Tr key={c.capability}>
                      <Td label="Capability" mono>
                        {c.capability}
                      </Td>
                      <Td label="Version" mono className="text-ink-2">
                        {c.version ?? "–"}
                      </Td>
                      <Td label="Availability">{c.available ? <StatusPill status="ready" label="available" /> : <StatusPill status="disabled" label="not available" />}</Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </Section>
          <Section title="Recent jobs">
            {recent_jobs.length === 0 ? (
              <Empty title="No jobs from this workspace yet.">Jobs appear here once this worker claims one.</Empty>
            ) : (
              <Table>
                <thead>
                  <tr>
                    <Th>Scenario</Th>
                    <Th>Status</Th>
                    <Th>Kind</Th>
                    <Th numeric>Attempt</Th>
                    <Th numeric>Duration</Th>
                    <Th>Finished</Th>
                  </tr>
                </thead>
                <tbody>
                  {recent_jobs.map((j) => (
                    <Tr key={j.id}>
                      <Td label="Scenario">
                        <Link href={`/executions/${j.execution_id}/jobs/${j.id}`} className="font-medium no-underline hover:underline">
                          {j.scenario_name}
                        </Link>
                        <span className="mono ml-2 text-xs text-muted">{shortId(j.id)}</span>
                      </Td>
                      <Td label="Status">
                        <StatusPill status={j.status} />
                      </Td>
                      <Td label="Kind" className="text-ink-2">
                        {j.kind}
                      </Td>
                      <Td label="Attempt" numeric mono>
                        {j.attempt}/{j.max_attempts}
                      </Td>
                      <Td label="Duration" numeric mono className="whitespace-nowrap">
                        {duration(j.duration_ms)}
                      </Td>
                      <Td label="Finished" className="whitespace-nowrap text-ink-2">
                        <span title={dateTime(j.finished_at)}>{j.finished_at ? relative(j.finished_at) : "–"}</span>
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </Section>
        </div>
        <aside className="order-1 min-[72rem]:order-2">
          <div className="panel flex flex-col gap-4 p-4">
            <h3>Worker</h3>
            <Meta
              items={[
                { label: "ID", value: <span className="mono text-xs">{w.id}</span> },
                { label: "Scope", value: shared ? "Shared" : "Workspace" },
                { label: "Version", value: <span className="mono">{w.version ?? "–"}</span> },
                { label: "Host", value: <span className="mono text-xs">{w.hostname ?? "–"}</span> },
                { label: "OS", value: w.os ?? "–" },
                { label: "Concurrency", value: <span className="mono">{w.concurrency}</span> },
                { label: "Active jobs", value: <span className="mono">{w.active_jobs}</span> },
                { label: "Jobs completed", value: <span className="mono">{w.jobs_completed}</span> },
                { label: "Last heartbeat", value: <span className="mono">{dateTime(w.last_heartbeat_at)}</span> },
                { label: "Registered", value: <span className="mono">{dateTime(w.registered_at)}</span> },
                ...(w.revoked_at ? [{ label: "Revoked", value: <span className="mono">{dateTime(w.revoked_at)}</span> }] : []),
                {
                  label: "Labels",
                  value: w.labels.length ? (
                    <span className="flex flex-wrap gap-1">
                      {w.labels.map((l) => (
                        <Tag key={l}>{l}</Tag>
                      ))}
                    </span>
                  ) : (
                    <span className="text-muted">none</span>
                  ),
                },
              ]}
            />
            {isAdmin && shared && !me?.user.is_instance_admin ? <p className="text-xs text-muted">Shared workers can only be revoked by an instance admin.</p> : null}
          </div>
        </aside>
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={`Revoke ${w.name}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>The worker&apos;s token stops working at once and it will claim no further jobs. Jobs it is running now may fail. To use the machine again, add a new worker and start it with the new token.</p>
            {revoke.error ? <p role="alert" className="text-sm text-error">{errorMessage(revoke.error)}</p> : null}
          </div>
        }
        confirmLabel="Revoke worker"
        loading={revoke.isPending}
        onConfirm={() => revoke.mutate()}
      />
    </>
  );
}
