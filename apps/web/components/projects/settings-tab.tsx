"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { api, unwrap } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { Section } from "@/components/ui/page";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/dialog";
import { Field, Input, Select, Textarea } from "@/components/ui/field";
import { cn } from "@/lib/cn";
import { can, InlineError, TINTS, type ProjectDetail } from "./common";

export function SettingsTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const ws = useWsId();
  const qc = useQueryClient();
  const router = useRouter();
  const canWrite = can(project, "project:write");
  const canDelete = can(project, "project:delete");
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description ?? "");
  const [visibility, setVisibility] = useState(project.visibility);
  const [color, setColor] = useState(project.color);
  const [confirming, setConfirming] = useState(false);
  const dirty = name !== project.name || description !== (project.description ?? "") || visibility !== project.visibility || color !== project.color;

  const save = useMutation({
    mutationFn: () => unwrap(api.PATCH("/api/v1/projects/{id}", { params: { path: { id } }, body: { name: name.trim(), description: description.trim(), visibility, color } })),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.project(id) });
      qc.invalidateQueries({ queryKey: keys.projects(ws) });
    },
  });
  const del = useMutation({
    mutationFn: () => unwrap(api.DELETE("/api/v1/projects/{id}", { params: { path: { id } } })),
    onSuccess: () => {
      qc.removeQueries({ queryKey: keys.project(id) });
      qc.invalidateQueries({ queryKey: keys.projects(ws) });
      router.push("/projects");
    },
  });

  return (
    <div className="flex max-w-2xl flex-col gap-10">
      <Section title="Project settings" description={canWrite ? undefined : "Your role on this project can read these settings but not change them."}>
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim() && dirty) save.mutate();
          }}
        >
          <Field label="Name">{(fid, d) => <Input id={fid} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} disabled={!canWrite} required />}</Field>
          <Field label="Description">
            {(fid, d) => <Textarea id={fid} aria-describedby={d} rows={3} value={description} onChange={(e) => setDescription(e.target.value)} disabled={!canWrite} />}
          </Field>
          <Field label="Visibility" hint={visibility === "private" ? "Only explicit grants (see Access) and workspace admins and owners can see it." : "Every workspace member can see it."}>
            {(fid, d) => (
              <Select id={fid} aria-describedby={d} value={visibility} onChange={(e) => setVisibility(e.target.value)} disabled={!canWrite}>
                <option value="workspace">Workspace</option>
                <option value="private">Private</option>
              </Select>
            )}
          </Field>
          <fieldset className="mb-4 flex flex-col gap-2" disabled={!canWrite}>
            <legend className="mb-1 text-sm font-medium text-ink-2">Identity colour</legend>
            <div className="flex flex-wrap gap-2" role="radiogroup" aria-label="Identity colour">
              {TINTS.map((t) => {
                const on = color === t.id;
                return (
                  <button
                    key={t.id}
                    type="button"
                    role="radio"
                    aria-checked={on}
                    aria-label={t.label}
                    title={t.label}
                    onClick={() => setColor(t.id)}
                    className={cn(
                      "inline-flex size-11 items-center justify-center rounded-[var(--radius-input)] border shadow-[inset_0_1px_0_var(--glint)] disabled:cursor-not-allowed disabled:opacity-55",
                      t.cls,
                      on ? "border-ink" : "border-rule",
                    )}
                  >
                    {on ? <Check aria-hidden className="size-4 text-ink" /> : null}
                  </button>
                );
              })}
            </div>
            <p className="text-xs text-muted">Identifies the project in lists. It never signals a status.</p>
          </fieldset>
          {canWrite ? (
            <div className="flex flex-wrap items-center gap-3">
              <Button type="submit" variant="primary" loading={save.isPending} disabled={!dirty || !name.trim()}>
                Save changes
              </Button>
              {save.isSuccess && !dirty ? (
                <span className="text-sm text-ink-2" aria-live="polite">
                  Saved.
                </span>
              ) : null}
            </div>
          ) : null}
          <InlineError error={save.error} />
        </form>
      </Section>

      {canDelete ? (
        <Section title="Delete project" description="Removes the project with its suites, scenarios, requirements, environments, repositories and execution history for every member. It cannot be restored from the interface.">
          <div>
            <Button variant="danger" onClick={() => setConfirming(true)}>
              Delete project
            </Button>
          </div>
        </Section>
      ) : null}

      <ConfirmDialog
        open={confirming}
        onOpenChange={(o) => {
          setConfirming(o);
          if (!o) del.reset();
        }}
        title={`Delete ${project.name}`}
        body={
          <div className="flex flex-col gap-2">
            <p>The project and every suite, scenario, requirement, environment, repository and execution in it disappear for every member. It cannot be restored from the interface.</p>
            <InlineError error={del.error} />
          </div>
        }
        confirmLabel="Delete project"
        loading={del.isPending}
        onConfirm={() => del.mutate()}
      />
    </div>
  );
}
