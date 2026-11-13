"use client";
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { Input } from "@/components/ui/field";
import { ErrorState, Skeleton } from "@/components/ui/states";
import { Tag } from "@/components/ui/status";

export function useStepLibrary() {
  return useQuery({ queryKey: ["step-library"], staleTime: 10 * 60_000, queryFn: () => unwrap(api.GET("/api/v1/step-library")) });
}

const words = (s: string) =>
  new Set(
    s
      .toLowerCase()
      .replace(/"[^"]*"|<[^>]*>|\{\{[^}]*\}\}/g, " ")
      .split(/[^a-z]+/)
      .filter((w) => w.length > 1 && !["i", "the", "a", "an", "to", "of", "on", "in", "with"].includes(w)),
  );

/** The library step whose wording overlaps most with an unsupported step, if any word overlaps at all. */
export function closestStep(step: string, library: Schemas["StepDef"][]): Schemas["StepDef"] | null {
  const a = words(step);
  if (!a.size) return null;
  let best: Schemas["StepDef"] | null = null;
  let score = 0;
  for (const def of library) {
    const b = words(def.pattern);
    const shared = [...a].filter((w) => b.has(w)).length;
    const s = shared / (a.size + b.size - shared || 1);
    if (s > score) {
      score = s;
      best = def;
    }
  }
  return score > 0 ? best : null;
}

/** Searchable reference of the built-in steps; activating one inserts its example at the editor cursor. */
export function StepLibrary({ onInsert }: { onInsert?: (example: string) => void }) {
  const lib = useStepLibrary();
  const [q, setQ] = useState("");
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return (lib.data ?? []).filter((d) => !needle || `${d.pattern} ${d.description} ${d.kind}`.toLowerCase().includes(needle));
  }, [lib.data, q]);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <h3>Step library</h3>
        <p className="text-sm text-ink-2">{onInsert ? "Select a step to insert its example at the cursor." : "Steps the engine can run."}</p>
      </div>
      <label className="visually-hidden" htmlFor="step-library-search">
        Search steps
      </label>
      <Input id="step-library-search" type="search" placeholder="Search steps" value={q} onChange={(e) => setQ(e.target.value)} />
      {lib.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-10" />
          ))}
        </div>
      ) : lib.error ? (
        <ErrorState error={lib.error} retry={() => lib.refetch()} />
      ) : shown.length === 0 ? (
        <p className="text-sm text-muted">No step matches &ldquo;{q}&rdquo;.</p>
      ) : (
        <ul className="flex max-h-[28rem] flex-col overflow-y-auto border-t border-rule">
          {shown.map((d) => {
            const body = (
              <>
                <span className="flex items-start justify-between gap-2">
                  <span className="mono text-sm break-words text-ink">{d.pattern}</span>
                  <Tag className="shrink-0">{d.kind}</Tag>
                </span>
                <span className="text-xs text-ink-2">{d.description}</span>
                <span className="mono text-xs break-words text-muted">{d.example}</span>
              </>
            );
            return (
              <li key={d.pattern} className="border-b border-rule">
                {onInsert ? (
                  <button type="button" className="flex w-full cursor-pointer flex-col gap-1 px-2 py-2 text-left transition-colors duration-[var(--dur-micro)] hover:bg-paper-3" onClick={() => onInsert(d.example)} aria-label={`Insert ${d.example}`}>
                    {body}
                  </button>
                ) : (
                  <div className="flex flex-col gap-1 px-2 py-2">{body}</div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
