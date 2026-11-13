"use client";
import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, Save, Trash2 } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { PageHeader } from "@/components/ui/page";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/dialog";
import { Field, Input } from "@/components/ui/field";
import { ErrorState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { GherkinEditor, type EditorDiagnostic, type GherkinEditorHandle } from "./gherkin-editor";
import { ScenarioPanel, useSuiteScenarios } from "./scenario-panel";
import { StepLibrary, closestStep, useStepLibrary } from "./step-library";
import { RunDialog } from "./run-dialog";

export const SUITE_TEMPLATE = `@browser
Feature: Name of the feature
  One or two sentences on what this feature covers and why it matters.

  Scenario: The home page loads
    Given I open "/"
    Then I should see "Welcome"
`;

type Initial = { id: string | null; source: string; name: string; description: string; path: string; updatedAt?: string };

/** Load the suite (when editing) and hand its saved values to the editor body. */
export function SuiteEditor({ projectId, suiteId }: { projectId: string; suiteId: string | null }) {
  const detail = useQuery({
    queryKey: keys.project(projectId, "suite", suiteId),
    enabled: !!projectId && !!suiteId,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/suites/{suite_id}", { params: { path: { id: projectId, suite_id: suiteId! } } })),
  });
  if (!suiteId) return <EditorBody projectId={projectId} initial={{ id: null, source: SUITE_TEMPLATE, name: "", description: "", path: "" }} />;
  if (detail.isPending)
    return (
      <div className="flex flex-col gap-4">
        <Skeleton className="h-10 w-64" />
        <Skeleton className="h-[520px]" />
      </div>
    );
  if (detail.error) return <ErrorState error={detail.error} retry={() => detail.refetch()} />;
  const s = detail.data.suite;
  return (
    <EditorBody
      key={s.id}
      projectId={projectId}
      initial={{ id: s.id, source: s.feature_source, name: s.name, description: s.description, path: s.path ?? "", updatedAt: s.updated_at }}
    />
  );
}

type SaveSummary = { created: number; kept: number; removed: number; returnedToDraft: string[]; validation: Schemas["ValidateResult"]; isNew: boolean };

function summarise(before: Schemas["Scenario"][], after: Schemas["Scenario"][], validation: Schemas["ValidateResult"], isNew: boolean): SaveSummary {
  const prev = new Map(before.map((s) => [s.name, s]));
  const next = new Set(after.map((s) => s.name));
  return {
    created: after.filter((s) => !prev.has(s.name)).length,
    kept: after.filter((s) => prev.has(s.name)).length,
    removed: before.filter((s) => !next.has(s.name)).length,
    returnedToDraft: after.filter((s) => prev.get(s.name)?.status === "approved" && s.status === "draft").map((s) => s.name),
    validation,
    isNew,
  };
}

function EditorBody({ projectId, initial }: { projectId: string; initial: Initial }) {
  const router = useRouter();
  const qc = useQueryClient();
  const canWrite = useCan("engineer");
  const [suiteId, setSuiteId] = useState(initial.id);
  const [source, setSource] = useState(initial.source);
  const [saved, setSaved] = useState(initial);
  const [name, setName] = useState(initial.name);
  const [description, setDescription] = useState(initial.description);
  const [path, setPath] = useState(initial.path);
  const [debounced, setDebounced] = useState(initial.source);
  const [summary, setSummary] = useState<SaveSummary | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [running, setRunning] = useState(false);
  const editor = useRef<GherkinEditorHandle | null>(null);
  const lib = useStepLibrary();
  const selectedScenario = useSearchParams().get("scenario");
  const scenarioList = useSuiteScenarios(projectId, suiteId);
  const revealLine = scenarioList.data?.find((s) => s.id === selectedScenario)?.line ?? null;

  const dirty = source !== saved.source || name !== saved.name || description !== saved.description || path !== saved.path;

  useEffect(() => {
    const t = setTimeout(() => setDebounced(source), 400);
    return () => clearTimeout(t);
  }, [source]);

  useEffect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);

  const validation = useQuery({
    queryKey: keys.project(projectId, "validate", debounced),
    enabled: !!projectId && debounced.trim().length > 0,
    placeholderData: keepPreviousData,
    staleTime: 60_000,
    queryFn: () => unwrap(api.POST("/api/v1/projects/{id}/gherkin/validate", { params: { path: { id: projectId } }, body: { source: debounced } })),
  });
  const checking = debounced !== source || validation.isFetching;
  const v = validation.data;

  const problems = useMemo(() => {
    if (!v) return [];
    const out: { line: number; message: string; step?: string | null; scenario?: string }[] = v.errors.map((e) => ({ line: e.line, message: e.message, step: e.step }));
    for (const sc of v.scenarios) for (const u of sc.unsupported) out.push({ line: u.line, message: u.message, step: u.step, scenario: sc.name });
    return out.sort((a, b) => a.line - b.line);
  }, [v]);
  const markers: EditorDiagnostic[] = useMemo(() => problems.map((p) => ({ line: p.line, message: p.message, severity: "error" })), [problems]);

  const save = useMutation({
    mutationFn: async () => {
      const listFor = (sid: string) => unwrap(api.GET("/api/v1/projects/{id}/scenarios", { params: { path: { id: projectId }, query: { suite_id: sid } } }));
      const before = suiteId ? await listFor(suiteId) : [];
      const body = { source, name: name.trim() || null, description: description.trim(), path: path.trim() || null };
      const res = suiteId
        ? await unwrap(api.PUT("/api/v1/projects/{id}/suites/{suite_id}", { params: { path: { id: projectId, suite_id: suiteId } }, body }))
        : await unwrap(api.POST("/api/v1/projects/{id}/suites", { params: { path: { id: projectId } }, body }));
      const after = await listFor(res.suite_id);
      const detail = await unwrap(api.GET("/api/v1/projects/{id}/suites/{suite_id}", { params: { path: { id: projectId, suite_id: res.suite_id } } }));
      return { res, before, after, detail };
    },
    onSuccess: ({ res, before, after, detail }) => {
      const isNew = !suiteId;
      setSummary(summarise(before, after, res.validation, isNew));
      const s = detail.suite;
      const next = { id: s.id, source: s.feature_source, name: s.name, description: s.description, path: s.path ?? "", updatedAt: s.updated_at };
      setSaved(next);
      setName(next.name);
      setDescription(next.description);
      setPath(next.path);
      if (source !== next.source) setSource(next.source);
      setSuiteId(s.id);
      qc.invalidateQueries({ queryKey: keys.project(projectId, "suites") });
      qc.invalidateQueries({ queryKey: keys.project(projectId, "scenarios") });
      qc.setQueryData(keys.project(projectId, "suite", s.id), detail);
      // Keep this editor mounted (and the save summary visible) while the URL becomes the suite's own.
      if (isNew) window.history.replaceState(null, "", `/studio/${s.id}?project=${projectId}`);
    },
  });

  const del = useMutation({
    mutationFn: () => unwrap(api.DELETE("/api/v1/projects/{id}/suites/{suite_id}", { params: { path: { id: projectId, suite_id: suiteId! } } })),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.project(projectId, "suites") });
      qc.invalidateQueries({ queryKey: keys.project(projectId, "scenarios") });
      router.push(`/studio?project=${projectId}`);
    },
  });

  const title = saved.name || (suiteId ? "Untitled suite" : "New suite");

  return (
    <>
      <PageHeader
        crumbs={[{ href: `/studio?project=${projectId}`, label: "Test Studio" }]}
        title={title}
        description={
          suiteId ? (
            <>
              {saved.path ? <span className="mono">{saved.path}</span> : "Written in Test Studio"}
              {saved.updatedAt ? <>, saved {relative(saved.updatedAt)}.</> : "."}
            </>
          ) : (
            "Write the feature in Gherkin using steps from the library. New scenarios start as drafts."
          )
        }
        actions={
          canWrite ? (
            <>
              {suiteId ? (
                <>
                  <Button size="lg" variant="danger" onClick={() => setConfirmDelete(true)}>
                    <Trash2 aria-hidden className="size-4" /> Delete
                  </Button>
                  <Button size="lg" onClick={() => setRunning(true)} disabled={dirty} title={dirty ? "Save your changes before running" : undefined}>
                    <Play aria-hidden className="size-4" /> Run
                  </Button>
                </>
              ) : null}
              <Button size="lg" variant="primary" loading={save.isPending} disabled={!dirty && !!suiteId} onClick={() => save.mutate()}>
                <Save aria-hidden className="size-4" /> {suiteId ? "Save" : "Create suite"}
              </Button>
            </>
          ) : null
        }
      />

      <div className="grid grid-cols-1 gap-8 min-[72rem]:grid-cols-[minmax(0,1fr)_320px]">
        <div className="flex min-w-0 flex-col gap-4">
          {save.error ? (
            <p role="alert" className="rounded-[var(--radius-input)] bg-error-soft p-3 text-sm text-error">
              {errorMessage(save.error)}
            </p>
          ) : null}
          {summary ? <SaveSummaryNote s={summary} onDismiss={() => setSummary(null)} /> : null}

          <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-3">
            <Field label="Name" hint="Defaults to the Feature name.">
              {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} readOnly={!canWrite} />}
            </Field>
            <Field label="Description">{(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} readOnly={!canWrite} />}</Field>
            <Field label="Repository path" hint="Optional, for features kept in a repository.">
              {(id, d) => <Input id={id} aria-describedby={d} value={path} onChange={(e) => setPath(e.target.value)} className="mono" readOnly={!canWrite} />}
            </Field>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2>Feature</h2>
            <span aria-live="polite" className="text-sm text-ink-2">
              {dirty ? "Unsaved changes. " : ""}
              {checking ? "Checking…" : v ? (v.ok && problems.length === 0 ? "Valid." : `${problems.length} ${problems.length === 1 ? "problem" : "problems"}.`) : ""}
            </span>
          </div>
          <GherkinEditor
            value={source}
            onChange={setSource}
            readOnly={!canWrite}
            diagnostics={markers}
            height="min(64vh, 640px)"
            label="Feature source in Gherkin"
            onReady={(h) => (editor.current = h)}
            revealLine={revealLine}
          />
          <Diagnostics validation={validation} problems={problems} library={lib.data ?? []} />
        </div>

        <aside className="flex min-w-0 flex-col gap-8" aria-label="Scenarios and step library">
          {suiteId ? <ScenarioPanel projectId={projectId} suiteId={suiteId} selectedId={selectedScenario} /> : null}
          <StepLibrary onInsert={canWrite ? (ex) => editor.current?.insert(ex) : undefined} />
        </aside>
      </div>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={(o) => {
          setConfirmDelete(o);
          if (!o) del.reset();
        }}
        title={`Delete ${title}`}
        body={
          <>
            <p>The suite and its scenarios are removed from Test Studio. Past executions keep their recorded evidence.</p>
            {del.error ? (
              <p role="alert" className="mt-2 text-sm text-error">
                {errorMessage(del.error)}
              </p>
            ) : null}
          </>
        }
        confirmLabel="Delete suite"
        loading={del.isPending}
        onConfirm={() => del.mutate()}
      />
      {suiteId ? <RunDialog projectId={projectId} suite={running ? { id: suiteId, name: title } : null} open={running} onOpenChange={setRunning} /> : null}
    </>
  );
}

