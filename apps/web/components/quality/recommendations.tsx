"use client";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RotateCw } from "lucide-react";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { keys, useCan } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { AiTag, Tag } from "@/components/ui/status";
import { Button } from "@/components/ui/button";

const DAYS = 30;

function confidenceOf(related: unknown): number | null {
  const c = (related as { confidence?: unknown } | null)?.confidence;
  return typeof c === "number" ? c : null;
}

export function RecommendationsTab({ projectId }: { projectId: string }) {
  const qc = useQueryClient();
  const canWrite = useCan("engineer");
  const quality = useQuery({
    queryKey: keys.project(projectId, "quality", DAYS),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/quality", { params: { path: { id: projectId }, query: { days: DAYS } } })),
  });
  const refresh = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/recommendations/refresh", { params: { path: { id: projectId } } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.project(projectId, "quality") }),
  });
  const dismiss = useMutation({
    mutationFn: (rid: string) => unwrap(api.POST("/api/v1/projects/{id}/recommendations/{rid}/dismiss", { params: { path: { id: projectId, rid } } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.project(projectId, "quality") }),
  });

  return (
    <Section
      title="Recommendations"
      description="Produced by deterministic rules over recorded runs, coverage and flakiness. Anything a model suggested is labelled as AI interpretation."
      actions={
        canWrite ? (
          <Button loading={refresh.isPending} onClick={() => refresh.mutate()}>
            <RotateCw aria-hidden className="size-4" /> Refresh
          </Button>
        ) : null
      }
    >
      {refresh.error ? <p role="alert" className="text-sm text-error">{errorMessage(refresh.error)}</p> : null}
      <QueryState
        query={quality}
        skeleton={
          <div className="flex flex-col gap-3">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-16" />
            ))}
          </div>
        }
        isEmpty={(q) => q.recommendations.length === 0}
        empty={
          <Empty title="No open recommendations." action={canWrite ? <Button onClick={() => refresh.mutate()} loading={refresh.isPending}>Refresh recommendations</Button> : undefined}>
            Refreshing re-evaluates the rules against the latest runs.
          </Empty>
        }
      >
        {(q) => (
          <ul className="flex flex-col">
            {q.recommendations.map((r) => {
              const ai = r.source !== "heuristic";
              return (
                <li key={r.id} className="flex flex-col gap-2 border-b border-rule py-4 sm:flex-row sm:items-start sm:justify-between">
                  <div className={ai ? "flex min-w-0 flex-col gap-1 rounded-[var(--radius-input)] bg-paper-2 p-3" : "flex min-w-0 flex-col gap-1"}>
                    <div className="flex flex-wrap items-center gap-2">
                      <p className="font-medium">{r.title}</p>
                      <Tag>{r.kind.replace(/_/g, " ")}</Tag>
                      {ai ? <AiTag confidence={confidenceOf(r.related)} /> : <Tag>rule</Tag>}
                    </div>
                    <p className="max-w-[80ch] text-sm whitespace-pre-line text-ink-2">{r.body}</p>
                    <span className="text-xs text-muted">Raised {relative(r.created_at)}</span>
                    {dismiss.error && dismiss.variables === r.id ? <p role="alert" className="text-sm text-error">{errorMessage(dismiss.error)}</p> : null}
                  </div>
                  {canWrite ? (
                    <Button size="sm" variant="ghost" loading={dismiss.isPending && dismiss.variables === r.id} onClick={() => dismiss.mutate(r.id)}>
                      Dismiss
                    </Button>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </QueryState>
    </Section>
  );
}
