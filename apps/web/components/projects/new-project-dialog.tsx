"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { Dialog } from "@/components/ui/dialog";
import { Field, Input, Select, Textarea } from "@/components/ui/field";
import { Button } from "@/components/ui/button";
import { InlineError } from "./common";

export function NewProjectDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const router = useRouter();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [visibility, setVisibility] = useState("workspace");
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/workspaces/{ws}/projects", {
          params: { path: { ws } },
          body: { name: name.trim(), description: description.trim() || null, visibility },
        }),
      ),
    onSuccess: (p) => {
      qc.invalidateQueries({ queryKey: keys.projects(ws) });
      onOpenChange(false);
      router.push(`/projects/${p.id}`);
    },
  });
  const close = (o: boolean) => {
    if (!o) {
      setName("");
      setDescription("");
      setVisibility("workspace");
      create.reset();
    }
    onOpenChange(o);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={close}
      title="New project"
      description="A project holds the application under test: its repository, requirements, environments and suites."
      footer={
        <>
          <Button onClick={() => close(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="new-project" loading={create.isPending} disabled={!name.trim()}>
            Create project
          </Button>
        </>
      }
    >
      <form
        id="new-project"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) create.mutate();
        }}
      >
        <Field label="Name">{(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} required autoFocus />}</Field>
        <Field label="Description" hint="Optional. One line on what the application does.">
          {(id, d) => <Textarea id={id} aria-describedby={d} rows={3} value={description} onChange={(e) => setDescription(e.target.value)} />}
        </Field>
        <Field label="Visibility" hint={visibility === "private" ? "Only explicit grants and workspace admins can see it." : "Every workspace member can see it."}>
          {(id, d) => (
            <Select id={id} aria-describedby={d} value={visibility} onChange={(e) => setVisibility(e.target.value)}>
              <option value="workspace">Workspace</option>
              <option value="private">Private</option>
            </Select>
          )}
        </Field>
        <InlineError error={create.error} />
      </form>
    </Dialog>
  );
}
