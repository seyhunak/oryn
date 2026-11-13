"use client";
import Link from "next/link";
import { use, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useCurrentProject } from "@/lib/hooks";
import { dateTime } from "@/lib/format";
import { Meta, PageHeader, Section } from "@/components/ui/page";
import { Empty, ErrorState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Field, Textarea } from "@/components/ui/field";
import { PlanItem } from "@/components/planner/plan-item";
import { SourceTag } from "@/components/planner/source-tag";

type Risk = { area?: string; level?: string; reason?: string };

function risksOf(v: unknown): Risk[] {
  if (!Array.isArray(v)) return [];
  return v.map((r) => (typeof r === "string" ? { reason: r } : (r as Risk)));
}
function gapsOf(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v.map((g) => (typeof g === "string" ? g : JSON.stringify(g)));
}

export default function PlanPage({ params }: { params: Promise<{ planId: string }> }) {
  const { planId } = use(params);
  const { project, isPending, error, refetch } = useCurrentProject();
  const pid = project?.id ?? "";
  const canWrite = useCan("engineer");
  const [dialog, setDialog] = useState<"approve" | "reject" | null>(null);
  const [result, setResult] = useState<Schemas["ApproveResult"] | null>(null);
  const detail = useQuery({
    queryKey: keys.project(pid, "plan", planId),
    enabled: !!pid,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/plans/{plan_id}", { params: { path: { id: pid, plan_id: planId } } })),
  });

  if (isPending || (pid && detail.isPending)) return <Skeleton className="h-40" />;
  if (error) return <ErrorState error={error} retry={() => refetch()} />;
  if (!project) return <Empty title="No projects yet.">Plans belong to a project.</Empty>;
  if (detail.error || !detail.data) return <ErrorState error={detail.error} retry={() => detail.refetch()} />;

  const { plan, items } = detail.data;
  const isDraft = plan.status === "draft";
  const editable = isDraft && canWrite;
  const risks = risksOf(plan.risk_analysis);
  const gaps = gapsOf(plan.coverage_gaps);
  const accepted = items.filter((i) => i.status === "accepted");
  const rejected = items.filter((i) => i.status === "rejected");
  const chosen = accepted.length ? accepted : items.filter((i) => i.status !== "rejected");
  const willCreate = chosen.filter((i) => i.executable).length;
  const willSkip = chosen.length - willCreate;

  return (
    <>
      <PageHeader
        crumbs={[{ href: `/planner?project=${pid}`, label: "AI Test Planner" }]}
        title={plan.title}
        description={plan.objective || undefined}
        actions={
          editable ? (
            <>
              <Button size="lg" variant="danger" onClick={() => setDialog("reject")}>
                Reject plan
              </Button>
              <Button size="lg" variant="primary" onClick={() => setDialog("approve")} disabled={willCreate === 0} title={willCreate === 0 ? "No executable items to approve" : undefined}>
                <Check aria-hidden className="size-4" /> Approve plan
              </Button>
            </>
          ) : null
        }
      />

      {result ? (
        <div role="status" className="mb-6 flex flex-col gap-2 rounded-[var(--radius-card)] bg-ink p-4 text-paper">
          <p className="font-display text-[1.2rem] font-bold">
            {result.suites_created} {result.suites_created === 1 ? "suite" : "suites"} created with {result.scenarios_approved} approved {result.scenarios_approved === 1 ? "scenario" : "scenarios"}.
          </p>
          {result.skipped.length ? (
            <div className="text-paper-3">
              <p>Skipped {result.skipped.length}:</p>
              <ul className="list-disc pl-5">
                {result.skipped.map((s, i) => (
                  <li key={i}>{s}</li>
                ))}
              </ul>
            </div>
          ) : null}
          <Link href={`/studio?project=${pid}`} className="self-start text-paper underline">
            Open Test Studio
          </Link>
        </div>
      ) : isDraft ? (
        <p className="mb-6 rounded-[var(--radius-input)] border border-rule bg-paper-2 p-3 text-sm text-ink-2">
          This plan is a proposal. Nothing runs and nothing is added to suites until a person approves it. On approval, each{" "}
          {accepted.length ? "accepted" : "item that is not rejected"} becomes its own suite with approved scenarios; items with unsupported steps are skipped.
        </p>
      ) : null}

      <div className="grid grid-cols-1 gap-8 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="order-2 flex min-w-0 flex-col gap-8 min-[72rem]:order-1">
          <Section title="Proposed scenarios" description={`${items.length} items. Accept the ones to keep, or leave all unmarked to keep every item that is not rejected.`}>
            {items.length === 0 ? (
              <Empty title="This plan has no items." />
            ) : (
              <ul className="flex flex-col">
                {items.map((it) => (
                  <PlanItem key={it.id} projectId={pid} planId={planId} item={it} source={plan.source} editable={editable} />
                ))}
              </ul>
            )}
          </Section>
        </div>

        <aside className="order-1 flex min-w-0 flex-col gap-6 min-[72rem]:order-2">
          <Meta
            items={[
              { label: "Status", value: <StatusPill status={plan.status} label={isDraft ? "awaiting review" : undefined} /> },
              { label: "Source", value: <SourceTag source={plan.source} /> },
              { label: "Items", value: <span className="mono">{items.length}</span> },
              { label: "Accepted", value: <span className="mono">{accepted.length}</span> },
              { label: "Rejected", value: <span className="mono">{rejected.length}</span> },
              { label: "Executable", value: <span className="mono">{items.filter((i) => i.executable).length}</span> },
              { label: "Created", value: <span className="mono text-xs">{dateTime(plan.created_at)}</span> },
              ...(plan.approved_at ? [{ label: "Approved", value: <span className="mono text-xs">{dateTime(plan.approved_at)}</span> }] : []),
            ]}
          />
          {plan.rationale ? (
            <div className="flex flex-col gap-2">
              <h3>Rationale</h3>
              <p className="text-sm text-ink-2">{plan.rationale}</p>
            </div>
          ) : null}
          {plan.input ? (
            <div className="flex flex-col gap-2">
              <h3>Instructions</h3>
              <p className="text-sm whitespace-pre-wrap text-ink-2">{plan.input}</p>
            </div>
          ) : null}
          <div className="flex flex-col gap-2">
            <h3>Risks</h3>
            {risks.length ? (
              <ul className="flex flex-col gap-2 text-sm">
                {risks.map((r, i) => (
                  <li key={i} className="flex flex-col gap-1 border-b border-rule pb-2">
                    <span className="flex flex-wrap items-center gap-2">
                      {r.area ? <span className="font-medium">{r.area}</span> : null}
                      {r.level ? <Tag>{r.level}</Tag> : null}
                    </span>
                    {r.reason ? <span className="text-ink-2">{r.reason}</span> : null}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-sm text-muted">No risks recorded.</p>
            )}
          </div>
          <div className="flex flex-col gap-2">
            <h3>Coverage gaps</h3>
            {gaps.length ? (
              <ul className="flex list-disc flex-col gap-1 pl-5 text-sm text-ink-2">
                {gaps.map((g, i) => (
                  <li key={i}>{g}</li>
                ))}
              </ul>
            ) : (
              <p className="text-sm text-muted">No gaps recorded.</p>
            )}
          </div>
        </aside>
      </div>

      <DecisionDialog
        kind={dialog}
        onClose={() => setDialog(null)}
        projectId={pid}
        planId={planId}
        willCreate={willCreate}
        willSkip={willSkip}
        onApproved={(r) => {
          setResult(r);
          setDialog(null);
        }}
      />
    </>
  );
}

function DecisionDialog({
  kind,
  onClose,
  projectId,
  planId,
  willCreate,
  willSkip,
  onApproved,
}: {
  kind: "approve" | "reject" | null;
  onClose: () => void;
  projectId: string;
  planId: string;
  willCreate: number;
  willSkip: number;
  onApproved: (r: Schemas["ApproveResult"]) => void;
}) {
  const qc = useQueryClient();
  const [comment, setComment] = useState("");
  const done = () => {
    qc.invalidateQueries({ queryKey: keys.project(projectId, "plan", planId) });
    qc.invalidateQueries({ queryKey: keys.project(projectId, "plans") });
  };
  const approve = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/plans/{plan_id}/approve", { params: { path: { id: projectId, plan_id: planId } }, body: { comment: comment.trim() || null } })),
    onSuccess: (r) => {
      done();
      qc.invalidateQueries({ queryKey: keys.project(projectId, "suites") });
      qc.invalidateQueries({ queryKey: keys.project(projectId, "scenarios") });
      onApproved(r);
    },
  });
  const reject = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/plans/{plan_id}/reject", { params: { path: { id: projectId, plan_id: planId } }, body: { comment: comment.trim() || null } })),
    onSuccess: () => {
      done();
      onClose();
    },
  });
  const m = kind === "reject" ? reject : approve;
  return (
    <Dialog
      open={!!kind}
      onOpenChange={(o) => {
        if (!o) {
          approve.reset();
          reject.reset();
          onClose();
        }
      }}
      title={kind === "reject" ? "Reject plan" : "Approve plan"}
      description={
        kind === "reject"
          ? "Nothing is added to suites. The plan stays on record as rejected."
          : `${willCreate} ${willCreate === 1 ? "item becomes a suite" : "items become suites"} with approved scenarios that can run.${willSkip ? ` ${willSkip} with unsupported steps will be skipped.` : ""}`
      }
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          {kind === "reject" ? (
            <Button variant="danger-solid" loading={reject.isPending} onClick={() => reject.mutate()}>
              Reject plan
            </Button>
          ) : (
            <Button variant="primary" loading={approve.isPending} onClick={() => approve.mutate()}>
              Approve and create suites
            </Button>
          )}
        </>
      }
    >
      <Field label="Comment (optional)" hint="Stored with the approval record.">
        {(id, d) => <Textarea id={id} aria-describedby={d} rows={3} value={comment} onChange={(e) => setComment(e.target.value)} className="h-auto py-2" />}
      </Field>
      {m.error ? (
        <p role="alert" className="text-sm text-error">
          {errorMessage(m.error)}
        </p>
      ) : null}
      {kind === "approve" ? (
        <p className="text-sm text-ink-2">
          Approved suites appear in <Link href={`/studio?project=${projectId}`}>Test Studio</Link>. Nothing runs until someone starts a run.
        </p>
      ) : null}
    </Dialog>
  );
}
