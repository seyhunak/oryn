"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan, useWorkspace, useWsId } from "@/lib/hooks";
import { dateTime, relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Field, Input, Select } from "@/components/ui/field";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { CopyField, ShownOnce } from "./copy-field";
import { ROLES } from "./members-tab";

type Role = Schemas["Role"];
const RANK: Record<string, number> = { viewer: 0, engineer: 1, admin: 2, owner: 3 };
const EXPIRY = [
  { v: "30", l: "30 days" },
  { v: "90", l: "90 days" },
  { v: "365", l: "1 year" },
  { v: "", l: "Never" },
];

function CreateTokenDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const { role: myRole } = useWorkspace();
  const allowed = ROLES.filter((r) => myRole != null && RANK[r] <= RANK[myRole]);
  const [name, setName] = useState("");
  const [role, setRole] = useState<Role>("engineer");
  const [expiry, setExpiry] = useState("90");
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/workspaces/{ws}/tokens", {
          params: { path: { ws } },
          body: { name: name.trim(), role, expires_in_days: expiry ? Number(expiry) : null },
        }),
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.ws(ws, "tokens") }),
  });
  const close = (o: boolean) => {
    onOpenChange(o);
    if (!o) {
      create.reset();
      setName("");
      setRole("engineer");
      setExpiry("90");
    }
  };
  const created = create.data;
  return (
    <Dialog
      open={open}
      onOpenChange={close}
      title={created ? `Token ${created.token.name} created` : "New API token"}
      description={created ? undefined : "For the CLI, CI pipelines and scripts. Send it as Authorization: Bearer <token>."}
      footer={
        created ? (
          <Button variant="primary" onClick={() => close(false)}>
            I have copied the token
          </Button>
        ) : (
          <>
            <Button onClick={() => close(false)}>Cancel</Button>
            <Button variant="primary" type="submit" form="create-token" loading={create.isPending} disabled={!name.trim()}>
              Create token
            </Button>
          </>
        )
      }
    >
      {created ? (
        <div className="flex flex-col gap-4">
          <ShownOnce what="token" />
          <CopyField label="API token" value={created.secret} />
          <p className="text-sm text-ink-2">
            Role ceiling <span className="font-medium">{created.token.role}</span>
            {created.token.expires_at ? (
              <>
                , expires <span className="mono">{dateTime(created.token.expires_at)}</span>
              </>
            ) : (
              ", no expiry"
            )}
            .
          </p>
        </div>
      ) : (
        <form
          id="create-token"
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate();
          }}
        >
          <Field label="Name" hint="Where it is used, for example github-actions-nightly.">
            {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required autoFocus />}
          </Field>
          <Field label="Role ceiling" hint="The token never acts above this role, and never above your own role at the time it is used.">
            {(id, d) => (
              <Select id={id} aria-describedby={d} value={role} onChange={(e) => setRole(e.target.value as Role)}>
                {allowed.map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Expires" error={create.error ? errorMessage(create.error) : null}>
            {(id, d) => (
              <Select id={id} aria-describedby={d} value={expiry} onChange={(e) => setExpiry(e.target.value)}>
                {EXPIRY.map((x) => (
                  <option key={x.l} value={x.v}>
                    {x.l}
                  </option>
                ))}
              </Select>
            )}
          </Field>
        </form>
      )}
    </Dialog>
  );
}

function tokenState(t: Schemas["ApiToken"]) {
  if (t.revoked_at) return <StatusPill status="revoked" />;
  if (t.expires_at && new Date(t.expires_at).getTime() < Date.now()) return <StatusPill status="cancelled" label="expired" />;
  return <StatusPill status="ready" label="active" />;
}

