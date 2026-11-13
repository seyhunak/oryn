"use client";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/field";
import { CopyField, ShownOnce } from "@/components/settings/copy-field";

export const WORKER_IMAGE = "ghcr.io/oryn-dev/oryn-worker:latest";
const RUNNER_PLACEHOLDER = "http://playwright-runner:7400";

function apiOrigin() {
  return typeof window === "undefined" ? "http://localhost:8080" : window.location.origin;
}

function StartInstructions({ creds, name }: { creds: Schemas["WorkerCredentials"]; name: string }) {
  const apiUrl = apiOrigin();
  const env = `ORYN_API_URL=${apiUrl}\nORYN_WORKER_TOKEN=${creds.token}\nORYN_RUNNER_URL=${RUNNER_PLACEHOLDER}`;
  const slug = name.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-+|-+$/g, "") || "worker";
  const docker = [
    `docker run -d --name oryn-worker-${slug} \\`,
    `  -e ORYN_API_URL=${apiUrl} \\`,
    `  -e ORYN_WORKER_TOKEN=${creds.token} \\`,
    `  -e ORYN_RUNNER_URL=${RUNNER_PLACEHOLDER} \\`,
    `  ${WORKER_IMAGE}`,
  ].join("\n");
  return (
    <div className="flex flex-col gap-4">
      <ShownOnce what="worker token" />
      <CopyField label="Worker token" value={creds.token} />
      <div className="flex flex-col gap-2 text-sm text-ink-2">
        <p>Start the worker with these environment variables:</p>
        <ul className="flex list-disc flex-col gap-1 pl-5">
          <li>
            <code className="mono">ORYN_API_URL</code>: the ORYN API as the worker can reach it. Inside docker compose that is <code className="mono">http://api:8080</code>.
          </li>
          <li>
            <code className="mono">ORYN_WORKER_TOKEN</code>: the token above. It identifies this worker.
          </li>
          <li>
            <code className="mono">ORYN_RUNNER_URL</code>: the Playwright runner that executes browser scenarios. Without it the worker runs API scenarios only.
          </li>
        </ul>
      </div>
      <CopyField label="Environment variables" value={env} multiline />
      <CopyField label="Docker command" value={docker} multiline />
    </div>
  );
}

export function AddWorkerDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const create = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/workspaces/{ws}/workers", { params: { path: { ws } }, body: { name: name.trim() } })),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.ws(ws, "workers") }),
  });
  const close = (o: boolean) => {
    onOpenChange(o);
    if (!o) {
      create.reset();
      setName("");
    }
  };
  const creds = create.data;
  return (
    <Dialog
      open={open}
      onOpenChange={close}
      wide={!!creds}
      title={creds ? `Worker ${name} registered` : "Add worker"}
      description={creds ? undefined : "Registers a worker for this workspace and issues its token. Only this workspace's jobs run on it."}
      footer={
        creds ? (
          <Button variant="primary" onClick={() => close(false)}>
            I have copied the token
          </Button>
        ) : (
          <>
            <Button onClick={() => close(false)}>Cancel</Button>
            <Button variant="primary" type="submit" form="add-worker" loading={create.isPending} disabled={!name.trim()}>
              Add worker
            </Button>
          </>
        )
      }
    >
      {creds ? (
        <StartInstructions creds={creds} name={name} />
      ) : (
        <form
          id="add-worker"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate();
          }}
        >
          <Field label="Name" hint="Shown in run evidence, for example ci-runner-02." error={create.error ? errorMessage(create.error) : null}>
            {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required autoFocus />}
          </Field>
        </form>
      )}
    </Dialog>
  );
}
