"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/dialog";
import { Field, Select } from "@/components/ui/field";
import { can, InlineError, type ProjectDetail } from "./common";

type Permission = Schemas["ProjectPermission"];
const ROLE_ORDER = ["viewer", "engineer", "admin", "owner"];

export function AccessTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const ws = useWsId();
  const qc = useQueryClient();
  const canManage = can(project, "members:manage");
  const [removing, setRemoving] = useState<Permission | null>(null);
  const perms = useQuery({
    queryKey: keys.project(id, "permissions"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/permissions", { params: { path: { id } } })),
  });
  const members = useQuery({
    queryKey: keys.ws(ws, "members"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/members", { params: { path: { ws } } })),
  });
  const teams = useQuery({
    queryKey: keys.ws(ws, "teams"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/teams", { params: { path: { ws } } })),
  });
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: keys.project(id, "permissions") });
    qc.invalidateQueries({ queryKey: keys.projects(ws) });
  };
  const remove = useMutation({
    mutationFn: (perm_id: string) => unwrap(api.DELETE("/api/v1/projects/{id}/permissions/{perm_id}", { params: { path: { id, perm_id } } })),
    onSuccess: () => {
      setRemoving(null);
      invalidate();
    },
  });
  const principal = (p: Permission) => {
    if (p.user_id) {
      const m = members.data?.find((x) => x.user_id === p.user_id);
      return { kind: "Member", name: m ? m.name : "Unknown member", detail: m?.email ?? p.user_id };
    }
    const t = teams.data?.find((x) => x.id === p.team_id);
    return { kind: "Team", name: t ? t.name : "Unknown team", detail: t ? `${t.member_count} members` : (p.team_id ?? "") };
  };

  return (
    <div className="flex flex-col gap-10">
      <Section
        title="Access"
        description={
          project.visibility === "private"
            ? "This project is private: only the members and teams granted below, and workspace admins and owners, can see it."
            : "Every workspace member can see this project with their workspace role. Grants below give a member or team a different role on this project. Make the project private in Settings to limit it to explicit grants and workspace admins."
        }
      >
        <QueryState
          query={perms}
          empty={
            <Empty title="No project grants.">
              {project.visibility === "private" ? "Only workspace admins and owners can see this project until someone is granted access." : "Members use their workspace role here."}
            </Empty>
          }
        >
          {(list) => (
            <Table>
              <thead>
                <tr>
                  <Th>Principal</Th>
                  <Th>Type</Th>
                  <Th>Project role</Th>
                  <Th>Granted</Th>
                  <Th>
                    <span className="visually-hidden">Actions</span>
                  </Th>
                </tr>
              </thead>
              <tbody>
                {list.map((p) => {
                  const who = principal(p);
                  return (
                    <Tr key={p.id}>
                      <Td label="Principal">
                        <div className="font-medium">{who.name}</div>
                        <div className="text-xs text-muted">{who.detail}</div>
                      </Td>
                      <Td label="Type">{who.kind}</Td>
                      <Td label="Project role">
                        <Tag>{p.role}</Tag>
                      </Td>
                      <Td label="Granted" className="text-ink-2">
                        {relative(p.created_at)}
                      </Td>
                      <Td label="Actions" className="text-right">
                        {canManage ? (
                          <Button variant="danger" size="sm" onClick={() => setRemoving(p)}>
                            Revoke
                          </Button>
                        ) : null}
                      </Td>
                    </Tr>
                  );
                })}
              </tbody>
            </Table>
          )}
        </QueryState>
      </Section>

      {canManage ? (
        <GrantForm projectId={id} members={members.data ?? []} teams={teams.data ?? []} loading={members.isPending || teams.isPending} loadError={members.error ?? teams.error} onGranted={invalidate} />
      ) : (
        <p className="text-sm text-ink-2">Workspace admins and owners can change access.</p>
      )}

      <ConfirmDialog
        open={!!removing}
        onOpenChange={(o) => {
          if (!o) {
            setRemoving(null);
            remove.reset();
          }
        }}
        title="Revoke access"
        body={
          <div className="flex flex-col gap-2">
            <p>
              {removing ? principal(removing).name : ""} loses the {removing?.role} role on this project.
              {project.visibility === "private" ? " On a private project they can no longer see it unless they are a workspace admin." : " They keep their workspace role."}
            </p>
            <InlineError error={remove.error} />
          </div>
        }
        confirmLabel="Revoke"
        loading={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </div>
  );
}

