"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Circle, Square } from "lucide-react";
import { ApiError, api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan } from "@/lib/hooks";
import { dateTime, relative, shortId } from "@/lib/format";
import { Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/field";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Section } from "@/components/ui/page";
import { GherkinEditor } from "./gherkin-editor";
import { useEnvironments } from "./run-dialog";

function eventCount(r: Schemas["Recording"]) {
  return Array.isArray(r.events) ? (r.events as unknown[]).length : null;
}

/** Past recordings of this project, newest first as the API returns them. */
export function RecordingsSection({ projectId, onRecord }: { projectId: string; onRecord: () => void }) {
  const canWrite = useCan("engineer");
  const recs = useQuery({
    queryKey: keys.project(projectId, "recordings"),
    enabled: !!projectId,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/recordings", { params: { path: { id: projectId } } })),
  });
  return (
    <Section title="Recordings" description="A recording captures clicks and typing in a real browser and turns them into library steps.">
      <QueryState
        query={recs}
        empty={
          <Empty title="No recordings yet." action={canWrite ? <Button onClick={onRecord}>Record a flow</Button> : undefined}>
            Record a flow in a browser to draft a suite from what you do.
          </Empty>
        }
      >
        {(d) => (
          <Table>
            <thead>
              <tr>
                <Th>Start URL</Th>
                <Th>Status</Th>
                <Th numeric>Events</Th>
                <Th>Started</Th>
                <Th>Updated</Th>
              </tr>
            </thead>
            <tbody>
              {d.map((r) => (
                <Tr key={r.id}>
                  <Td label="Start URL">
                    <span className="mono break-all">{r.url}</span>
                    <span className="mono ml-2 text-xs text-muted">{shortId(r.id)}</span>
                  </Td>
                  <Td label="Status">
                    <StatusPill status={r.status} />
                  </Td>
                  <Td label="Events" numeric mono>
                    {eventCount(r) ?? "–"}
                  </Td>
                  <Td label="Started" className="text-ink-2">
                    {dateTime(r.created_at)}
                  </Td>
                  <Td label="Updated" className="text-ink-2">
                    {relative(r.updated_at)}
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
    </Section>
  );
}

/** Start a recording, watch its status, stop it and turn the generated Gherkin into a suite. */
export function RecordDialog({ projectId, open, onOpenChange }: { projectId: string; open: boolean; onOpenChange: (o: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} wide title="Record a flow" description="The recorder opens the start URL in a browser on the recording runner. Everything you click and type there becomes library steps.">
      {open ? <RecordFlow projectId={projectId} onClose={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function RecordFlow({ projectId, onClose }: { projectId: string; onClose: () => void }) {
  const qc = useQueryClient();
  const router = useRouter();
  const envs = useEnvironments(projectId);
  const [url, setUrl] = useState<string | null>(null);
  const [recId, setRecId] = useState<string | null>(null);
  const [stopped, setStopped] = useState<Schemas["StoppedRecording"] | null>(null);
  const suggested = envs.data?.find((e) => e.is_default)?.base_url ?? envs.data?.find((e) => e.base_url)?.base_url ?? "";
  const startUrl = url ?? suggested;

  const start = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/recordings", { params: { path: { id: projectId } }, body: { url: startUrl.trim() } })),
    onSuccess: (r) => {
      setRecId(r.id);
      qc.invalidateQueries({ queryKey: keys.project(projectId, "recordings") });
    },
  });
  const live = useQuery({
    queryKey: keys.project(projectId, "recording", recId),
    enabled: !!recId && !stopped,
    refetchInterval: 2_000,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/recordings/{rec_id}", { params: { path: { id: projectId, rec_id: recId! } } })),
  });
  const stop = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/recordings/{rec_id}/stop", { params: { path: { id: projectId, rec_id: recId! } } })),
    onSuccess: (s) => {
      setStopped(s);
      qc.invalidateQueries({ queryKey: keys.project(projectId, "recordings") });
    },
  });
  const save = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/suites", { params: { path: { id: projectId } }, body: { source: stopped!.gherkin } })),
    onSuccess: (r) => {
      qc.invalidateQueries({ queryKey: keys.project(projectId, "suites") });
      router.push(`/studio/${r.suite_id}?project=${projectId}`);
    },
  });

  if (stopped) {
    const hasPlaceholder = /\{\{[^}]+\}\}/.test(stopped.gherkin);
    return (
      <div className="flex flex-col gap-4">
        <p className="text-ink-2">
          Recording stopped. The Gherkin below was generated from {eventCount(stopped.recording) ?? "the"} recorded events using library steps only. Nothing has been saved yet.
        </p>
        <p className="rounded-[var(--radius-input)] bg-paper-2 p-3 text-sm text-ink-2">
          Values typed into password fields were not stored. They appear as placeholders such as <code>{"{{password}}"}</code>, which resolve from the environment&rsquo;s secrets at run time.
          {hasPlaceholder ? "" : " This recording contains no redacted values."}
        </p>
        <GherkinEditor value={stopped.gherkin} readOnly height={320} label="Generated Gherkin" />
        {save.error ? (
          <p role="alert" className="text-sm text-error">
            {errorMessage(save.error)}
          </p>
        ) : null}
        <div className="flex flex-wrap justify-end gap-2 border-t border-rule pt-4">
          <Button onClick={onClose}>Discard</Button>
          <Button variant="primary" loading={save.isPending} onClick={() => save.mutate()}>
            Open in editor
          </Button>
        </div>
        <p className="text-xs text-muted">Open in editor saves this as a new suite. Its scenarios start as drafts and need approval before they can run.</p>
      </div>
    );
  }

  if (recId) {
    const r = live.data;
    return (
      <div className="flex flex-col gap-4">
        <div aria-live="polite" className="flex flex-wrap items-center gap-3">
          <StatusPill status={r?.status ?? "recording"} />
          <span className="mono break-all text-sm">{r?.url ?? startUrl}</span>
          {r && eventCount(r) != null ? <span className="mono text-xs text-muted">{eventCount(r)} events so far</span> : null}
        </div>
        <p className="text-ink-2">Work through the flow in the recorder&rsquo;s browser window. Stop when you are done; the events are converted into Gherkin.</p>
        {live.error ? (
          <p role="alert" className="text-sm text-error">
            Status could not be read: <code>{errorMessage(live.error)}</code>
          </p>
        ) : null}
        {stop.error ? (
          <p role="alert" className="text-sm text-error">
            {errorMessage(stop.error)}
          </p>
        ) : null}
        <div className="flex flex-wrap justify-end gap-2 border-t border-rule pt-4">
          <Button variant="primary" loading={stop.isPending} onClick={() => stop.mutate()}>
            <Square aria-hidden className="size-4" /> Stop and generate
          </Button>
        </div>
      </div>
    );
  }

  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        start.mutate();
      }}
    >
      <Field label="Start URL" hint="An absolute http or https URL. Defaults to the default environment's base URL.">
        {(id, d) => <Input id={id} aria-describedby={d} type="url" required placeholder="https://" value={startUrl} onChange={(e) => setUrl(e.target.value)} className="mono" />}
      </Field>
      {start.error ? (
        <div role="alert" className="flex flex-col gap-1 rounded-[var(--radius-input)] bg-error-soft p-3 text-sm text-error">
          {start.error instanceof ApiError && start.error.status === 503 ? <p className="font-medium">Recording is not available on this server.</p> : null}
          <p>{errorMessage(start.error)}</p>
        </div>
      ) : null}
      <div className="flex flex-wrap justify-end gap-2 border-t border-rule pt-4">
        <Button onClick={onClose}>Cancel</Button>
        <Button type="submit" variant="primary" loading={start.isPending} disabled={!startUrl.trim()}>
          <Circle aria-hidden className="size-4" /> Start recording
        </Button>
      </div>
    </form>
  );
}
