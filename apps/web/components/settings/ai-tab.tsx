"use client";
import { useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Zap } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWsId } from "@/lib/hooks";
import { duration } from "@/lib/format";
import { Section, Stat } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Input } from "@/components/ui/field";
import { ConfirmDialog } from "@/components/ui/dialog";
import { EditProviderDialog, NewProviderDialog, kindLabel } from "./ai-provider-dialogs";

const usd = (n: number) => `$${Math.abs(n) < 0.005 && n !== 0 ? n.toFixed(4) : n.toFixed(2)}`;
const price = (n: number) => `$${n.toFixed(2)}`;
const num = (n: number) => n.toLocaleString();

function AddModelForm({ provider }: { provider: Schemas["ProviderView"] }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [inCost, setInCost] = useState("");
  const [outCost, setOutCost] = useState("");
  const [ctx, setCtx] = useState("");
  const [isDefault, setIsDefault] = useState(false);
  const add = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/workspaces/{ws}/ai/providers/{pid}/models", {
          params: { path: { ws, pid: provider.id } },
          body: { name: name.trim(), input_cost_per_mtok: Number(inCost) || 0, output_cost_per_mtok: Number(outCost) || 0, context_window: ctx ? Number(ctx) : null, is_default: isDefault },
        }),
      ),
    onSuccess: () => {
      setName("");
      setInCost("");
      setOutCost("");
      setCtx("");
      setIsDefault(false);
      qc.invalidateQueries({ queryKey: keys.ws(ws, "ai") });
    },
  });
  return (
    <form
      className="flex flex-wrap items-end gap-x-3 gap-y-0"
      aria-label={`Add a model to ${provider.name}`}
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim()) add.mutate();
      }}
    >
      <Field label="Model name" className="w-56" error={add.error ? errorMessage(add.error) : null}>
        {(id, d) => <Input id={id} aria-describedby={d} className="font-mono" value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" />}
      </Field>
      <Field label="Input $ / MTok" className="w-32">
        {(id) => <Input id={id} inputMode="decimal" className="font-mono" value={inCost} onChange={(e) => setInCost(e.target.value)} placeholder="0" />}
      </Field>
      <Field label="Output $ / MTok" className="w-32">
        {(id) => <Input id={id} inputMode="decimal" className="font-mono" value={outCost} onChange={(e) => setOutCost(e.target.value)} placeholder="0" />}
      </Field>
      <Field label="Context window" className="w-36">
        {(id) => <Input id={id} inputMode="numeric" className="font-mono" value={ctx} onChange={(e) => setCtx(e.target.value.replace(/\D/g, ""))} placeholder="tokens" />}
      </Field>
      <div className="flex min-h-[calc(36px+1lh+4px+1.25rem)] flex-col justify-start gap-1 pt-5">
        <Checkbox label="Default" checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)} />
      </div>
      <div className="flex min-h-[calc(36px+1lh+4px)] flex-col">
        <Button type="submit" size="sm" loading={add.isPending} disabled={!name.trim()}>
          Add model
        </Button>
      </div>
    </form>
  );
}