function GrantForm({
  projectId,
  members,
  teams,
  loading,
  loadError,
  onGranted,
}: {
  projectId: string;
  members: Schemas["Member"][];
  teams: Schemas["Team"][];
  loading: boolean;
  loadError: unknown;
  onGranted: () => void;
}) {
  const id = projectId;
  const [kind, setKind] = useState<"member" | "team">("member");
  const [who, setWho] = useState("");
  const [role, setRole] = useState<Schemas["Role"]>("engineer");
  const roles = useQuery({ queryKey: ["roles"], staleTime: 300_000, queryFn: () => unwrap(api.GET("/api/v1/roles")) });
  const roleList = (roles.data ?? []).map((r) => r.role).sort((a, b) => ROLE_ORDER.indexOf(a) - ROLE_ORDER.indexOf(b));
  const grant = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/projects/{id}/permissions", {
          params: { path: { id } },
          body: { role, user_id: kind === "member" ? who : null, team_id: kind === "team" ? who : null },
        }),
      ),
    onSuccess: () => {
      setWho("");
      onGranted();
    },
  });
  const perms = roles.data?.find((r) => r.role === role)?.permissions ?? [];

  return (
    <section className="flex flex-col gap-3">
      <h2>Grant access</h2>
      <form
        className="grid grid-cols-1 items-end gap-x-4 sm:grid-cols-[auto_minmax(0,1fr)_auto_auto]"
        onSubmit={(e) => {
          e.preventDefault();
          if (who) grant.mutate();
        }}
      >
        <Field label="Grant to">
          {(fid, d) => (
            <Select
              id={fid}
              aria-describedby={d}
              value={kind}
              onChange={(e) => {
                setKind(e.target.value as "member" | "team");
                setWho("");
              }}
            >
              <option value="member">Member</option>
              <option value="team">Team</option>
            </Select>
          )}
        </Field>
        <Field label={kind === "member" ? "Member" : "Team"} hint={!loading && kind === "team" && teams.length === 0 ? "No teams in this workspace yet." : undefined}>
          {(fid, d) => (
            <Select id={fid} aria-describedby={d} value={who} onChange={(e) => setWho(e.target.value)} disabled={loading}>
              <option value="">{loading ? "Loading" : "Choose"}</option>
              {kind === "member"
                ? members.map((m) => (
                    <option key={m.user_id} value={m.user_id}>
                      {m.name} ({m.email}, workspace {m.role})
                    </option>
                  ))
                : teams.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name} ({t.member_count} members)
                    </option>
                  ))}
            </Select>
          )}
        </Field>
        <Field label="Project role">
          {(fid, d) => (
            <Select id={fid} aria-describedby={d} value={role} onChange={(e) => setRole(e.target.value as Schemas["Role"])} disabled={roles.isPending}>
              {(roleList.length ? roleList : ROLE_ORDER).map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </Select>
          )}
        </Field>
        <Button type="submit" variant="primary" disabled={!who} loading={grant.isPending} className="mb-[calc(1lh+4px)]">
          Grant
        </Button>
      </form>
      {perms.length ? (
        <p className="text-xs text-muted">
          {role} can: <span className="mono">{perms.join(", ")}</span>
        </p>
      ) : null}
      <InlineError error={grant.error ?? loadError ?? roles.error} />
    </section>
  );
}