export function TokensTab() {
  const ws = useWsId();
  const qc = useQueryClient();
  const { role: myRole, me } = useWorkspace();
  const seesAll = useCan("admin");
  const [creating, setCreating] = useState(false);
  const [revoking, setRevoking] = useState<Schemas["ApiToken"] | null>(null);
  const tokens = useQuery({
    queryKey: keys.ws(ws, "tokens"),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/tokens", { params: { path: { ws } } })),
  });
  const members = useQuery({
    queryKey: keys.ws(ws, "members"),
    enabled: !!ws && seesAll,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/members", { params: { path: { ws } } })),
  });
  const ownerName = (uid: string) => (uid === me?.user.id ? "you" : (members.data?.find((m) => m.user_id === uid)?.name ?? "another member"));
  const revoke = useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE("/api/v1/workspaces/{ws}/tokens/{id}", { params: { path: { ws, id } } })),
    onSuccess: () => {
      setRevoking(null);
      qc.invalidateQueries({ queryKey: keys.ws(ws, "tokens") });
    },
  });
  const newButton = (
    <Button variant="primary" onClick={() => setCreating(true)}>
      <Plus aria-hidden className="size-4" /> New token
    </Button>
  );
  return (
    <Section
      title="API tokens"
      description={
        <>
          {seesAll ? "Every API token in this workspace; as an admin you can revoke any of them." : "Your API tokens for this workspace."} A token acts with its effective role: the lower of its role ceiling and its owner&apos;s current workspace role
          {myRole ? (
            <>
              {" "}
              (yours is <span className="font-medium">{myRole}</span>)
            </>
          ) : null}
          . Lowering a member&apos;s role lowers their tokens with it.
        </>
      }
      actions={newButton}
    >
      <QueryState query={tokens} empty={<Empty title="No API tokens." action={newButton}>Create one for the CLI or a CI pipeline.</Empty>}>
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Token</Th>
                <Th>State</Th>
                {seesAll ? <Th>Owner</Th> : null}
                <Th>Role ceiling</Th>
                <Th>Effective role</Th>
                <Th>Last used</Th>
                <Th>Expires</Th>
                <Th>Created</Th>
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {list.map((t) => {
                const mine = t.user_id === me?.user.id;
                const effective = !mine ? null : myRole && RANK[myRole] < RANK[t.role] ? myRole : t.role;
                return (
                  <Tr key={t.id}>
                    <Td label="Token" className="font-medium">
                      {t.name}
                    </Td>
                    <Td label="State">{tokenState(t)}</Td>
                    {seesAll ? (
                      <Td label="Owner" className="text-ink-2">
                        {ownerName(t.user_id)}
                      </Td>
                    ) : null}
                    <Td label="Role ceiling" className="text-ink-2">
                      {t.role}
                    </Td>
                    <Td label="Effective role">{effective ?? <span className="text-muted" title="Depends on the owner's current role">capped by owner&apos;s role</span>}</Td>
                    <Td label="Last used" className="text-ink-2">
                      <span title={dateTime(t.last_used_at)}>{relative(t.last_used_at)}</span>
                    </Td>
                    <Td label="Expires" mono className="text-xs text-ink-2">
                      {t.expires_at ? dateTime(t.expires_at) : "never"}
                    </Td>
                    <Td label="Created" mono className="text-xs text-ink-2">
                      {dateTime(t.created_at)}
                    </Td>
                    <Td label="Actions" className="text-right">
                      {!t.revoked_at ? (
                        <Button size="sm" variant="danger" onClick={() => setRevoking(t)}>
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
      <CreateTokenDialog open={creating} onOpenChange={setCreating} />
      <ConfirmDialog
        open={!!revoking}
        onOpenChange={(o) => {
          if (!o) {
            setRevoking(null);
            revoke.reset();
          }
        }}
        title={`Revoke ${revoking?.name ?? "token"}?`}
        body={
          <div className="flex flex-col gap-2">
            <p>Requests using this token are refused from now on. This cannot be undone; create a new token if you need access again.</p>
            {revoke.error ? <p role="alert" className="text-sm text-error">{errorMessage(revoke.error)}</p> : null}
          </div>
        }
        confirmLabel="Revoke token"
        loading={revoke.isPending}
        onConfirm={() => revoking && revoke.mutate(revoking.id)}
      />
    </Section>
  );
}
