"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWsId } from "@/lib/hooks";
import { dateTime, relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState, TableSkeleton } from "@/components/ui/states";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/field";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { StatusPill } from "@/components/ui/status";

function CreateTeamDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const create = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces/{ws}/teams", { params: { path: { ws } }, body: { name: name.trim(), description: description.trim() || null } })),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: keys.ws(ws, "teams") });
      setName("");
      setDescription("");
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
      title="New team"
      description="Group members, for example by product area or on-call rotation."
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="create-team" loading={create.isPending} disabled={!name.trim()}>
            Create team
          </Button>
        </>
      }
    >
      <form
        id="create-team"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) create.mutate();
        }}
      >
        <Field label="Name">{(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required autoFocus />}</Field>
        <Field label="Description" hint="Optional." error={create.error ? errorMessage(create.error) : null}>
          {(id, d) => <Input id={id} aria-describedby={d} value={description} onChange={(e) => setDescription(e.target.value)} autoComplete="off" />}
        </Field>
      </form>
    </Dialog>
  );
}

/** The API has no "list team members" call, so membership is changed per person and the result reported inline. */
function TeamMembersDialog({ team, onOpenChange }: { team: Schemas["Team"] | null; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const members = useQuery({
    queryKey: keys.ws(ws, "members"),
    enabled: !!ws && !!team,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/members", { params: { path: { ws } } })),
  });
  const inTeam = useQuery({
    queryKey: keys.ws(ws, "team-members", team?.id ?? ""),
    enabled: !!ws && !!team,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/teams/{team_id}/members", { params: { path: { ws, team_id: team!.id } } })),
  });
  const ids = new Set((inTeam.data ?? []).map((m) => m.user_id));
  const change = useMutation({
    mutationFn: ({ user_id, add }: { user_id: string; add: boolean }) => {
      const params = { params: { path: { ws, team_id: team!.id, user_id } } };
      return unwrap(add ? api.PUT("/api/v1/workspaces/{ws}/teams/{team_id}/members/{user_id}", params) : api.DELETE("/api/v1/workspaces/{ws}/teams/{team_id}/members/{user_id}", params));
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.ws(ws, "team-members", team?.id ?? "") });
      qc.invalidateQueries({ queryKey: keys.ws(ws, "teams") });
    },
  });
  return (
    <Dialog
      open={!!team}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) change.reset();
      }}
      wide
      title={team ? `Members of ${team.name}` : "Team members"}
      description={inTeam.data ? `${ids.size} ${ids.size === 1 ? "member" : "members"}. Add or remove workspace members.` : "Add or remove workspace members."}
    >
      <QueryState query={members} skeleton={<TableSkeleton rows={3} cols={3} />} empty={<Empty title="This workspace has no members." />}>
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Member</Th>
                <Th>Team</Th>
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {list.map((m) => {
                const busy = (change.isPending && change.variables?.user_id === m.user_id) || inTeam.isPending;
                const member = ids.has(m.user_id);
                return (
                  <Tr key={m.user_id}>
                    <Td label="Member">
                      <span className="font-medium">{m.name}</span>
                      <div className="mono text-xs text-muted">{m.email}</div>
                      {change.error && change.variables?.user_id === m.user_id ? <p role="alert" className="text-xs text-error">{errorMessage(change.error)}</p> : null}
                    </Td>
                    <Td label="Team">{inTeam.isPending ? "–" : member ? <StatusPill status="approved" label="in team" /> : <span className="text-muted">not in team</span>}</Td>
                    <Td label="Actions" className="text-right whitespace-nowrap">
                      {member ? (
                        <Button size="sm" variant="ghost" disabled={busy} onClick={() => change.mutate({ user_id: m.user_id, add: false })}>
                          Remove
                        </Button>
                      ) : (
                        <Button size="sm" disabled={busy} onClick={() => change.mutate({ user_id: m.user_id, add: true })}>
                          Add
                        </Button>
                      )}
                    </Td>
                  </Tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </QueryState>
    </Dialog>
  );
}

export function TeamsTab() {
  const ws = useWsId();
  const qc = useQueryClient();
  const canManage = useCan("admin");
  const [creating, setCreating] = useState(false);
  const [managing, setManaging] = useState<Schemas["Team"] | null>(null);
  const [deleting, setDeleting] = useState<Schemas["Team"] | null>(null);
  const teams = useQuery({
    queryKey: keys.ws(ws, "teams"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/teams", { params: { path: { ws } } })),
  });
  const remove = useMutation({
    mutationFn: (team_id: string) => unwrap(api.DELETE("/api/v1/workspaces/{ws}/teams/{team_id}", { params: { path: { ws, team_id } } })),
    onSuccess: () => {
      setDeleting(null);
      qc.invalidateQueries({ queryKey: keys.ws(ws, "teams") });
    },
  });
  const newButton = canManage ? (
    <Button variant="primary" onClick={() => setCreating(true)}>
      <Plus aria-hidden className="size-4" /> New team
    </Button>
  ) : null;

  return (
    <Section title="Teams" description="Named groups of workspace members." actions={newButton}>
      <QueryState query={teams} empty={<Empty title="No teams yet." action={newButton}>Teams group members, for example by product area.</Empty>}>
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Team</Th>
                <Th numeric>Members</Th>
                <Th>Created</Th>
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {list.map((t) => (
                <Tr key={t.id}>
                  <Td label="Team">
                    <span className="font-medium">{t.name}</span>
                    {t.description ? <div className="text-xs text-ink-2">{t.description}</div> : null}
                  </Td>
                  <Td label="Members" numeric mono>
                    {t.member_count}
                  </Td>
                  <Td label="Created" className="text-ink-2">
                    <span title={dateTime(t.created_at)}>{relative(t.created_at)}</span>
                  </Td>
                  <Td label="Actions" className="text-right whitespace-nowrap">
                    {canManage ? (
                      <>
                        <Button size="sm" onClick={() => setManaging(t)}>
                          Manage members
                        </Button>{" "}
                        <Button size="sm" variant="danger" onClick={() => setDeleting(t)}>
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
      <CreateTeamDialog open={creating} onOpenChange={setCreating} />
      <TeamMembersDialog team={managing} onOpenChange={(o) => !o && setManaging(null)} />
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            remove.reset();
          }
        }}
        title={`Delete ${deleting?.name ?? "team"}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>The team is removed. Its members keep their workspace access and roles. This cannot be undone.</p>
            {remove.error ? <p role="alert" className="text-sm text-error">{errorMessage(remove.error)}</p> : null}
          </div>
        }
        confirmLabel="Delete team"
        loading={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting.id)}
      />
    </Section>
  );
}
