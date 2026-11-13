"use client";
import Link from "next/link";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan } from "@/lib/hooks";
import { Button } from "@/components/ui/button";
import { ErrorState, Skeleton } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";

type Status = Schemas["ScenarioStatus"];

/** Bring the selected scenario into view only when the aside sits beside the editor (>= 72rem); stacked, the editor stays in view. */
function scrollWhenDocked(el: HTMLLIElement | null) {
  if (el && window.matchMedia("(min-width: 72rem)").matches) el.scrollIntoView({ block: "nearest" });
}

export function useSuiteScenarios(projectId: string, suiteId: string | null) {
  return useQuery({
    queryKey: keys.project(projectId, "scenarios", { suite_id: suiteId }),
    enabled: !!projectId && !!suiteId,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/scenarios", { params: { path: { id: projectId }, query: { suite_id: suiteId! } } })),
  });
}

/** Scenarios of one suite with their governance status and approve / draft / archive controls. */
/** `selectedId` (from `?scenario=`) is highlighted and scrolled into view. */
export function ScenarioPanel({ projectId, suiteId, selectedId }: { projectId: string; suiteId: string; selectedId?: string | null }) {
  const qc = useQueryClient();
  const canWrite = useCan("engineer");
  const scenarios = useSuiteScenarios(projectId, suiteId);
  const setStatus = useMutation({
    mutationFn: (v: { ids: string[]; status: Status }) => unwrap(api.POST("/api/v1/projects/{id}/scenarios/status", { params: { path: { id: projectId } }, body: { scenario_ids: v.ids, status: v.status } })),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.project(projectId, "scenarios") });
      qc.invalidateQueries({ queryKey: keys.project(projectId, "suites") });
    },
  });
  const pendingFor = (id: string) => setStatus.isPending && setStatus.variables?.ids.length === 1 && setStatus.variables.ids[0] === id;

  const list = scenarios.data ?? [];
  const approvable = list.filter((s) => s.executable && s.status !== "approved").map((s) => s.id);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div className="flex flex-col gap-1">
          <h3>Scenarios</h3>
          <p className="text-sm text-ink-2">Only approved scenarios run. Editing an approved scenario&rsquo;s steps returns it to draft.</p>
        </div>
      </div>
      {canWrite && approvable.length ? (
        <Button size="sm" className="self-start" loading={setStatus.isPending && setStatus.variables?.ids.length !== 1} onClick={() => setStatus.mutate({ ids: approvable, status: "approved" })}>
          Approve all executable ({approvable.length})
        </Button>
      ) : null}
      {setStatus.error ? (
        <p role="alert" className="text-sm text-error">
          {errorMessage(setStatus.error)}
        </p>
      ) : null}
      {scenarios.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-12" />
          ))}
        </div>
      ) : scenarios.error ? (
        <ErrorState error={scenarios.error} retry={() => scenarios.refetch()} />
      ) : list.length === 0 ? (
        <p className="text-sm text-muted">No scenarios yet. Add a Scenario to the feature and save.</p>
      ) : (
        <ul className="flex flex-col border-t border-rule">
          {list.map((s) => (
            <li
              key={s.id}
              ref={s.id === selectedId ? scrollWhenDocked : undefined}
              aria-current={s.id === selectedId ? "true" : undefined}
              className={s.id === selectedId ? "flex flex-col gap-2 border-b border-rule bg-paper-3 px-2 py-3 shadow-[inset_2px_0_0_var(--color-accent)]" : "flex flex-col gap-2 border-b border-rule py-3"}
            >
              <div className="flex items-start justify-between gap-2">
                <Link href={`/studio/scenarios/${s.id}?project=${projectId}`} className="min-w-0 font-medium break-words no-underline hover:underline">
                  {s.name}
                </Link>
                <StatusPill status={s.status} />
              </div>
              <div className="flex flex-wrap items-center gap-2 text-xs text-ink-2">
                <span className="mono text-muted">line {s.line}</span>
                {s.executable ? null : <span className="text-error">Unsupported steps, cannot be approved</span>}
                {s.last_status ? (
                  <span className="inline-flex items-center gap-1">
                    Last run <StatusPill status={s.last_status} />
                  </span>
                ) : null}
              </div>
              {canWrite ? (
                <div className="flex flex-wrap gap-1">
                  {s.status !== "approved" ? (
                    <Button size="sm" disabled={!s.executable} loading={pendingFor(s.id) && setStatus.variables?.status === "approved"} onClick={() => setStatus.mutate({ ids: [s.id], status: "approved" })}>
                      Approve
                    </Button>
                  ) : null}
                  {s.status !== "draft" ? (
                    <Button size="sm" variant="ghost" loading={pendingFor(s.id) && setStatus.variables?.status === "draft"} onClick={() => setStatus.mutate({ ids: [s.id], status: "draft" })}>
                      Return to draft
                    </Button>
                  ) : null}
                  {s.status !== "archived" ? (
                    <Button size="sm" variant="ghost" loading={pendingFor(s.id) && setStatus.variables?.status === "archived"} onClick={() => setStatus.mutate({ ids: [s.id], status: "archived" })}>
                      Archive
                    </Button>
                  ) : null}
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
