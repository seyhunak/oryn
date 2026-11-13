"use client";
import Link from "next/link";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, unwrap } from "@/lib/api/client";
import { keys, useCan, useWsId } from "@/lib/hooks";
import { dateTime, relative } from "@/lib/format";
import { PageHeader, Stat } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { AddWorkerDialog } from "@/components/workers/add-worker-dialog";

export default function WorkersPage() {
  const ws = useWsId();
  const isAdmin = useCan("admin");
  const [adding, setAdding] = useState(false);
  const workers = useQuery({
    queryKey: keys.ws(ws, "workers"),
    enabled: !!ws,
    refetchInterval: 10_000,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/workers", { params: { path: { ws } } })),
  });
  const queue = useQuery({
    queryKey: keys.ws(ws, "queue"),
    enabled: !!ws,
    refetchInterval: 5_000,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/queue", { params: { path: { ws } } })),
  });
  const addButton = isAdmin ? (
    <Button variant="primary" size="lg" onClick={() => setAdding(true)}>
      <Plus aria-hidden className="size-4" /> Add worker
    </Button>
  ) : null;

  return (
    <>
      <PageHeader title="Workers" description="Machines that claim jobs from the queue and record evidence. Shared workers serve every workspace on this instance." actions={addButton} />
      <div className="flex flex-col gap-6">
        <QueryState
          query={queue}
          skeleton={
            <div className="grid grid-cols-2 gap-6 md:grid-cols-4">
              <Skeleton className="h-14" />
              <Skeleton className="h-14" />
            </div>
          }
        >
          {(q) => {
            const list = workers.data ?? [];
            const online = list.filter((w) => w.status === "online");
            const capacity = online.reduce((n, w) => n + w.concurrency, 0);
            return (
              <div className="grid grid-cols-2 gap-x-6 gap-y-4 border-y border-rule py-4 md:grid-cols-4">
                <Stat label="Jobs queued" value={q.queued} hint="waiting for a worker" />
                <Stat label="Jobs running" value={q.running} />
                <Stat label="Workers online" value={workers.data ? online.length : "–"} hint={workers.data ? `of ${list.length} registered` : undefined} />
                <Stat label="Online capacity" value={workers.data ? capacity : "–"} hint="concurrent jobs across online workers" />
              </div>
            );
          }}
        </QueryState>

        <QueryState
          query={workers}
          empty={
            <Empty title="No workers registered." action={addButton}>
              A worker runs scenarios. Add one and start it with the token it is issued.
            </Empty>
          }
        >
          {(list) => (
            <Table>
              <thead>
                <tr>
                  <Th>Worker</Th>
                  <Th>Status</Th>
                  <Th>Scope</Th>
                  <Th>Version</Th>
                  <Th>Host</Th>
                  <Th numeric>Concurrency</Th>
                  <Th numeric>Active jobs</Th>
                  <Th numeric>Jobs completed</Th>
                  <Th>Last heartbeat</Th>
                </tr>
              </thead>
              <tbody>
                {list.map((w) => (
                  <Tr key={w.id}>
                    <Td label="Worker">
                      <Link href={`/workers/${w.id}`} className="font-medium no-underline hover:underline">
                        {w.name}
                      </Link>
                      {w.labels.length ? (
                        <div className="mt-1 flex flex-wrap gap-1">
                          {w.labels.map((l) => (
                            <Tag key={l}>{l}</Tag>
                          ))}
                        </div>
                      ) : null}
                    </Td>
                    <Td label="Status">
                      <StatusPill status={w.status} />
                    </Td>
                    <Td label="Scope" className="text-ink-2">
                      {w.workspace_id ? "Workspace" : "Shared"}
                    </Td>
                    <Td label="Version" mono className="text-ink-2">
                      {w.version ?? "–"}
                    </Td>
                    <Td label="Host" className="text-ink-2">
                      <span className="mono text-xs">{w.hostname ?? "–"}</span>
                      {w.os ? <div className="text-xs text-muted">{w.os}</div> : null}
                    </Td>
                    <Td label="Concurrency" numeric mono>
                      {w.concurrency}
                    </Td>
                    <Td label="Active jobs" numeric mono>
                      {w.active_jobs}
                    </Td>
                    <Td label="Jobs completed" numeric mono>
                      {w.jobs_completed}
                    </Td>
                    <Td label="Last heartbeat" className="text-ink-2">
                      <span title={dateTime(w.last_heartbeat_at)}>{relative(w.last_heartbeat_at)}</span>
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          )}
        </QueryState>
      </div>
      {isAdmin ? <AddWorkerDialog open={adding} onOpenChange={setAdding} /> : null}
    </>
  );
}