function SaveSummaryNote({ s, onDismiss }: { s: SaveSummary; onDismiss: () => void }) {
  const parts = s.isNew
    ? [`${s.created} ${s.created === 1 ? "scenario" : "scenarios"} created as draft`]
    : [`${s.created} created`, `${s.kept} kept`, `${s.removed} removed`];
  const blocked = s.validation.scenarios.filter((x) => !x.executable).length;
  return (
    <div role="status" className="flex flex-col gap-2 rounded-[var(--radius-input)] border border-rule bg-paper-2 p-3 text-sm">
      <div className="flex items-start justify-between gap-2">
        <p className="font-medium text-ink">Saved. {parts.join(", ")}.</p>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
      {s.returnedToDraft.length ? (
        <p className="text-ink-2">
          {s.returnedToDraft.length === 1 ? "One approved scenario" : `${s.returnedToDraft.length} approved scenarios`} changed and returned to draft: {s.returnedToDraft.join(", ")}. Approve {s.returnedToDraft.length === 1 ? "it" : "them"} again before {s.returnedToDraft.length === 1 ? "it runs" : "they run"}.
        </p>
      ) : null}
      {!s.validation.ok || blocked ? (
        <p className="text-error">
          {s.validation.errors.length ? `${s.validation.errors.length} parse ${s.validation.errors.length === 1 ? "error" : "errors"}. ` : ""}
          {blocked ? `${blocked} ${blocked === 1 ? "scenario has" : "scenarios have"} unsupported steps and cannot be approved.` : ""}
        </p>
      ) : null}
    </div>
  );
}

