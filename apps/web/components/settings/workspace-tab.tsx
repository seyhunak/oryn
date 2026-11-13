"use client";
import { useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWsId } from "@/lib/hooks";
import { usePrefs } from "@/lib/store";
import { dateTime } from "@/lib/format";
import { Meta, Section } from "@/components/ui/page";
import { QueryState, Skeleton } from "@/components/ui/states";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Input } from "@/components/ui/field";
import { Dialog } from "@/components/ui/dialog";

function WorkspaceForm({ w }: { w: Schemas["WorkspaceDetail"] }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const isAdmin = useCan("admin");
  const [name, setName] = useState(w.name);
  const [approval, setApproval] = useState(w.require_approval);
  const save = useMutation({
    mutationFn: () => unwrap(api.PATCH("/api/v1/workspaces/{ws}", { params: { path: { ws } }, body: { name: name.trim(), require_approval: approval } })),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.ws(ws, "detail") });
      qc.invalidateQueries({ queryKey: keys.me });
    },
  });
  const dirty = name.trim() !== w.name || approval !== w.require_approval;
  return (
    <div className="grid grid-cols-1 gap-10 lg:grid-cols-[minmax(0,1fr)_320px]">
      <form
        className="flex max-w-xl flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (dirty && name.trim()) save.mutate();
        }}
      >
        <Field label="Name">{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} disabled={!isAdmin} required />}</Field>
        <Field label="Slug" hint="Set when the workspace is created; used in URLs and the CLI.">
          {(id, d) => <Input id={id} aria-describedby={d} value={w.slug} readOnly disabled className="font-mono" />}
        </Field>
        <Checkbox label="Require approval before scenarios can run" checked={approval} disabled={!isAdmin} onChange={(e) => setApproval(e.target.checked)} />
        <p className="text-xs text-muted">When on, only scenarios an engineer approved in Test Studio are queued; unapproved ones are skipped and counted.</p>
        {isAdmin ? (
          <div className="mt-2 flex items-center gap-3">
            <Button type="submit" variant="primary" disabled={!dirty || !name.trim()} loading={save.isPending}>
              Save workspace
            </Button>
          </div>
        ) : (
          <p className="text-sm text-muted">Only admins and owners can change workspace settings.</p>
        )}
        {save.error ? <p role="alert" className="text-sm text-error">{errorMessage(save.error)}</p> : null}
      </form>
      <aside className="panel flex flex-col gap-3 self-start p-4">
        <h3>Your access</h3>
        <Meta
          items={[
            { label: "Role", value: w.role },
            { label: "Created", value: <span className="mono">{dateTime(w.created_at)}</span> },
            { label: "ID", value: <span className="mono text-xs">{w.id}</span> },
          ]}
        />
        <p className="text-xs text-ink-2">Permissions: {w.permissions.join(", ")}</p>
      </aside>
    </div>
  );
}

function NewWorkspaceDialog() {
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const qc = useQueryClient();
  const setWorkspace = usePrefs((s) => s.setWorkspace);
  const open = sp.get("new") === "1";
  const [name, setName] = useState("");
  const setOpen = (o: boolean) => {
    const q = new URLSearchParams(sp.toString());
    if (o) q.set("new", "1");
    else q.delete("new");
    router.replace(`${path}?${q.toString()}`, { scroll: false });
  };
  const create = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces", { body: { name: name.trim() } })),
    onSuccess: async (w) => {
      await qc.invalidateQueries({ queryKey: keys.me });
      setWorkspace(w.id);
      setName("");
      router.replace("/settings?tab=workspace");
    },
  });
  return (
    <>
      <Button onClick={() => setOpen(true)}>
        <Plus aria-hidden className="size-4" /> New workspace
      </Button>
      <Dialog
        open={open}
        onOpenChange={(o) => {
          setOpen(o);
          if (!o) create.reset();
        }}
        title="New workspace"
        description="A separate space with its own projects, members, workers and settings. You become its owner."
        footer={
          <>
            <Button onClick={() => setOpen(false)}>Cancel</Button>
            <Button variant="primary" type="submit" form="new-workspace" loading={create.isPending} disabled={!name.trim()}>
              Create workspace
            </Button>
          </>
        }
      >
        <form
          id="new-workspace"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate();
          }}
        >
          <Field label="Name" hint="The slug is derived from the name." error={create.error ? errorMessage(create.error) : null}>
            {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required autoFocus />}
          </Field>
        </form>
      </Dialog>
    </>
  );
}

export function WorkspaceTab() {
  const ws = useWsId();
  const detail = useQuery({
    queryKey: keys.ws(ws, "detail"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}", { params: { path: { ws } } })),
  });
  return (
    <Section title="Workspace" actions={<NewWorkspaceDialog />}>
      <QueryState query={detail} skeleton={<Skeleton className="h-48 max-w-xl" />}>
        {(w) => <WorkspaceForm key={`${w.id}-${w.name}-${w.require_approval}`} w={w} />}
      </QueryState>
    </Section>
  );
}
