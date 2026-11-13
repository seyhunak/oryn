"use client";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { Section } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { Field, Input, Textarea } from "@/components/ui/field";
import { can, InlineError, type ProjectDetail } from "./common";

type TestData = Schemas["TestData"];
const NAME = /^[A-Za-z0-9_.-]+$/;

function shape(data: unknown): string {
  if (Array.isArray(data)) return `list of ${data.length}`;
  if (data && typeof data === "object") {
    const k = Object.keys(data);
    return k.length ? k.slice(0, 6).join(", ") + (k.length > 6 ? `, +${k.length - 6}` : "") : "empty object";
  }
  return typeof data;
}

export function TestDataTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const qc = useQueryClient();
  const canWrite = can(project, "test:write");
  const [editing, setEditing] = useState<TestData | "new" | null>(null);
  const [deleting, setDeleting] = useState<TestData | null>(null);
  const data = useQuery({
    queryKey: keys.project(id, "test-data"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/test-data", { params: { path: { id } } })),
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: keys.project(id, "test-data") });
  const del = useMutation({
    mutationFn: (data_id: string) => unwrap(api.DELETE("/api/v1/projects/{id}/test-data/{data_id}", { params: { path: { id, data_id } } })),
    onSuccess: () => {
      setDeleting(null);
      invalidate();
    },
  });

  return (
    <Section
      title="Test data"
      description="Named JSON data sets that scenarios read. Saving a set with an existing name replaces it. Do not store secrets here; use environment secrets."
      actions={
        canWrite ? (
          <Button variant="primary" onClick={() => setEditing("new")}>
            Add data set
          </Button>
        ) : null
      }
    >
      <QueryState
        query={data}
        empty={
          <Empty title="No test data yet." action={canWrite ? <Button onClick={() => setEditing("new")}>Add a data set</Button> : undefined}>
            A data set is a named JSON value, such as a list of products or customer records, that scenarios can use.
          </Empty>
        }
      >
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Name</Th>
                <Th>Description</Th>
                <Th>Contents</Th>
                <Th>Updated</Th>
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {list.map((t) => (
                <Tr key={t.id}>
                  <Td label="Name" mono className="font-medium">
                    {t.name}
                  </Td>
                  <Td label="Description" className="text-ink-2">
                    {t.description || <span className="text-muted">none</span>}
                  </Td>
                  <Td label="Contents" mono className="text-xs text-ink-2">
                    {shape(t.data)}
                  </Td>
                  <Td label="Updated" className="text-ink-2">
                    {relative(t.updated_at)}
                  </Td>
                  <Td label="Actions" className="text-right whitespace-nowrap">
                    <Button variant="ghost" size="sm" onClick={() => setEditing(t)}>
                      {canWrite ? "Edit" : "View"}
                    </Button>
                    {canWrite ? (
                      <Button variant="danger" size="sm" onClick={() => setDeleting(t)}>
                        Delete
                      </Button>
                    ) : null}
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>

      {editing ? <DataForm key={editing === "new" ? "new" : editing.id} projectId={id} editing={editing} readOnly={!canWrite} onClose={() => setEditing(null)} onSaved={invalidate} /> : null}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            del.reset();
          }
        }}
        title={`Delete ${deleting?.name ?? "data set"}`}
        body={
          <div className="flex flex-col gap-2">
            <p>Scenarios that read this data set will fail until it is added again. This cannot be undone.</p>
            <InlineError error={del.error} />
          </div>
        }
        confirmLabel="Delete data set"
        loading={del.isPending}
        onConfirm={() => deleting && del.mutate(deleting.id)}
      />
    </Section>
  );
}

function DataForm({ projectId, editing, readOnly, onClose, onSaved }: { projectId: string; editing: TestData | "new"; readOnly: boolean; onClose: () => void; onSaved: () => void }) {
  const id = projectId;
  const isNew = editing === "new";
  const [name, setName] = useState(isNew ? "" : editing.name);
  const [description, setDescription] = useState(isNew ? "" : editing.description);
  const [json, setJson] = useState(isNew ? "{\n  \n}" : JSON.stringify(editing.data, null, 2));
  let parsed: unknown = undefined;
  let jsonError: string | null = null;
  try {
    parsed = JSON.parse(json);
  } catch (e) {
    jsonError = e instanceof Error ? e.message : "Not valid JSON.";
  }
  const nameError = name && !NAME.test(name.trim()) ? "Use letters, digits, - _ and . only." : null;
  const save = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/test-data", { params: { path: { id } }, body: { name: name.trim(), description: description.trim(), data: parsed as never } })),
    onSuccess: () => {
      onSaved();
      onClose();
    },
  });
  const valid = !!name.trim() && !nameError && !jsonError;

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      wide
      title={isNew ? "Add data set" : readOnly ? editing.name : `Edit ${editing.name}`}
      footer={
        readOnly ? (
          <Button onClick={onClose}>Close</Button>
        ) : (
          <>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" type="submit" form="data-form" loading={save.isPending} disabled={!valid}>
              {isNew ? "Add data set" : "Save changes"}
            </Button>
          </>
        )
      }
    >
      <form
        id="data-form"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (valid && !readOnly) save.mutate();
        }}
      >
        <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
          <Field label="Name" error={nameError} hint={isNew ? "Letters, digits, - _ and ." : "Names do not change; save under a new name to copy."}>
            {(fid, d) => (
              <Input id={fid} aria-describedby={d} aria-invalid={!!nameError} className="mono" value={name} onChange={(e) => setName(e.target.value)} disabled={!isNew || readOnly} required autoFocus={isNew} placeholder="customers" />
            )}
          </Field>
          <Field label="Description" hint="Optional.">
            {(fid, d) => <Input id={fid} aria-describedby={d} value={description} onChange={(e) => setDescription(e.target.value)} disabled={readOnly} />}
          </Field>
        </div>
        <Field label="Value (JSON)" error={jsonError} hint="Limited to 1 MB.">
          {(fid, d) => (
            <Textarea
              id={fid}
              aria-describedby={d}
              aria-invalid={!!jsonError}
              rows={16}
              spellCheck={false}
              className="mono text-xs"
              value={json}
              onChange={(e) => setJson(e.target.value)}
              readOnly={readOnly}
            />
          )}
        </Field>
        <InlineError error={save.error} />
      </form>
    </Dialog>
  );
}
