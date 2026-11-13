"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Field, Input, Textarea } from "@/components/ui/field";
import { Empty, QueryState } from "@/components/ui/states";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Section } from "@/components/ui/page";

/** Custom steps: a project phrase that expands to library steps. */
export function StepDefinitionsSection({ projectId }: { projectId: string }) {
  const qc = useQueryClient();
  const canWrite = useCan("engineer");
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<Schemas["StepDefinition"] | null>(null);
  const defs = useQuery({
    queryKey: keys.project(projectId, "step-definitions"),
    enabled: !!projectId,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/step-definitions", { params: { path: { id: projectId } } })),
  });
  const del = useMutation({
    mutationFn: (defId: string) => unwrap(api.DELETE("/api/v1/projects/{id}/step-definitions/{def_id}", { params: { path: { id: projectId, def_id: defId } } })),
    onSuccess: () => {
      setDeleting(null);
      qc.invalidateQueries({ queryKey: keys.project(projectId, "step-definitions") });
    },
  });

  return (
    <Section
      title="Custom steps"
      description="A custom step is a phrase your team writes in scenarios that expands to library steps. The body is validated against the step library."
      actions={canWrite ? <Button onClick={() => setCreating(true)}>New custom step</Button> : undefined}
    >
      <QueryState
        query={defs}
        empty={
          <Empty title="No custom steps yet." action={canWrite ? <Button onClick={() => setCreating(true)}>New custom step</Button> : undefined}>
            Name a sequence you repeat, such as signing in, and use it as a single step.
          </Empty>
        }
      >
        {(d) => (
          <Table>
            <thead>
              <tr>
                <Th>Phrase</Th>
                <Th>Expands to</Th>
                <Th>Created</Th>
                {canWrite ? <Th className="w-0" /> : null}
              </tr>
            </thead>
            <tbody>
              {d.map((s) => (
                <Tr key={s.id}>
                  <Td label="Phrase">
                    <span className="mono">{s.phrase}</span>
                    {s.description ? <div className="text-xs text-ink-2">{s.description}</div> : null}
                  </Td>
                  <Td label="Expands to">
                    <pre className="m-0 text-xs whitespace-pre-wrap text-ink-2">{s.body}</pre>
                  </Td>
                  <Td label="Created" className="text-ink-2">
                    {relative(s.created_at)}
                  </Td>
                  {canWrite ? (
                    <Td>
                      <Button size="sm" variant="ghost" aria-label={`Delete ${s.phrase}`} onClick={() => setDeleting(s)}>
                        <Trash2 aria-hidden className="size-4" />
                      </Button>
                    </Td>
                  ) : null}
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
      <CreateStepDialog projectId={projectId} open={creating} onOpenChange={setCreating} />
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            del.reset();
          }
        }}
        title="Delete custom step"
        body={
          <>
            <p>
              Scenarios that use <code>{deleting?.phrase}</code> will stop compiling and cannot run until the step is restored or rewritten.
            </p>
            {del.error ? (
              <p role="alert" className="mt-2 text-sm text-error">
                {errorMessage(del.error)}
              </p>
            ) : null}
          </>
        }
        confirmLabel="Delete step"
        loading={del.isPending}
        onConfirm={() => deleting && del.mutate(deleting.id)}
      />
    </Section>
  );
}

function CreateStepDialog({ projectId, open, onOpenChange }: { projectId: string; open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient();
  const [phrase, setPhrase] = useState("");
  const [description, setDescription] = useState("");
  const [body, setBody] = useState("");
  const create = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/step-definitions", { params: { path: { id: projectId } }, body: { phrase: phrase.trim(), description: description.trim(), body } })),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.project(projectId, "step-definitions") });
      setPhrase("");
      setDescription("");
      setBody("");
      onOpenChange(false);
    },
  });
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) create.reset();
        onOpenChange(o);
      }}
      title="New custom step"
      description="Write the phrase as authors will type it after Given, When or Then."
    >
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <Field label="Phrase" hint="For example: I sign in as the demo customer">
          {(id, d) => <Input id={id} aria-describedby={d} required value={phrase} onChange={(e) => setPhrase(e.target.value)} className="mono" />}
        </Field>
        <Field label="Description (optional)">{(id) => <Input id={id} value={description} onChange={(e) => setDescription(e.target.value)} />}</Field>
        <Field label="Library steps" hint="One step per line. The Given, When or Then keyword is optional.">
          {(id, d) => <Textarea id={id} aria-describedby={d} required rows={6} value={body} onChange={(e) => setBody(e.target.value)} className="mono h-auto py-2" placeholder={'I open "/login"\nI fill "Email" with "demo@northwind.test"'} />}
        </Field>
        {create.error ? (
          <p role="alert" className="rounded-[var(--radius-input)] bg-error-soft p-3 text-sm text-error">
            {errorMessage(create.error)}
          </p>
        ) : null}
        <div className="flex flex-wrap justify-end gap-2 border-t border-rule pt-4">
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="submit" variant="primary" loading={create.isPending} disabled={!phrase.trim() || !body.trim()}>
            Create step
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
