"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Send } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWsId } from "@/lib/hooks";
import { dateTime, duration } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Input } from "@/components/ui/field";
import { ConfirmDialog } from "@/components/ui/dialog";
import { EventPicker } from "./create-integration-dialog";

export function configUrl(i: Schemas["Integration"]): string | null {
  const u = (i.config as { url?: unknown } | null)?.url;
  return typeof u === "string" ? u : null;
}

export function deliveryStatus(code: number | null | undefined, error?: string | null) {
  if (code != null && code >= 200 && code < 300) return <StatusPill status="delivered" label={`${code}`} />;
  if (code != null) return <StatusPill status="failed" label={`${code}`} />;
  if (error) return <StatusPill status="failed" label="no response" />;
  return <span className="text-muted">pending</span>;
}

export function IntegrationDetail({ integration: i, onDeleted }: { integration: Schemas["Integration"]; onDeleted: () => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const isAdmin = useCan("admin");
  const [events, setEvents] = useState<string[]>(i.events);
  const [url, setUrl] = useState(configUrl(i) ?? "");
  const [confirm, setConfirm] = useState(false);
  const invalidate = () => qc.invalidateQueries({ queryKey: keys.ws(ws, "integrations") });

  const deliveries = useQuery({
    queryKey: keys.ws(ws, "integrations", i.id, "deliveries"),
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/integrations/{id}/deliveries", { params: { path: { ws, id: i.id } } })),
  });
  const patch = useMutation({
    mutationFn: (body: Schemas["UpdateIntegration"]) => unwrap(api.PATCH("/api/v1/workspaces/{ws}/integrations/{id}", { params: { path: { ws, id: i.id } }, body })),
    onSuccess: invalidate,
  });
  const test = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces/{ws}/integrations/{id}/test", { params: { path: { ws, id: i.id } } })),
    onSettled: () => {
      invalidate();
      qc.invalidateQueries({ queryKey: keys.ws(ws, "integrations", i.id, "deliveries") });
    },
  });
  const remove = useMutation({
    mutationFn: () => unwrap(api.DELETE("/api/v1/workspaces/{ws}/integrations/{id}", { params: { path: { ws, id: i.id } } })),
    onSuccess: async () => {
      setConfirm(false);
      await invalidate();
      onDeleted();
    },
  });

  const dirty = events.slice().sort().join() !== i.events.slice().sort().join() || (i.kind === "webhook" && url.trim() !== (configUrl(i) ?? ""));

  return (
    <Section
      title={i.name}
      description={
        <>
          {i.kind === "slack" ? "Slack incoming webhook" : "Webhook"}. Signing secret {i.has_secret ? "set" : "not set"}.
        </>
      }
      actions={
        isAdmin ? (
          <>
            <Button loading={test.isPending} onClick={() => test.mutate()}>
              <Send aria-hidden className="size-4" /> Send test
            </Button>
            <Button variant="danger" onClick={() => setConfirm(true)}>
              Delete
            </Button>
          </>
        ) : null
      }
    >
      {test.data ? (
        <p role="status" className={test.data.ok ? "text-sm text-accent-deep" : "text-sm text-error"}>
          Test delivery {test.data.ok ? "accepted" : "failed"}
          {test.data.status != null ? <span className="mono">, HTTP {test.data.status}</span> : null}
          {test.data.error ? <code className="ml-1 text-xs break-all">{test.data.error}</code> : null}
        </p>
      ) : null}
      {test.error ? <p role="alert" className="text-sm text-error">{errorMessage(test.error)}</p> : null}

      {isAdmin ? (
        <form
          className="flex max-w-2xl flex-col gap-2 border-b border-rule pb-6"
          onSubmit={(e) => {
            e.preventDefault();
            patch.mutate({ events, url: i.kind === "webhook" ? url.trim() : undefined });
          }}
        >
          <Checkbox label="Enabled" checked={i.enabled} disabled={patch.isPending} onChange={(e) => patch.mutate({ enabled: e.target.checked })} />
          {i.kind === "webhook" ? (
            <Field label="Endpoint URL">
              {(id) => <Input id={id} type="url" className="font-mono" value={url} onChange={(e) => setUrl(e.target.value)} autoComplete="off" />}
            </Field>
          ) : null}
          <EventPicker value={events} onChange={setEvents} idPrefix={i.id} />
          <div className="flex items-center gap-3">
            <Button type="submit" disabled={!dirty || events.length === 0} loading={patch.isPending}>
              Save changes
            </Button>
            {patch.error ? <p role="alert" className="text-sm text-error">{errorMessage(patch.error)}</p> : null}
          </div>
        </form>
      ) : null}

      <h3 className="mt-2">Deliveries</h3>
      <QueryState query={deliveries} empty={<Empty title="No deliveries yet.">Deliveries appear after a matching execution event, or after Send test.</Empty>}>
        {(rows) => (
          <Table>
            <thead>
              <tr>
                <Th>Time</Th>
                <Th>Event</Th>
                <Th>Response</Th>
                <Th numeric>Duration</Th>
                <Th>Error</Th>
              </tr>
            </thead>
            <tbody>
              {rows.map((d) => (
                <Tr key={d.id}>
                  <Td label="Time" mono className="text-xs whitespace-nowrap">
                    {dateTime(d.created_at)}
                  </Td>
                  <Td label="Event" mono className="text-xs">
                    {d.event}
                  </Td>
                  <Td label="Response">{deliveryStatus(d.status_code, d.error)}</Td>
                  <Td label="Duration" numeric mono>
                    {duration(d.duration_ms)}
                  </Td>
                  <Td label="Error" className="max-w-[28rem]">
                    {d.error ? <code className="text-xs break-words text-error">{d.error}</code> : <span className="text-muted">pending</span>}
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>

      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={`Delete ${i.name}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>Notifications stop at once and the delivery log is removed. This cannot be undone.</p>
            {remove.error ? <p role="alert" className="text-sm text-error">{errorMessage(remove.error)}</p> : null}
          </div>
        }
        confirmLabel="Delete integration"
        loading={remove.isPending}
        onConfirm={() => remove.mutate()}
      />
    </Section>
  );
}