function Diagnostics({
  validation,
  problems,
  library,
}: {
  validation: { data: Schemas["ValidateResult"] | undefined; error: unknown; isPending: boolean; refetch: () => unknown };
  problems: { line: number; message: string; step?: string | null; scenario?: string }[];
  library: Schemas["StepDef"][];
}) {
  if (validation.error) return <ErrorState error={validation.error} retry={() => validation.refetch()} />;
  const v = validation.data;
  if (!v) return validation.isPending ? <Skeleton className="h-16" /> : null;
  return (
    <div className="flex flex-col gap-4">
      {problems.length ? (
        <section className="flex flex-col gap-2" aria-label="Problems">
          <h3>Problems</h3>
          <ul className="flex flex-col border-t border-rule">
            {problems.map((p, i) => {
              const near = p.step ? closestStep(p.step, library) : null;
              return (
                <li key={`${p.line}-${i}`} className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2 border-b border-rule py-2 text-sm">
                  <span className="mono text-muted">line {p.line}</span>
                  <div className="flex min-w-0 flex-col gap-1">
                    <span className="break-words text-error">{p.message}</span>
                    {p.scenario ? <span className="text-xs text-ink-2">In scenario {p.scenario}</span> : null}
                    {near ? (
                      <span className="text-xs text-ink-2">
                        Closest library step: <code className="text-ink">{near.pattern}</code>, for example <code>{near.example}</code>
                      </span>
                    ) : null}
                  </div>
                </li>
              );
            })}
          </ul>
        </section>
      ) : null}
      {v.scenarios.length ? (
        <section className="flex flex-col gap-2" aria-label="Compiled scenarios">
          <h3>Compiled scenarios</h3>
          <ul className="flex flex-col border-t border-rule">
            {v.scenarios.map((sc, i) => (
              <li key={`${sc.line}-${i}`} className="flex flex-wrap items-center gap-2 border-b border-rule py-2 text-sm">
                <span className="mono w-16 text-muted">line {sc.line}</span>
                <span className="min-w-0 flex-1 break-words">{sc.name}</span>
                <Tag>{sc.kind}</Tag>
                <span className="mono text-xs text-ink-2">{sc.steps} steps</span>
                {sc.executable ? <StatusPill status="ready" label="executable" /> : <StatusPill status="failed" label="unsupported steps" />}
              </li>
            ))}
          </ul>
        </section>
      ) : !problems.length ? (
        <p className="text-sm text-muted">No scenarios yet. Add a Scenario under the Feature.</p>
      ) : null}
    </div>
  );
}