function ProviderBlock({ p, onEdit, onDelete }: { p: Schemas["ProviderView"]; onEdit: () => void; onDelete: () => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const canConfigure = useCan("admin");
  const invalidate = () => qc.invalidateQueries({ queryKey: keys.ws(ws, "ai") });
  const patch = useMutation({
    mutationFn: (body: Schemas["UpdateProvider"]) => unwrap(api.PATCH("/api/v1/workspaces/{ws}/ai/providers/{pid}", { params: { path: { ws, pid: p.id } }, body })),
    onSuccess: invalidate,
  });
  const test = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces/{ws}/ai/providers/{pid}/test", { params: { path: { ws, pid: p.id } } })),
    onSettled: () => qc.invalidateQueries({ queryKey: keys.ws(ws, "ai", "usage") }),
  });
  const removeModel = useMutation({
    mutationFn: (mid: string) => unwrap(api.DELETE("/api/v1/workspaces/{ws}/ai/providers/{pid}/models/{mid}", { params: { path: { ws, pid: p.id, mid } } })),
    onSuccess: invalidate,
  });
  const keyNeeded = p.kind !== "mock" && p.kind !== "ollama";

  return (
    <article className="panel flex flex-col gap-4 p-4" aria-label={p.name}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3>{p.name}</h3>
            {p.enabled ? <StatusPill status="ready" label="enabled" /> : <StatusPill status="disabled" />}
            {p.is_default ? <Tag>default</Tag> : null}
          </div>
          <p className="text-sm text-ink-2">
            {kindLabel(p.kind)}
            {keyNeeded ? <>, API key {p.has_api_key ? "set" : "not set"}</> : null}
            {p.base_url ? (
              <>
                , <span className="mono text-xs">{p.base_url}</span>
              </>
            ) : null}
            {p.api_version ? (
              <>
                , API version <span className="mono text-xs">{p.api_version}</span>
              </>
            ) : null}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {canConfigure ? (
            <Button size="sm" loading={test.isPending} onClick={() => test.mutate()}>
              <Zap aria-hidden className="size-3.5" /> Test connection
            </Button>
          ) : null}
          {canConfigure ? (
            <>
              <Button size="sm" variant="ghost" onClick={onEdit}>
                Edit
              </Button>
              <Button size="sm" variant="danger" onClick={onDelete}>
                Delete
              </Button>
            </>
          ) : null}
        </div>
      </div>

      {test.data ? (
        <div role="status" className={test.data.ok ? "text-sm text-ink" : "text-sm text-error"}>
          <span className="mr-2 inline-flex align-middle">{test.data.ok ? <StatusPill status="passed" label="connected" /> : <StatusPill status="failed" label="failed" />}</span>
          <span className="mono text-xs">
            {test.data.model}, {duration(test.data.latency_ms)}, {num(test.data.input_tokens)} in / {num(test.data.output_tokens)} out, {usd(test.data.cost_usd)}
          </span>
          {test.data.message ? <code className="mt-1 block text-xs break-words text-ink-2">{test.data.message}</code> : null}
        </div>
      ) : null}
      {test.error ? <p role="alert" className="text-sm text-error">{errorMessage(test.error)}</p> : null}

      {canConfigure ? (
        <div className="flex flex-wrap gap-x-6">
          <Checkbox label="Enabled" checked={p.enabled} disabled={patch.isPending} onChange={(e) => patch.mutate({ enabled: e.target.checked })} />
          <Checkbox label="Workspace default" checked={p.is_default} disabled={patch.isPending || p.is_default} onChange={(e) => patch.mutate({ is_default: e.target.checked })} />
          {patch.error ? <p role="alert" className="text-sm text-error">{errorMessage(patch.error)}</p> : null}
        </div>
      ) : null}

      {p.models.length === 0 ? (
        <p className="text-sm text-muted">No models. Add one below to use this provider.</p>
      ) : (
        <Table>
          <thead>
            <tr>
              <Th>Model</Th>
              <Th numeric>Input $ / MTok</Th>
              <Th numeric>Output $ / MTok</Th>
              <Th numeric>Context window</Th>
              <Th>Notes</Th>
              {canConfigure ? (
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              ) : null}
            </tr>
          </thead>
          <tbody>
            {p.models.map((m) => (
              <Tr key={m.id}>
                <Td label="Model" mono>
                  {m.name}
                </Td>
                <Td label="Input $ / MTok" numeric mono>
                  {price(m.input_cost_per_mtok)}
                </Td>
                <Td label="Output $ / MTok" numeric mono>
                  {price(m.output_cost_per_mtok)}
                </Td>
                <Td label="Context window" numeric mono>
                  {m.context_window != null ? num(m.context_window) : "–"}
                </Td>
                <Td label="Notes">
                  <span className="flex flex-wrap gap-1">
                    {m.is_default ? <Tag>default</Tag> : null}
                    {m.supports_json ? <Tag>JSON mode</Tag> : null}
                  </span>
                </Td>
                {canConfigure ? (
                  <Td label="Actions" className="text-right">
                    <Button size="sm" variant="ghost" loading={removeModel.isPending && removeModel.variables === m.id} onClick={() => removeModel.mutate(m.id)} aria-label={`Remove model ${m.name}`}>
                      Remove
                    </Button>
                  </Td>
                ) : null}
              </Tr>
            ))}
          </tbody>
        </Table>
      )}
      {removeModel.error ? <p role="alert" className="text-sm text-error">{errorMessage(removeModel.error)}</p> : null}
      {canConfigure ? <AddModelForm provider={p} /> : null}
    </article>
  );
}

