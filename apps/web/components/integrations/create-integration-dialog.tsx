"use client";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { keys, useProjects, useWsId } from "@/lib/hooks";
import { Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Input, Select } from "@/components/ui/field";

export const EVENTS = [
  { id: "execution.finished", label: "Execution finished", hint: "every run, whatever the outcome" },
  { id: "execution.failed", label: "Execution failed", hint: "runs with at least one failed or errored scenario" },
];

export function EventPicker({ value, onChange, idPrefix }: { value: string[]; onChange: (v: string[]) => void; idPrefix: string }) {
  return (
    <fieldset className="flex flex-col gap-0">
      <legend className="mb-1 text-sm font-medium text-ink-2">Events</legend>
      {EVENTS.map((e) => (
        <Checkbox
          key={e.id}
          id={`${idPrefix}-${e.id}`}
          label={`${e.label} (${e.hint})`}
          checked={value.includes(e.id)}
          onChange={(ev) => onChange(ev.target.checked ? [...value, e.id] : value.filter((x) => x !== e.id))}
        />
      ))}
    </fieldset>
  );
}

export function CreateIntegrationDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const projects = useProjects();
  const [kind, setKind] = useState<"webhook" | "slack">("webhook");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [secret, setSecret] = useState("");
  const [project, setProject] = useState("");
  const [events, setEvents] = useState<string[]>(["execution.failed"]);
  const reset = () => {
    setKind("webhook");
    setName("");
    setUrl("");
    setSecret("");
    setProject("");
    setEvents(["execution.failed"]);
  };
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/workspaces/{ws}/integrations", {
          params: { path: { ws } },
          body: {
            kind,
            name: name.trim(),
            events,
            project_id: project || null,
            url: kind === "webhook" ? url.trim() : null,
            secret: secret.trim() || null,
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: keys.ws(ws, "integrations") });
      onOpenChange(false);
      reset();
      create.reset();
    },
  });
  const valid = name.trim() && events.length > 0 && (kind === "webhook" ? url.trim() : secret.trim());
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) create.reset();
      }}
      title="Add integration"
      description="Send a notification when an execution finishes or fails."
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="create-integration" loading={create.isPending} disabled={!valid}>
            Add integration
          </Button>
        </>
      }
    >
      <form
        id="create-integration"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) create.mutate();
        }}
      >
        <Field label="Kind">
          {(id) => (
            <Select id={id} value={kind} onChange={(e) => setKind(e.target.value as "webhook" | "slack")}>
              <option value="webhook">Webhook</option>
              <option value="slack">Slack (incoming webhook)</option>
            </Select>
          )}
        </Field>
        <Field label="Name" hint="For example, CI dashboard or #qa-alerts.">
          {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required />}
        </Field>
        {kind === "webhook" ? (
          <>
            <Field label="Endpoint URL" hint="ORYN posts a JSON body here.">
              {(id, d) => <Input id={id} aria-describedby={d} type="url" inputMode="url" className="font-mono" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://ci.example.com/hooks/oryn" autoComplete="off" required />}
            </Field>
            <Field label="Signing secret" hint="Optional. When set, each request carries x-oryn-signature: sha256=<hex>, the HMAC-SHA256 of the raw body with this secret. Stored encrypted; never shown again.">
              {(id, d) => <Input id={id} aria-describedby={d} type="password" autoComplete="off" value={secret} onChange={(e) => setSecret(e.target.value)} />}
            </Field>
          </>
        ) : (
          <Field label="Slack incoming-webhook URL" hint="From Slack: Apps, Incoming Webhooks. It is a secret: stored encrypted and never shown again.">
            {(id, d) => <Input id={id} aria-describedby={d} type="password" autoComplete="off" className="font-mono" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="https://hooks.slack.com/services/…" required />}
          </Field>
        )}
        <Field label="Project" hint="Limit notifications to one project, or leave on all projects.">
          {(id, d) => (
            <Select id={id} aria-describedby={d} value={project} onChange={(e) => setProject(e.target.value)}>
              <option value="">All projects</option>
              {(projects.data ?? []).map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          )}
        </Field>
        <EventPicker value={events} onChange={setEvents} idPrefix="new" />
        {events.length === 0 ? <p className="text-xs text-error">Choose at least one event.</p> : null}
        {create.error ? <p role="alert" className="text-sm text-error">{errorMessage(create.error)}</p> : null}
      </form>
    </Dialog>
  );
}
