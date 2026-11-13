"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Field, Input, Textarea } from "@/components/ui/field";
import { Skeleton } from "@/components/ui/states";
import { cn } from "@/lib/cn";

const MODES: { id: Schemas["PlanMode"]; label: string; body: string }[] = [
  { id: "auto", label: "Auto", body: "Uses the workspace's AI model when one is configured, otherwise the heuristic planner." },
  { id: "heuristic", label: "Heuristic", body: "No AI. Derives scenarios from acceptance criteria that name a page or endpoint, and from detected routes." },
  { id: "ai", label: "AI", body: "Uses the model configured for this workspace. Fails if none is configured." },
];

/** `requirement` (an id or a key, e.g. from `/planner?requirement=US-3`) is preselected as plan input. */
export function NewPlanDialog({ projectId, open, onOpenChange, requirement }: { projectId: string; open: boolean; onOpenChange: (o: boolean) => void; requirement?: string | null }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} wide title="New test plan" description="The planner proposes scenarios for review. Nothing runs and nothing is added to suites until a person approves the plan.">
      {open ? <NewPlanForm projectId={projectId} requirement={requirement ?? null} onCancel={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function NewPlanForm({ projectId, requirement, onCancel }: { projectId: string; requirement: string | null; onCancel: () => void }) {
  const router = useRouter();
  const qc = useQueryClient();
  const [mode, setMode] = useState<Schemas["PlanMode"]>("auto");
  // null until the reviewer changes the selection; until then the linked requirement (if any) is preselected.
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const [instructions, setInstructions] = useState("");
  const [max, setMax] = useState("");
  const reqs = useQuery({
    queryKey: keys.project(projectId, "requirements"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/requirements", { params: { path: { id: projectId } } })),
  });
  const preset = requirement ? reqs.data?.find((r) => r.id === requirement || r.key.toLowerCase() === requirement.toLowerCase()) : undefined;
  const selected = picked ?? new Set(preset ? [preset.id] : []);
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/projects/{id}/plans", {
          params: { path: { id: projectId } },
          body: { mode, requirement_ids: [...selected], instructions: instructions.trim(), max_scenarios: max ? Number(max) : null },
        }),
      ),
    onSuccess: (d) => {
      qc.invalidateQueries({ queryKey: keys.project(projectId, "plans") });
      router.push(`/planner/${d.plan.id}?project=${projectId}`);
    },
  });
  const toggle = (id: string) =>
    setPicked(() => {
      const n = new Set(selected);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate();
      }}
    >
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-sm font-medium text-ink-2">Planner</legend>
        {MODES.map((m) => (
          <label
            key={m.id}
            className={cn(
              "flex cursor-pointer items-start gap-3 rounded-[var(--radius-input)] border border-rule p-3 transition-colors duration-[var(--dur-micro)] hover:bg-paper-3",
              mode === m.id && "border-accent bg-accent-soft hover:bg-accent-soft",
            )}
          >
            <input type="radio" name="mode" value={m.id} checked={mode === m.id} onChange={() => setMode(m.id)} className="mt-1 size-4 accent-[var(--color-accent)]" />
            <span className="flex flex-col gap-0.5">
              <span className="font-medium text-ink">{m.label}</span>
              <span className="text-sm text-ink-2">{m.body}</span>
            </span>
          </label>
        ))}
      </fieldset>

      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-sm font-medium text-ink-2">Requirements</legend>
        <p className="text-sm text-muted">{selected.size ? `${selected.size} selected.` : "None selected: the plan covers every requirement."}</p>
        {requirement && reqs.data && !preset ? <p className="text-sm text-error">Requirement {requirement} was not found in this project.</p> : null}
        {reqs.isPending ? (
          <Skeleton className="h-24" />
        ) : reqs.error ? (
          <p role="alert" className="text-sm text-error">
            Requirements could not be loaded: <code>{errorMessage(reqs.error)}</code>
          </p>
        ) : reqs.data.length === 0 ? (
          <p className="text-sm text-ink-2">This project has no requirements yet. The heuristic planner then works from detected routes only.</p>
        ) : (
          <ul className="flex max-h-56 flex-col overflow-y-auto rounded-[var(--radius-input)] border border-rule">
            {reqs.data.map((r) => (
              <li key={r.id} className="border-b border-rule last:border-b-0">
                <label className="flex min-h-11 cursor-pointer items-center gap-3 px-3 py-1 text-sm hover:bg-paper-3">
                  <input type="checkbox" className="size-4 accent-[var(--color-accent)]" checked={selected.has(r.id)} onChange={() => toggle(r.id)} />
                  <span className="mono font-medium">{r.key}</span>
                  <span className="min-w-0 flex-1 truncate text-ink-2">{r.title}</span>
                  <span className="text-xs text-muted tabular-nums">{r.linked_scenarios} linked</span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </fieldset>

      <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-[minmax(0,1fr)_10rem]">
        <Field label="Instructions (optional)" hint="What to focus on. Passed to the model as data, not as commands.">
          {(id, d) => <Textarea id={id} aria-describedby={d} rows={3} maxLength={20000} value={instructions} onChange={(e) => setInstructions(e.target.value)} className="h-auto py-2" />}
        </Field>
        <Field label="Scenario limit">{(id) => <Input id={id} type="number" min={1} max={100} value={max} onChange={(e) => setMax(e.target.value)} />}</Field>
      </div>

      {create.error ? (
        <p role="alert" className="rounded-[var(--radius-input)] bg-error-soft p-3 text-sm text-error">
          {errorMessage(create.error)}
        </p>
      ) : null}
      <div className="flex flex-wrap justify-end gap-2 border-t border-rule pt-4">
        <Button onClick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" loading={create.isPending}>
          {create.isPending ? "Planning…" : "Create plan"}
        </Button>
      </div>
    </form>
  );
}
