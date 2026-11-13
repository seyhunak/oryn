"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWsId } from "@/lib/hooks";
import { dateTime, relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Field, Input, Select } from "@/components/ui/field";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";

function NewConnectionDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const [kind, setKind] = useState<"github" | "gitlab">("github");
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [token, setToken] = useState("");
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/workspaces/{ws}/connections", {
          params: { path: { ws } },
          body: { kind, name: name.trim(), base_url: baseUrl.trim() || null, token: token || null },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: keys.ws(ws, "connections") });
      setName("");
      setBaseUrl("");
      setToken("");
      onOpenChange(false);
    },
  });
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) create.reset();
      }}
      title="New repository connection"
      description="Read access to GitHub or GitLab repositories, used to import and analyse a project's code."
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="new-connection" loading={create.isPending} disabled={!name.trim()}>
            Add connection
          </Button>
        </>
      }
    >
      <form
        id="new-connection"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) create.mutate();
        }}
      >
        <Field label="Provider">
          {(id) => (
            <Select id={id} value={kind} onChange={(e) => setKind(e.target.value as "github" | "gitlab")}>
              <option value="github">GitHub</option>
              <option value="gitlab">GitLab</option>
            </Select>
          )}
        </Field>
        <Field label="Name" hint="For example, GitHub org or self-hosted GitLab.">
          {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required />}
        </Field>
        <Field label="Base URL (optional)" hint={`Defaults to ${kind === "github" ? "https://github.com" : "https://gitlab.com"}. Set it for GitHub Enterprise or self-hosted GitLab.`}>
          {(id, d) => <Input id={id} aria-describedby={d} type="url" className="font-mono" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} autoComplete="off" />}
        </Field>
        <Field label="Access token" hint="A personal access token with read access to repositories. Stored encrypted; never shown again." error={create.error ? errorMessage(create.error) : null}>
          {(id, d) => <Input id={id} aria-describedby={d} type="password" autoComplete="new-password" value={token} onChange={(e) => setToken(e.target.value)} />}
        </Field>
      </form>
    </Dialog>
  );
}

function VerifyCell({ c }: { c: Schemas["RepositoryConnection"] }) {
  const ws = useWsId();
  const verify = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces/{ws}/connections/{id}/verify", { params: { path: { ws, id: c.id } } })),
  });
  return (
    <div className="flex flex-col items-start gap-1">
      <Button size="sm" loading={verify.isPending} onClick={() => verify.mutate()}>
        Verify
      </Button>
      {verify.data ? (
        <span role="status" className="flex flex-wrap items-center gap-2 text-xs">
          {verify.data.ok ? <StatusPill status="verified" /> : <StatusPill status="failed" />}
          {verify.data.account ? <span className="mono">{verify.data.account}</span> : null}
          <span className={verify.data.ok ? "text-ink-2" : "text-error"}>{verify.data.message}</span>
        </span>
      ) : null}
      {verify.error ? <span role="alert" className="text-xs text-error">{errorMessage(verify.error)}</span> : null}
    </div>
  );
}

export function ConnectionsTab() {
  const ws = useWsId();
  const qc = useQueryClient();
  const canManage = useCan("admin");
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState<Schemas["RepositoryConnection"] | null>(null);
  const list = useQuery({
    queryKey: keys.ws(ws, "connections"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/connections", { params: { path: { ws } } })),
  });
  const remove = useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE("/api/v1/workspaces/{ws}/connections/{id}", { params: { path: { ws, id } } })),
    onSuccess: () => {
      setDeleting(null);
      qc.invalidateQueries({ queryKey: keys.ws(ws, "connections") });
    },
  });
  const addButton = canManage ? (
    <Button variant="primary" onClick={() => setAdding(true)}>
      <Plus aria-hidden className="size-4" /> New connection
    </Button>
  ) : null;
  return (
    <Section title="Repository connections" description="Credentials for private GitHub and GitLab repositories. Public repositories need no connection." actions={addButton}>
      <QueryState query={list} empty={<Empty title="No repository connections." action={addButton}>Add one to import private repositories into a project.</Empty>}>
        {(rows) => (
          <Table>
            <thead>
              <tr>
                <Th>Connection</Th>
                <Th>Provider</Th>
                <Th>Base URL</Th>
                <Th>Created</Th>
                <Th>Check</Th>
                {canManage ? (
                  <Th>
                    <span className="visually-hidden">Actions</span>
                  </Th>
                ) : null}
              </tr>
            </thead>
            <tbody>
              {rows.map((c) => (
                <Tr key={c.id}>
                  <Td label="Connection" className="font-medium">
                    {c.name}
                  </Td>
                  <Td label="Provider" className="text-ink-2">
                    {c.kind === "gitlab" ? "GitLab" : c.kind === "github" ? "GitHub" : c.kind}
                  </Td>
                  <Td label="Base URL" mono className="text-xs text-ink-2">
                    {c.base_url}
                  </Td>
                  <Td label="Created" className="text-ink-2">
                    <span title={dateTime(c.created_at)}>{relative(c.created_at)}</span>
                  </Td>
                  <Td label="Check">
                    <VerifyCell c={c} />
                  </Td>
                  {canManage ? (
                    <Td label="Actions" className="text-right">
                      <Button size="sm" variant="danger" onClick={() => setDeleting(c)}>
                        Delete
                      </Button>
                    </Td>
                  ) : null}
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
      {canManage ? <NewConnectionDialog open={adding} onOpenChange={setAdding} /> : null}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            remove.reset();
          }
        }}
        title={`Delete ${deleting?.name ?? "connection"}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>The connection and its stored token are removed. This cannot be undone.</p>
            {remove.error ? <p role="alert" className="text-sm text-error">{errorMessage(remove.error)}</p> : null}
          </div>
        }
        confirmLabel="Delete connection"
        loading={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting.id)}
      />
    </Section>
  );
}
