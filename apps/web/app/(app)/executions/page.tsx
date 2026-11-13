"use client";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { duration, relative, shortId } from "@/lib/format";
import { PageHeader } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { ProjectPicker } from "@/components/common/project-picker";
import { Button } from "@/components/ui/button";

const STATUSES = ["", "queued", "running", "passed", "failed", "error", "cancelled"];
const PER_PAGE = 25;

export default function ExecutionsPage() {
  const ws = useWsId();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const project = sp.get("project");
  const status = sp.get("status") ?? "";
  const page = Number(sp.get("page") ?? "1");
  const query = {
    project_id: project && project !== "all" ? project : undefined,
    status: status || undefined,
    page,
    per_page: PER_PAGE,
  };
  const runs = useQuery({
    queryKey: keys.ws(ws, "executions", query),
    enabled: !!ws,
    refetchInterval: 5_000,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/executions", { params: { path: { ws }, query } })),
  });
  const set = (k: string, v: string | null) => {
    const q = new URLSearchParams(sp.toString());
    if (v) q.set(k, v);
    else q.delete(k);
    if (k !== "page") q.delete("page");
    router.push(`${path}?${q.toString()}`);
  };

  return (
    <>
      <PageHeader title="Executions" description="Every run, newest first. A verdict comes from the Rust engine's assertions, never from a model." />
      <div className="mb-4 flex flex-wrap items-center gap-4">
        <ProjectPicker allowAll />
        <label className="flex items-center gap-2 text-sm text-ink-2">
          Status
          <select className="field h-9 w-auto" value={status} onChange={(e) => set("status", e.target.value || null)}>
            {STATUSES.map((s) => (
              <option key={s} value={s}>
                {s || "Any"}
              </option>
            ))}
          </select>
        </label>
      </div>
      <QueryState query={runs} isEmpty={(d) => d.items.length === 0} empty={<Empty title="No executions match.">Run approved scenarios from Test Studio, or clear the filters.</Empty>}>
        {(d) => (
          <>
            <Table>
              <thead>
                <tr>
                  <Th>Run</Th>
                  <Th>Status</Th>
                  <Th numeric>Passed</Th>
                  <Th numeric>Failed</Th>
                  <Th>Environment</Th>
                  <Th>Trigger</Th>
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
                    <Td label="Failed" numeric mono className={e.failed + e.errored ? "text-error" : undefined}>
                      {e.failed + e.errored}
                    </Td>
                    <Td label="Environment">{e.environment_name}</Td>
                    <Td label="Trigger" className="text-ink-2">
                      {e.trigger}
                      {e.triggered_by_name ? `, ${e.triggered_by_name}` : ""}
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
            <div className="mt-4 flex items-center justify-between text-sm text-ink-2">
              <span className="tabular-nums">
                {(page - 1) * PER_PAGE + 1} to {(page - 1) * PER_PAGE + d.items.length} of {d.total}
              </span>
              <div className="flex gap-2">
                <Button size="sm" disabled={page <= 1} onClick={() => set("page", String(page - 1))}>
                  Newer
                </Button>
                <Button size="sm" disabled={page * PER_PAGE >= d.total} onClick={() => set("page", String(page + 1))}>
                  Older
                </Button>
              </div>
            </div>
          </>
        )}
      </QueryState>
    </>
  );
}
