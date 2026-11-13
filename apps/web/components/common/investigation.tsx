"use client";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Sparkles } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { dateTime } from "@/lib/format";
import { AiTag, Tag } from "@/components/ui/status";
import { Button } from "@/components/ui/button";

type Observation = { kind: string; source: string; detail: string };
type Interpretation = { source: string; model: string; label: string; output: { summary: string; probable_root_cause: string; category: string; confidence: number; suggested_investigation: string[]; suggested_remediation: string[] } };

const list = (v: unknown): string[] => (Array.isArray(v) ? v.map(String) : []);

/**
 * Observations (what the evidence shows) are set apart from interpretation (what it may
 * mean). The AI part is labelled, carries its model and confidence, and never changes
 * the verdict.
 */
export function InvestigationView({ inv, jobId, canRun }: { inv: Schemas["Investigation"] | undefined; jobId: string; canRun: boolean }) {
  const qc = useQueryClient();
  const run = useMutation({
    mutationFn: (use_ai: boolean) => unwrap(api.POST("/api/v1/jobs/{job_id}/investigate", { params: { path: { job_id: jobId } }, body: { use_ai } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["job", jobId] }),
  });

  if (!inv) {
    return (
      <div className="flex flex-col items-start gap-3">
        <p className="text-ink-2">No investigation for this job yet.</p>
        {canRun ? (
          <Button loading={run.isPending} onClick={() => run.mutate(false)}>
            Investigate
          </Button>
        ) : null}
        {run.error ? <p role="alert" className="text-sm text-error">{errorMessage(run.error)}</p> : null}
      </div>
    );
  }
  const obs = (Array.isArray(inv.observations) ? inv.observations : []) as Observation[];
  const ai = (inv.interpretation as unknown as Interpretation | null) ?? null;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Tag>{inv.category.replace(/_/g, " ")}</Tag>
          <span className="mono text-xs text-muted">rule confidence {Math.round(inv.confidence * 100)}%</span>
          <span className="text-xs text-muted">{dateTime(inv.created_at)}</span>
        </div>
        <p className="text-[1rem] font-medium">{inv.summary}</p>
        <p className="text-ink-2">{inv.probable_root_cause}</p>
      </div>

      <div className="flex flex-col gap-2">
        <h3>Observations</h3>
        <p className="text-xs text-muted">Extracted by deterministic rules from steps, logs and network records.</p>
        <ul className="flex flex-col">
          {obs.map((o, i) => (
            <li key={i} className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)] gap-3 border-b border-rule py-2 text-sm">
              <span className="text-muted">{o.kind.replace(/_/g, " ")}</span>
              <span className="min-w-0 break-words">
                <code className="text-xs">{o.detail}</code>
                <span className="block text-xs text-muted">{o.source}</span>
              </span>
            </li>
          ))}
        </ul>
      </div>

      <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
        <Checklist title="What to check" items={list(inv.suggested_investigation)} />
        <Checklist title="How to fix" items={list(inv.suggested_remediation)} />
      </div>

      {ai ? (
        <div className="flex flex-col gap-3 rounded-[var(--radius-card)] bg-paper-2 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <AiTag confidence={ai.output.confidence} />
            <span className="mono text-xs text-muted">{ai.model}</span>
          </div>
          <p>{ai.output.summary}</p>
          <p className="text-ink-2">{ai.output.probable_root_cause}</p>
          <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
            <Checklist title="Suggested checks" items={ai.output.suggested_investigation} />
            <Checklist title="Suggested fixes" items={ai.output.suggested_remediation} />
          </div>
          <p className="text-xs text-muted">An interpretation. It does not change the verdict, which comes from the recorded assertions.</p>
        </div>
      ) : canRun ? (
        <div className="flex flex-col items-start gap-2">
          <Button loading={run.isPending} onClick={() => run.mutate(true)}>
            <Sparkles aria-hidden className="size-4" /> Ask the model for an interpretation
          </Button>
          <p className="text-xs text-muted">Sends the observations above, never raw artifacts or secrets, to the workspace&apos;s configured model.</p>
          {run.error ? <p role="alert" className="text-sm text-error">{errorMessage(run.error)}</p> : null}
        </div>
      ) : null}
    </div>
  );
}

function Checklist({ title, items }: { title: string; items: string[] }) {
  if (!items.length) return null;
  return (
    <div className="flex flex-col gap-2">
      <h3>{title}</h3>
      <ol className="flex list-decimal flex-col gap-1 pl-5 text-sm">
        {items.map((s, i) => (
          <li key={i}>{s}</li>
        ))}
      </ol>
    </div>
  );
}
