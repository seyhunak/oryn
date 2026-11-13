"use client";
import Link from "next/link";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, Pencil, X } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { Button } from "@/components/ui/button";
import { StatusPill, Tag } from "@/components/ui/status";
import { GherkinEditor } from "@/components/studio/gherkin-editor";
import { SourceTag } from "./source-tag";

function diagnosticsOf(item: Schemas["TestPlanItem"]): string[] {
  const d = item.diagnostics as unknown;
  if (!Array.isArray(d)) return [];
  return d.map((x) => (typeof x === "string" ? x : x && typeof x === "object" && "message" in x ? String((x as { message: unknown }).message) : JSON.stringify(x)));
}

/** One proposed scenario: its Gherkin, whether it compiles, and the reviewer's accept / reject decision. */
export function PlanItem({ projectId, planId, item, source, editable }: { projectId: string; planId: string; item: Schemas["TestPlanItem"]; source: string; editable: boolean }) {
  const qc = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(item.gherkin);
  const update = useMutation({
    mutationFn: (body: Schemas["UpdateItem"]) => unwrap(api.PATCH("/api/v1/projects/{id}/plans/{plan_id}/items/{item_id}", { params: { path: { id: projectId, plan_id: planId, item_id: item.id } }, body })),
    onSuccess: (updated, body) => {
      qc.setQueryData<Schemas["PlanDetail"]>(keys.project(projectId, "plan", planId), (d) => (d ? { ...d, items: d.items.map((i) => (i.id === updated.id ? updated : i)) } : d));
      if (body.gherkin != null) setEditing(false);
    },
  });
  const diags = diagnosticsOf(item);
  const busy = (k: "accepted" | "rejected" | "proposed" | "gherkin") => update.isPending && (k === "gherkin" ? update.variables?.gherkin != null : update.variables?.status === k);

  return (
    <li className="flex flex-col gap-3 border-b border-rule py-6 first:pt-0">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <h3 className="break-words">
            <span className="mono mr-2 text-muted">{item.position + 1}.</span>
            {item.title}
          </h3>
          <div className="flex flex-wrap items-center gap-1">
            <SourceTag source={source} />
            <Tag>{item.category}</Tag>
            <Tag>{item.priority} priority</Tag>
            {item.requirement_keys.map((k) => (
              <Tag key={k} className="mono">
                {k}
              </Tag>
            ))}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {item.executable ? <StatusPill status="ready" label="executable" /> : <StatusPill status="failed" label="unsupported steps" />}
          <StatusPill status={item.status} />
        </div>
      </div>

      {item.rationale || item.risk ? (
        <dl className="grid grid-cols-[minmax(0,auto)_minmax(0,1fr)] gap-x-4 gap-y-1 text-sm">
          {item.risk ? (
            <>
              <dt className="text-muted">Risk</dt>
              <dd className="m-0 text-ink-2">{item.risk}</dd>
            </>
          ) : null}
          {item.rationale ? (
            <>
              <dt className="text-muted">Why</dt>
              <dd className="m-0 text-ink-2">{item.rationale}</dd>
            </>
          ) : null}
        </dl>
      ) : null}

      {editing ? (
        <div className="flex flex-col gap-2">
          <GherkinEditor value={draft} onChange={setDraft} height={260} label={`Gherkin for ${item.title}`} />
          <p className="text-xs text-muted">Saving re-validates the steps against the step library.</p>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" loading={busy("gherkin")} disabled={draft === item.gherkin} onClick={() => update.mutate({ gherkin: draft })}>
              Save Gherkin
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setDraft(item.gherkin);
                setEditing(false);
                update.reset();
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <pre className="m-0 overflow-x-auto rounded-[var(--radius-input)] bg-paper-2 p-3 text-sm leading-relaxed text-ink">{item.gherkin}</pre>
      )}

      {diags.length ? (
        <ul className="flex flex-col gap-1 text-sm text-error">
          {diags.map((d, i) => (
            <li key={i} className="break-words">
              {d}
            </li>
          ))}
        </ul>
      ) : null}
      {!item.executable ? <p className="text-sm text-ink-2">This item will be skipped on approval until every step matches the step library.</p> : null}
      {item.scenario_id ? (
        <p className="text-sm">
          <Link href={`/studio/scenarios/${item.scenario_id}?project=${projectId}`}>Open the approved scenario</Link>
        </p>
      ) : null}

      {update.error ? (
        <p role="alert" className="text-sm text-error">
          {errorMessage(update.error)}
        </p>
      ) : null}

      {editable && !editing ? (
        <div className="flex flex-wrap gap-2">
          <Button size="sm" aria-pressed={item.status === "accepted"} loading={busy(item.status === "accepted" ? "proposed" : "accepted")} onClick={() => update.mutate({ status: item.status === "accepted" ? "proposed" : "accepted" })}>
            <Check aria-hidden className="size-3.5" /> Accept
          </Button>
          <Button size="sm" aria-pressed={item.status === "rejected"} loading={busy(item.status === "rejected" ? "proposed" : "rejected")} onClick={() => update.mutate({ status: item.status === "rejected" ? "proposed" : "rejected" })}>
            <X aria-hidden className="size-3.5" /> Reject
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>
            <Pencil aria-hidden className="size-3.5" /> Edit Gherkin
          </Button>
        </div>
      ) : null}
    </li>
  );
}
