"use client";
import { useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { dateTime, shortId } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";

const PER_PAGE = 50;

export function AuditTab() {
  const ws = useWsId();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const action = sp.get("action") ?? "";
  const page = Number(sp.get("page") ?? "1");
  const [draft, setDraft] = useState(action);
  const query = { action: action || undefined, page, per_page: PER_PAGE };
  const audit = useQuery({
    queryKey: keys.ws(ws, "audit", query),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/audit", { params: { path: { ws }, query } })),
  });
  const set = (k: string, v: string | null) => {
    const q = new URLSearchParams(sp.toString());
    if (v) q.set(k, v);
    else q.delete(k);
    if (k !== "page") q.delete("page");
    router.push(`${path}?${q.toString()}`, { scroll: false });
  };

  return (
    <Section title="Audit log" description="Every change to this workspace, newest first: who did it, what changed, and from where.">
      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          set("action", draft.trim() || null);
        }}
      >
        <label className="flex flex-col gap-1 text-sm font-medium text-ink-2">
          Action starts with
          <input className="field h-9 w-64 font-mono" value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="execution." autoComplete="off" />
        </label>
        <Button type="submit" size="sm">
          Filter
        </Button>
        {action ? (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setDraft("");
              set("action", null);
            }}
          >
            Clear
          </Button>
        ) : null}
      </form>
      <QueryState query={audit} isEmpty={(d) => d.items.length === 0} empty={<Empty title={action ? `No events whose action starts with ${action}.` : "No audit events yet."}>{action ? "Clear the filter to see every event." : "Changes to projects, members, settings and runs are recorded here."}</Empty>}>
        {(d) => (
          <>
            <Table>
              <thead>
                <tr>
                  <Th>Time</Th>
                  <Th>Actor</Th>
                  <Th>Action</Th>
                  <Th>Target</Th>
                  <Th>Details</Th>
                  <Th>IP</Th>
                </tr>
              </thead>
              <tbody>
                {d.items.map((e) => {
                  const data = e.data && typeof e.data === "object" ? Object.entries(e.data as Record<string, unknown>) : [];
                  return (
                    <Tr key={e.id}>
                      <Td label="Time" mono className="text-xs whitespace-nowrap">
                        {dateTime(e.created_at)}
                      </Td>
                      <Td label="Actor">
                        {e.actor_label ?? <span className="text-muted">unknown</span>}
                        {e.actor_kind !== "user" ? <div className="text-xs text-muted">{e.actor_kind}</div> : null}
                      </Td>
                      <Td label="Action" mono className="text-xs">
                        {e.action}
                      </Td>
                      <Td label="Target" className="text-xs text-ink-2">
                        {e.subject_type ? (
                          <>
                            {e.subject_type}
                            {e.subject_id ? (
                              <span className="mono ml-1" title={e.subject_id}>
                                {shortId(e.subject_id)}
                              </span>
                            ) : null}
                          </>
                        ) : (
                          "–"
                        )}
                      </Td>
                      <Td label="Details" className="max-w-[24rem]">
                        {data.length ? (
                          <code className="line-clamp-2 text-xs break-words text-ink-2">{data.map(([k, v]) => `${k}=${typeof v === "object" ? JSON.stringify(v) : String(v)}`).join(", ")}</code>
                        ) : (
                          <span className="text-muted">none</span>
                        )}
                      </Td>
                      <Td label="IP" mono className="text-xs text-ink-2">
                        {e.ip ?? "–"}
                      </Td>
                    </Tr>
                  );
                })}
              </tbody>
            </Table>
            <div className="flex items-center justify-between text-sm text-ink-2">
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
    </Section>
  );
}
