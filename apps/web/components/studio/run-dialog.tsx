"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Play } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Input, Select } from "@/components/ui/field";
import { Skeleton } from "@/components/ui/states";

const BROWSERS: Schemas["Browser"][] = ["chromium", "firefox", "webkit"];

export function useEnvironments(projectId: string, enabled = true) {
  return useQuery({
    queryKey: keys.project(projectId, "environments"),
    enabled: !!projectId && enabled,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/environments", { params: { path: { id: projectId } } })),
  });
}

/** Start an execution of one suite. Only approved, executable scenarios run; the server says which ones block it. */
export function RunDialog({ projectId, suite, open, onOpenChange }: { projectId: string; suite: { id: string; name: string } | null; open: boolean; onOpenChange: (o: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={suite ? `Run ${suite.name}` : "Run suite"} description="Runs every approved, executable scenario in this suite.">
      {open && suite ? <RunForm projectId={projectId} suite={suite} onCancel={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function RunForm({ projectId, suite, onCancel }: { projectId: string; suite: { id: string; name: string }; onCancel: () => void }) {
  const router = useRouter();
  const qc = useQueryClient();
  const envs = useEnvironments(projectId);
  const [envId, setEnvId] = useState<string>("");
  const [browser, setBrowser] = useState<Schemas["Browser"]>("chromium");
  const [retry, setRetry] = useState(false);
  const [attempts, setAttempts] = useState(2);
  const [video, setVideo] = useState(false);
  const [trace, setTrace] = useState(true);
  const [shots, setShots] = useState(false);
  const [title, setTitle] = useState("");
  const defaultEnv = envs.data?.find((e) => e.is_default) ?? envs.data?.[0];
  const environment = envId || defaultEnv?.id || "";

  const run = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/projects/{id}/executions", {
          params: { path: { id: projectId } },
          body: {
            suite_id: suite.id,
            environment_id: environment || null,
            browser,
            retry_on_failure: retry,
            max_attempts: retry ? attempts : 1,
            record_video: video,
            record_trace: trace,
            screenshots_every_step: shots,
            title: title.trim() || null,
            trigger: "manual",
          },
        }),
      ),
    onSuccess: (e) => {
      qc.invalidateQueries({ queryKey: ["ws"] });
      router.push(`/executions/${e.id}`);
    },
  });

  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        run.mutate();
      }}
    >
      {envs.isPending ? (
        <Skeleton className="h-9" />
      ) : envs.error ? (
        <p role="alert" className="text-sm text-error">
          Environments could not be loaded: <code>{errorMessage(envs.error)}</code>
        </p>
      ) : (
        <Field label="Environment" hint={envs.data?.length ? undefined : "This project has no environments. Add one under Projects."}>
          {(id, d) => (
            <Select id={id} aria-describedby={d} value={environment} onChange={(e) => setEnvId(e.target.value)} disabled={!envs.data?.length}>
              {envs.data?.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                  {e.is_default ? " (default)" : ""}
                  {e.base_url ? ` · ${e.base_url}` : ""}
                </option>
              ))}
            </Select>
          )}
        </Field>
      )}
      <Field label="Browser">
        {(id) => (
          <Select id={id} value={browser} onChange={(e) => setBrowser(e.target.value as Schemas["Browser"])}>
            {BROWSERS.map((b) => (
              <option key={b} value={b}>
                {b[0].toUpperCase() + b.slice(1)}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <fieldset className="flex flex-col">
        <legend className="text-sm font-medium text-ink-2">Retries</legend>
        <Checkbox label="Retry failed scenarios" checked={retry} onChange={(e) => setRetry(e.target.checked)} />
        {retry ? (
          <Field label="Total attempts per scenario" hint="Errors and timeouts are always retried up to this number; with this option failures are too.">
            {(id, d) => <Input id={id} aria-describedby={d} type="number" min={2} max={5} value={attempts} onChange={(e) => setAttempts(Math.max(2, Math.min(5, Number(e.target.value) || 2)))} className="w-24" />}
          </Field>
        ) : null}
      </fieldset>
      <fieldset className="flex flex-col">
        <legend className="text-sm font-medium text-ink-2">Evidence</legend>
        <Checkbox label="Record a trace" checked={trace} onChange={(e) => setTrace(e.target.checked)} />
        <Checkbox label="Record video" checked={video} onChange={(e) => setVideo(e.target.checked)} />
        <Checkbox label="Screenshot every step" checked={shots} onChange={(e) => setShots(e.target.checked)} />
      </fieldset>
      <Field label="Title (optional)" hint="Shown in the Executions list.">
        {(id, d) => <Input id={id} aria-describedby={d} value={title} onChange={(e) => setTitle(e.target.value)} maxLength={200} />}
      </Field>
      {run.error ? (
        <p role="alert" className="rounded-[var(--radius-input)] bg-error-soft p-3 text-sm text-error">
          {errorMessage(run.error)}
        </p>
      ) : null}
      <div className="flex flex-wrap justify-end gap-2 border-t border-rule pt-4">
        <Button onClick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" loading={run.isPending} disabled={envs.isPending}>
          <Play aria-hidden className="size-4" /> Start run
        </Button>
      </div>
    </form>
  );
}
