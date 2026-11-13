"use client";
import Link from "next/link";
import { useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, unwrap } from "@/lib/api/client";
import { keys, useCan, useProjects, useWsId } from "@/lib/hooks";
import { dateTime, relative } from "@/lib/format";
import { PageHeader, Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button, buttonClass } from "@/components/ui/button";
import { CreateIntegrationDialog } from "@/components/integrations/create-integration-dialog";
import { IntegrationDetail, configUrl, deliveryStatus } from "@/components/integrations/integration-detail";

export default function IntegrationsPage() {
  const ws = useWsId();
  const isAdmin = useCan("admin");
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const selected = sp.get("id");
  const [adding, setAdding] = useState(false);
  const projects = useProjects();
  const list = useQuery({
    queryKey: keys.ws(ws, "integrations"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/integrations", { params: { path: { ws } } })),
  });
  const select = (id: string | null) => {
    const q = new URLSearchParams(sp.toString());
    if (id) q.set("id", id);
    else q.delete("id");
    router.push(`${path}?${q.toString()}`, { scroll: false });
  };
  const projectName = (id?: string | null) => (id ? (projects.data?.find((p) => p.id === id)?.name ?? "One project") : "All projects");
  const addButton = isAdmin ? (
    <Button variant="primary" size="lg" onClick={() => setAdding(true)}>
      <Plus aria-hidden className="size-4" /> Add integration
    </Button>
  ) : null;
  const current = list.data?.find((i) => i.id === selected) ?? null;

  return (
    <>
      <PageHeader title="Integrations" description="Notify other systems when executions finish or fail. Webhooks are signed; Slack posts go to an incoming webhook." actions={addButton} />
      <div className="flex flex-col gap-10">
        <Section title="Notifications">
          <p className="max-w-[80ch] text-sm text-ink-2">
            Outbound URLs that resolve to private or loopback networks are refused unless the server is started with <code className="mono">ORYN_ALLOW_PRIVATE_NETWORK=true</code>.
          </p>
          <QueryState
            query={list}
            empty={
              <Empty title="No integrations yet." action={addButton}>
                Add a webhook or a Slack channel to hear about failed runs without opening ORYN.
              </Empty>
            }
          >
            {(rows) => (
              <Table>
                <thead>
                  <tr>
                    <Th>Integration</Th>
                    <Th>Kind</Th>
                    <Th>Events</Th>
                    <Th>Scope</Th>
                    <Th>State</Th>
                    <Th>Last delivery</Th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((i) => (
                    <Tr key={i.id} className={i.id === selected ? "bg-paper-3" : undefined}>
                      <Td label="Integration">
                        <button type="button" className="text-left font-medium hover:underline" aria-pressed={i.id === selected} onClick={() => select(i.id === selected ? null : i.id)}>
                          {i.name}
                        </button>
                        {configUrl(i) ? <div className="mono max-w-[22rem] truncate text-xs text-muted">{configUrl(i)}</div> : null}
                      </Td>
                      <Td label="Kind" className="text-ink-2">
                        {i.kind === "slack" ? "Slack" : "Webhook"}
                      </Td>
                      <Td label="Events" mono className="text-xs text-ink-2">
                        {i.events.join(", ")}
                      </Td>
                      <Td label="Scope" className="text-ink-2">
                        {projectName(i.project_id)}
                      </Td>
                      <Td label="State">{i.enabled ? <StatusPill status="ready" label="enabled" /> : <StatusPill status="disabled" />}</Td>
                      <Td label="Last delivery">
                        {i.last_delivery_at ? (
                          <span className="inline-flex items-center gap-2" title={dateTime(i.last_delivery_at)}>
                            {deliveryStatus(i.last_delivery_status, i.last_delivery_status == null ? "no response" : null)}
                            <span className="text-xs text-ink-2">{relative(i.last_delivery_at)}</span>
                          </span>
                        ) : (
                          <span className="text-muted">never</span>
                        )}
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </QueryState>
        </Section>

        {current ? <IntegrationDetail key={current.id} integration={current} onDeleted={() => select(null)} /> : null}

        <Section title="Other connections">
          <ul className="flex flex-col gap-2 text-sm text-ink-2">
            <li>
              <Link href="/settings?tab=connections" className={buttonClass("ghost", "sm")}>
                Repository connections
              </Link>{" "}
              GitHub and GitLab access for repository analysis and regression selection.
            </li>
            <li>
              <Link href="/settings?tab=ai" className={buttonClass("ghost", "sm")}>
                AI models
              </Link>{" "}
              Providers and models for planning and failure investigation. ORYN works fully without one.
            </li>
          </ul>
        </Section>
      </div>
      {isAdmin ? <CreateIntegrationDialog open={adding} onOpenChange={setAdding} /> : null}
    </>
  );
}
