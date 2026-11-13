"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { Checkbox, Field, Input, Textarea } from "@/components/ui/field";
import { can, InlineError, KeyValueRows, type ProjectDetail } from "./common";

type Environment = Schemas["Environment"];
type SecretState = { name: string; mode: "keep" | "replace" | "remove"; value: string };

export function EnvironmentsTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const qc = useQueryClient();
  const canWrite = can(project, "project:write");
  const canSecrets = can(project, "secrets:manage");
  const [editing, setEditing] = useState<Environment | "new" | null>(null);
  const [deleting, setDeleting] = useState<Environment | null>(null);
  const envs = useQuery({
    queryKey: keys.project(id, "environments"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/environments", { params: { path: { id } } })),
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: keys.project(id, "environments") });
  const del = useMutation({
    mutationFn: (env_id: string) => unwrap(api.DELETE("/api/v1/projects/{id}/environments/{env_id}", { params: { path: { id, env_id } } })),
    onSuccess: () => {
      setDeleting(null);
      invalidate();
    },
  });

  return (
    <Section
      title="Environments"
      description="Where runs point: base URLs, the hosts steps may reach, plain variables available as {{name}}, and encrypted secrets. Secret values are never shown."
      actions={
        canWrite ? (
          <Button variant="primary" onClick={() => setEditing("new")}>
            Add environment
          </Button>
        ) : null
      }
    >
      <QueryState
        query={envs}
        empty={
          <Empty title="No environments yet." action={canWrite ? <Button onClick={() => setEditing("new")}>Add an environment</Button> : undefined}>
            An environment tells a run which base URL to open and which variables and secrets to use.
          </Empty>
        }
      >
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Environment</Th>
                <Th>Base URL</Th>
                <Th>API base URL</Th>
                <Th>Variables</Th>
                <Th>Secrets</Th>
                <Th>Updated</Th>
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {list.map((e) => (
                <Tr key={e.id}>
                  <Td label="Environment">
                    <span className="font-medium">{e.name}</span>
                    {e.is_default ? <Tag className="ml-2">default</Tag> : null}
                    {e.allowed_hosts.length ? <div className="mono mt-1 text-xs text-muted">also reaches {e.allowed_hosts.join(", ")}</div> : null}
                  </Td>
                  <Td label="Base URL" mono className="text-xs break-all">
                    {e.base_url ?? "–"}
                  </Td>
                  <Td label="API base URL" mono className="text-xs break-all">
                    {e.api_base_url ?? "–"}
                  </Td>
                  <Td label="Variables" mono className="text-xs">
                    {Object.keys(e.variables).length ? Object.keys(e.variables).join(", ") : <span className="text-muted">none</span>}
                  </Td>
                  <Td label="Secrets">
                    {e.secret_names.length ? (
                      <div className="flex flex-wrap gap-1">
                        {e.secret_names.map((s) => (
                          <Tag key={s}>
                            <span className="mono">{s}</span>&nbsp;set
                          </Tag>
                        ))}
                      </div>
                    ) : (
                      <span className="text-muted">none</span>
                    )}
                  </Td>
                  <Td label="Updated" className="text-ink-2">
                    {relative(e.updated_at)}
                  </Td>
                  <Td label="Actions" className="text-right whitespace-nowrap">
                    {canWrite ? (
                      <>
                        <Button variant="ghost" size="sm" onClick={() => setEditing(e)}>
                          Edit
                        </Button>
                        <Button variant="danger" size="sm" onClick={() => setDeleting(e)}>
                          Delete
                        </Button>
                      </>
                    ) : null}
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>

      {editing ? (
        <EnvironmentForm key={editing === "new" ? "new" : editing.id} projectId={id} editing={editing} canSecrets={canSecrets} onClose={() => setEditing(null)} onSaved={invalidate} />
      ) : null}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            del.reset();
          }
        }}
        title={`Delete ${deleting?.name ?? "environment"}`}
        body={
          <div className="flex flex-col gap-2">
            <p>Its variables and encrypted secrets are deleted. Runs that name this environment can no longer start. This cannot be undone.</p>
            <InlineError error={del.error} />
          </div>
        }
        confirmLabel="Delete environment"
        loading={del.isPending}
        onConfirm={() => deleting && del.mutate(deleting.id)}
      />
    </Section>
  );
}