function UsageSection() {
  const ws = useWsId();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const days = Number(sp.get("days") ?? "30");
  const usage = useQuery({
    queryKey: keys.ws(ws, "ai", "usage", days),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/ai/usage", { params: { path: { ws }, query: { days } } })),
  });
  const setDays = (d: string) => {
    const q = new URLSearchParams(sp.toString());
    q.set("days", d);
    router.replace(`${path}?${q.toString()}`, { scroll: false });
  };
  return (
    <Section
      title="Usage"
      description="Requests, tokens and cost recorded for every model call. Cost is computed from the per-model prices."
      actions={
        <label className="flex items-center gap-2 text-sm text-ink-2">
          Window
          <select className="field h-9 w-auto" value={String(days)} onChange={(e) => setDays(e.target.value)}>
            {[7, 30, 90].map((d) => (
              <option key={d} value={d}>
                {d} days
              </option>
            ))}
          </select>
        </label>
      }
    >
      <QueryState query={usage} skeleton={<Skeleton className="h-32" />}>
        {(u) => (
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-2 gap-x-6 gap-y-4 border-y border-rule py-4 md:grid-cols-4">
              <Stat label="Requests" value={num(u.total_requests)} />
              <Stat label="Input tokens" value={num(u.total_input_tokens)} />
              <Stat label="Output tokens" value={num(u.total_output_tokens)} />
              <Stat label="Cost" value={usd(u.total_cost_usd)} hint={`last ${u.days} days`} />
            </div>
            {u.rows.length === 0 ? (
              <Empty title={`No model calls in the last ${u.days} days.`}>Usage appears when the planner or a failure investigation calls a model.</Empty>
            ) : (
              <Table>
                <thead>
                  <tr>
                    <Th>Purpose</Th>
                    <Th>Model</Th>
                    <Th numeric>Requests</Th>
                    <Th numeric>Failed</Th>
                    <Th numeric>Input tokens</Th>
                    <Th numeric>Output tokens</Th>
                    <Th numeric>Cost</Th>
                  </tr>
                </thead>
                <tbody>
                  {u.rows.map((r) => (
                    <Tr key={`${r.purpose}-${r.provider_kind}-${r.model}`}>
                      <Td label="Purpose">{r.purpose.replace(/_/g, " ")}</Td>
                      <Td label="Model">
                        <span className="mono text-xs">{r.model}</span>
                        <div className="text-xs text-muted">{kindLabel(r.provider_kind)}</div>
                      </Td>
                      <Td label="Requests" numeric mono>
                        {num(r.requests)}
                      </Td>
                      <Td label="Failed" numeric mono className={r.failures ? "text-error" : undefined}>
                        {num(r.failures)}
                      </Td>
                      <Td label="Input tokens" numeric mono>
                        {num(r.input_tokens)}
                      </Td>
                      <Td label="Output tokens" numeric mono>
                        {num(r.output_tokens)}
                      </Td>
                      <Td label="Cost" numeric mono>
                        {usd(r.cost_usd)}
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </div>
        )}
      </QueryState>
    </Section>
  );
}

export function AiTab() {
  const ws = useWsId();
  const qc = useQueryClient();
  const canConfigure = useCan("admin");
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Schemas["ProviderView"] | null>(null);
  const [deleting, setDeleting] = useState<Schemas["ProviderView"] | null>(null);
  const providers = useQuery({
    queryKey: keys.ws(ws, "ai", "providers"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/ai/providers", { params: { path: { ws } } })),
  });
  const remove = useMutation({
    mutationFn: (pid: string) => unwrap(api.DELETE("/api/v1/workspaces/{ws}/ai/providers/{pid}", { params: { path: { ws, pid } } })),
    onSuccess: () => {
      setDeleting(null);
      qc.invalidateQueries({ queryKey: keys.ws(ws, "ai") });
    },
  });
  const addButton = canConfigure ? (
    <Button variant="primary" onClick={() => setAdding(true)}>
      <Plus aria-hidden className="size-4" /> Add provider
    </Button>
  ) : null;

  return (
    <div className="flex flex-col gap-10">
      <Section
        title="AI models"
        description="ORYN works fully without any provider: authoring, running, evidence, verdicts and quality figures never depend on a model. A provider adds test planning and failure interpretation, always labelled as AI interpretation."
        actions={addButton}
      >
        <QueryState
          query={providers}
          skeleton={<Skeleton className="h-40" />}
          empty={
            <Empty title="No AI provider configured." action={addButton}>
              Everything except AI planning and AI failure interpretation works as it is.
            </Empty>
          }
        >
          {(list) => (
            <div className="flex flex-col gap-4">
              {list.map((p) => (
                <ProviderBlock key={p.id} p={p} onEdit={() => setEditing(p)} onDelete={() => setDeleting(p)} />
              ))}
            </div>
          )}
        </QueryState>
      </Section>
      <UsageSection />
      {canConfigure ? (
        <>
          <NewProviderDialog open={adding} onOpenChange={setAdding} />
          <EditProviderDialog key={editing?.id ?? "none"} provider={editing} onOpenChange={(o) => !o && setEditing(null)} />
        </>
      ) : null}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            remove.reset();
          }
        }}
        title={`Delete ${deleting?.name ?? "provider"}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>The provider, its models and its stored key are removed. This cannot be undone.</p>
            {remove.error ? <p role="alert" className="text-sm text-error">{errorMessage(remove.error)}</p> : null}
          </div>
        }
        confirmLabel="Delete provider"
        loading={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting.id)}
      />
    </div>
  );
}
