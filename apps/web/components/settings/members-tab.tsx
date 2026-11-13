"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWorkspace, useWsId } from "@/lib/hooks";
import { dateTime, relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState, TableSkeleton } from "@/components/ui/states";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Field, Input, Select } from "@/components/ui/field";
import { ConfirmDialog } from "@/components/ui/dialog";

type Role = Schemas["Role"];
export const ROLES: Role[] = ["viewer", "engineer", "admin", "owner"];

export function useRoles() {
  return useQuery({ queryKey: ["roles"], staleTime: 3_600_000, queryFn: () => unwrap(api.GET("/api/v1/roles")) });
}

export function RolesReference() {
  const roles = useRoles();
  return (
    <Section title="Roles" description="What each role may do. Permissions are enforced by the server.">
      <QueryState query={roles} skeleton={<TableSkeleton rows={4} cols={2} />}>
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Role</Th>
                <Th>Permissions</Th>
              </tr>
            </thead>
            <tbody>
              {list.map((r) => (
                <Tr key={r.role}>
                  <Td label="Role" className="font-medium">
                    {r.role}
                  </Td>
                  <Td label="Permissions">
                    <span className="mono text-xs leading-relaxed text-ink-2">{r.permissions.join(", ")}</span>
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

function AddMemberForm() {
  const ws = useWsId();
  const qc = useQueryClient();
  const { role: myRole } = useWorkspace();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<Role>("engineer");
  const add = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces/{ws}/members", { params: { path: { ws } }, body: { email: email.trim(), role } })),
    onSuccess: () => {
      setEmail("");
      qc.invalidateQueries({ queryKey: keys.ws(ws, "members") });
    },
  });
  const assignable = ROLES.filter((r) => r !== "owner" || myRole === "owner");
  return (
    <form
      className="flex flex-wrap items-end gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        if (email.trim()) add.mutate();
      }}
    >
      <Field label="Email" hint="The person must already have an ORYN account on this instance." error={add.error ? errorMessage(add.error) : null} className="w-full max-w-sm">
        {(id, d) => <Input id={id} aria-describedby={d} type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} required />}
      </Field>
      <Field label="Role" className="w-40">
        {(id) => (
          <Select id={id} value={role} onChange={(e) => setRole(e.target.value as Role)}>
            {assignable.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <div className="flex min-h-[calc(36px+1lh+4px)] flex-col">
        <Button type="submit" variant="primary" loading={add.isPending} disabled={!email.trim()}>
          Add member
        </Button>
      </div>
    </form>
  );
}

export function MembersTab() {
  const ws = useWsId();
  const qc = useQueryClient();
  const { me, role: myRole } = useWorkspace();
  const canManage = useCan("admin");
  const [removing, setRemoving] = useState<Schemas["Member"] | null>(null);
  const members = useQuery({
    queryKey: keys.ws(ws, "members"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/members", { params: { path: { ws } } })),
  });
  const setRole = useMutation({
    mutationFn: (v: { user_id: string; role: Role }) => unwrap(api.PATCH("/api/v1/workspaces/{ws}/members/{user_id}", { params: { path: { ws, user_id: v.user_id } }, body: { role: v.role } })),
    onSettled: () => qc.invalidateQueries({ queryKey: keys.ws(ws, "members") }),
  });
  const remove = useMutation({
    mutationFn: (user_id: string) => unwrap(api.DELETE("/api/v1/workspaces/{ws}/members/{user_id}", { params: { path: { ws, user_id } } })),
    onSuccess: () => {
      setRemoving(null);
      qc.invalidateQueries({ queryKey: keys.ws(ws, "members") });
    },
  });
  const assignable = ROLES.filter((r) => r !== "owner" || myRole === "owner");

  return (
    <div className="flex flex-col gap-10">
      <Section title="Members" description="People with access to this workspace and their role.">
        {canManage ? <AddMemberForm /> : null}
        <QueryState query={members} empty={<Empty title="No members.">Add people by the email they sign in with.</Empty>}>
          {(list) => (
            <Table>
              <thead>
                <tr>
                  <Th>Member</Th>
                  <Th>Role</Th>
                  <Th>Added</Th>
                  <Th>
                    <span className="visually-hidden">Actions</span>
                  </Th>
                </tr>
              </thead>
              <tbody>
                {list.map((m) => {
                  const self = m.user_id === me?.user.id;
                  const editable = canManage && !self && (m.role !== "owner" || myRole === "owner");
                  return (
                    <Tr key={m.user_id}>
                      <Td label="Member">
                        <span className="font-medium">{m.name}</span>
                        {self ? <span className="ml-2 text-xs text-muted">you</span> : null}
                        <div className="mono text-xs text-muted">{m.email}</div>
                      </Td>
                      <Td label="Role">
                        {editable ? (
                          <>
                            <label className="visually-hidden" htmlFor={`role-${m.user_id}`}>
                              Role for {m.name}
                            </label>
                            <select
                              id={`role-${m.user_id}`}
                              className="field h-9 w-auto"
                              value={m.role}
                              disabled={setRole.isPending && setRole.variables?.user_id === m.user_id}
                              onChange={(e) => setRole.mutate({ user_id: m.user_id, role: e.target.value as Role })}
                            >
                              {(assignable.includes(m.role as Role) ? assignable : [...assignable, m.role as Role]).map((r) => (
                                <option key={r} value={r}>
                                  {r}
                                </option>
                              ))}
                            </select>
                            {setRole.error && setRole.variables?.user_id === m.user_id ? <p role="alert" className="mt-1 text-xs text-error">{errorMessage(setRole.error)}</p> : null}
                          </>
                        ) : (
                          <span className="text-ink-2">{m.role}</span>
                        )}
                      </Td>
                      <Td label="Added" className="text-ink-2">
                        <span title={dateTime(m.created_at)}>{relative(m.created_at)}</span>
                      </Td>
                      <Td label="Actions" className="text-right">
                        {editable ? (
                          <Button size="sm" variant="danger" onClick={() => setRemoving(m)}>
                            Remove
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
      <RolesReference />
      <ConfirmDialog
        open={!!removing}
        onOpenChange={(o) => {
          if (!o) {
            setRemoving(null);
            remove.reset();
          }
        }}
        title={`Remove ${removing?.name ?? "member"}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>
              {removing?.email} loses access to this workspace at once. You can add them again later.
            </p>
            {remove.error ? <p role="alert" className="text-sm text-error">{errorMessage(remove.error)}</p> : null}
          </div>
        }
        confirmLabel="Remove member"
        loading={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.user_id)}
      />
    </div>
  );
}