function asText(v: unknown) {
  return typeof v === "string" ? v : JSON.stringify(v);
}

function EnvironmentForm({
  projectId,
  editing,
  canSecrets,
  onClose,
  onSaved,
}: {
  projectId: string;
  editing: Environment | "new";
  canSecrets: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const id = projectId;
  const isNew = editing === "new";
  const env = isNew ? null : editing;
  const [name, setName] = useState(env?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(env?.base_url ?? "");
  const [apiBaseUrl, setApiBaseUrl] = useState(env?.api_base_url ?? "");
  const [hosts, setHosts] = useState((env?.allowed_hosts ?? []).join("\n"));
  const [isDefault, setIsDefault] = useState(env?.is_default ?? false);
  const [vars, setVars] = useState(Object.entries(env?.variables ?? {}).map(([k, v]) => ({ k, v: asText(v) })));
  const [existing, setExisting] = useState<SecretState[]>((env?.secret_names ?? []).map((n) => ({ name: n, mode: "keep", value: "" })));
  const [added, setAdded] = useState<{ k: string; v: string }[]>([]);
  const [localError, setLocalError] = useState<string | null>(null);

  const setSecret = (i: number, patch: Partial<SecretState>) => setExisting((s) => s.map((x, j) => (j === i ? { ...x, ...patch } : x)));

  const save = useMutation({
    mutationFn: () => {
      const variables: Record<string, string> = {};
      for (const r of vars) if (r.k.trim()) variables[r.k.trim()] = r.v;
      const body: Schemas["EnvironmentReq"] = {
        name: name.trim(),
        base_url: baseUrl.trim() || null,
        api_base_url: apiBaseUrl.trim() || null,
        allowed_hosts: hosts
          .split(/[\n,]/)
          .map((h) => h.trim())
          .filter(Boolean),
        is_default: isDefault,
        variables: variables as never,
      };
      if (canSecrets) {
        // PUT semantics: null keeps the stored value, a string replaces it, an omitted name is deleted.
        const secrets: Record<string, string | null> = {};
        for (const s of existing) {
          if (s.mode === "keep") secrets[s.name] = null;
          else if (s.mode === "replace") secrets[s.name] = s.value;
        }
        for (const r of added) if (r.k.trim()) secrets[r.k.trim()] = r.v;
        body.secrets = secrets as never;
      }
      return env
        ? unwrap(api.PUT("/api/v1/projects/{id}/environments/{env_id}", { params: { path: { id, env_id: env.id } }, body }))
        : unwrap(api.POST("/api/v1/projects/{id}/environments", { params: { path: { id } }, body }));
    },
    onSuccess: () => {
      onSaved();
      onClose();
    },
  });

  const submit = () => {
    setLocalError(null);
    if (existing.some((s) => s.mode === "replace" && !s.value)) return setLocalError("Enter the new value for each secret you replace, or keep the stored one.");
    if (added.some((r) => r.k.trim() && !r.v)) return setLocalError("Every new secret needs a value.");
    save.mutate();
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      wide
      title={isNew ? "Add environment" : `Edit ${env!.name}`}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form="env-form" loading={save.isPending} disabled={!name.trim()}>
            {isNew ? "Add environment" : "Save changes"}
          </Button>
        </>
      }
    >
      <form
        id="env-form"
        className="flex flex-col gap-2"
        autoComplete="off"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) submit();
        }}
      >
        <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
          <Field label="Name">{(fid, d) => <Input id={fid} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} placeholder="staging" required autoFocus />}</Field>
          <div className="flex items-center pt-5">
            <Checkbox label="Default environment for runs" checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)} />
          </div>
          <Field label="Base URL" hint="Where browser scenarios open pages.">
            {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" type="url" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://staging.example.com" />}
          </Field>
          <Field label="API base URL" hint="Where API scenarios send requests.">
            {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" type="url" value={apiBaseUrl} onChange={(e) => setApiBaseUrl(e.target.value)} placeholder="https://api.staging.example.com" />}
          </Field>
        </div>
        <Field label="Allowed hosts" hint="One per line. Extra hosts steps may reach; base URL hosts are always allowed. *.example.com wildcards work.">
          {(fid, d) => <Textarea id={fid} aria-describedby={d} rows={3} className="mono text-xs" value={hosts} onChange={(e) => setHosts(e.target.value)} />}
        </Field>

        <fieldset className="mb-4 flex flex-col gap-2 border-t border-rule pt-4">
          <legend className="text-sm font-medium text-ink-2">Variables</legend>
          <p className="text-xs text-muted">Plain values, visible to project members. Use them in steps as {"{{name}}"}.</p>
          <KeyValueRows rows={vars} onChange={setVars} keyLabel="Variable" />
        </fieldset>

        <fieldset className="flex flex-col gap-2 border-t border-rule pt-4">
          <legend className="text-sm font-medium text-ink-2">Secrets</legend>
          <p className="text-xs text-muted">
            Encrypted at rest and never returned. {canSecrets ? "Stored values stay unless you replace or remove them." : "Your role cannot change secrets; saving keeps them as they are."}
          </p>
          {existing.length ? (
            <ul className="flex flex-col">
              {existing.map((s, i) => (
                <li key={s.name} className="flex flex-wrap items-center gap-2 border-b border-rule py-2">
                  <span className="mono min-w-32 text-sm">{s.name}</span>
                  {s.mode === "remove" ? (
                    <Tag className="text-error">removed on save</Tag>
                  ) : s.mode === "replace" ? (
                    <input
                      className="field mono w-auto min-w-0 flex-1"
                      type="password"
                      autoComplete="new-password"
                      aria-label={`New value for ${s.name}`}
                      placeholder="new value"
                      value={s.value}
                      onChange={(e) => setSecret(i, { value: e.target.value })}
                    />
                  ) : (
                    <Tag>set</Tag>
                  )}
                  {canSecrets ? (
                    <span className="ml-auto flex gap-1">
                      {s.mode === "keep" ? (
                        <>
                          <Button size="sm" onClick={() => setSecret(i, { mode: "replace" })}>
                            Replace
                          </Button>
                          <Button size="sm" variant="danger" onClick={() => setSecret(i, { mode: "remove" })}>
                            Remove
                          </Button>
                        </>
                      ) : (
                        <Button size="sm" variant="ghost" onClick={() => setSecret(i, { mode: "keep", value: "" })}>
                          Keep stored value
                        </Button>
                      )}
                    </span>
                  ) : null}
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-ink-2">No secrets stored.</p>
          )}
          {canSecrets ? (
            <div className="flex flex-col gap-2">
              {added.map((r, i) => (
                <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto] items-center gap-2">
                  <input
                    className="field mono"
                    aria-label={`New secret name ${i + 1}`}
                    placeholder="name"
                    autoComplete="off"
                    value={r.k}
                    onChange={(e) => setAdded(added.map((x, j) => (j === i ? { ...x, k: e.target.value } : x)))}
                  />
                  <input
                    className="field mono"
                    type="password"
                    autoComplete="new-password"
                    aria-label={`New secret value ${i + 1}`}
                    placeholder="value"
                    value={r.v}
                    onChange={(e) => setAdded(added.map((x, j) => (j === i ? { ...x, v: e.target.value } : x)))}
                  />
                  <Button size="sm" variant="ghost" onClick={() => setAdded(added.filter((_, j) => j !== i))}>
                    Remove
                  </Button>
                </div>
              ))}
              <div>
                <Button size="sm" onClick={() => setAdded([...added, { k: "", v: "" }])}>
                  Add secret
                </Button>
              </div>
            </div>
          ) : null}
        </fieldset>
        {localError ? (
          <p role="alert" className="text-sm text-error">
            {localError}
          </p>
        ) : null}
        <InlineError error={save.error} />
      </form>
    </Dialog>
  );
}
