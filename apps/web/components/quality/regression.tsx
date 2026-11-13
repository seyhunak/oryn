"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Play } from "lucide-react";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useCan } from "@/lib/hooks";
import { Section } from "@/components/ui/page";
import { Empty } from "@/components/ui/states";
import { Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Field, Input, Select, Textarea } from "@/components/ui/field";

type Req = Schemas["RegressionReq"];
const BROWSERS: Schemas["Browser"][] = ["chromium", "firefox", "webkit"];

export function RegressionTab({ projectId }: { projectId: string }) {
  const router = useRouter();
  const canRun = useCan("engineer");
  const [files, setFiles] = useState("");
  const [minScore, setMinScore] = useState("");
  const [env, setEnv] = useState("");
  const [browser, setBrowser] = useState<Schemas["Browser"]>("chromium");
  const [lastReq, setLastReq] = useState<Req | null>(null);

  const envs = useQuery({
    queryKey: keys.project(projectId, "environments"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/environments", { params: { path: { id: projectId } } })),
  });

  const buildReq = (): Req => {
    const changed_files = files
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const n = minScore.trim() === "" ? null : Number(minScore);
    return { changed_files, min_score: n != null && Number.isFinite(n) ? n : null };
  };
  const scoreError = minScore.trim() !== "" && !Number.isFinite(Number(minScore)) ? "Enter a number, for example 0.5." : null;

  const select = useMutation({
    mutationFn: (body: Req) => unwrap(api.POST("/api/v1/projects/{id}/regression/select", { params: { path: { id: projectId } }, body })),
    onSuccess: (_d, body) => setLastReq(body),
  });
  const run = useMutation({
    mutationFn: (body: Schemas["RegressionRunReq"]) => unwrap(api.POST("/api/v1/projects/{id}/regression/run", { params: { path: { id: projectId } }, body })),
    onSuccess: (e) => router.push(`/executions/${e.id}`),
  });

  const result = select.data;
  const hasResult = !!result;

  return (
    <div className="flex flex-col gap-10">
      <Section
        title="Regression selection"
        description="Ranks approved scenarios by how closely they relate to the changed files: module matches, linked requirements, recent failures and a smoke baseline. The score and its reasons come from deterministic rules."
      >
        <form
          className="grid max-w-3xl grid-cols-1 gap-x-6 gap-y-2 md:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]"
          onSubmit={(e) => {
            e.preventDefault();
            if (!scoreError) select.mutate(buildReq());
          }}
        >
          <Field label="Changed files" hint="One path per line, as printed by git diff with the name-only flag. Leave empty to use the change set from the last repository analysis." className="md:row-span-2">
            {(id, d) => <Textarea id={id} aria-describedby={d} rows={7} className="font-mono text-sm" value={files} onChange={(e) => setFiles(e.target.value)} placeholder={"src/checkout/cart.ts\nsrc/api/orders.rs"} spellCheck={false} />}
          </Field>
          <Field label="Minimum score" hint="Higher means more closely related. The smoke baseline scores 0.5. Empty uses the server default." error={scoreError}>
            {(id, d) => <Input id={id} aria-describedby={d} inputMode="decimal" className="font-mono" value={minScore} onChange={(e) => setMinScore(e.target.value)} placeholder="0.5" />}
          </Field>
          <div className="flex items-start gap-2">
            <Button type="submit" variant={hasResult ? "secondary" : "primary"} loading={select.isPending}>
              {hasResult ? "Select again" : "Select scenarios"}
            </Button>
          </div>
        </form>
        {select.error ? <p role="alert" className="text-sm text-error">{errorMessage(select.error)}</p> : null}
      </Section>

      {result ? (
        <Section
          title="Selected scenarios"
          description={`${result.selections.length} of ${result.total_runnable} runnable scenarios selected, highest score first.`}
        >
          <div className="flex flex-col gap-2 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-ink-2">Changed files</span>
              {result.changed_files.length ? (
                result.changed_files.map((f) => (
                  <code key={f} className="rounded-full border border-rule px-2 text-xs">
                    {f}
                  </code>
                ))
              ) : (
                <span className="text-muted">none</span>
              )}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-ink-2">Changed modules</span>
              {result.changed_modules.length ? result.changed_modules.map((m) => <Tag key={m}>{m}</Tag>) : <span className="text-muted">none</span>}
            </div>
          </div>
          {result.selections.length === 0 ? (
            <Empty title="No scenario reached the minimum score.">Lower the minimum score, or list the changed files explicitly.</Empty>
          ) : (
            <>
              <Table>
                <thead>
                  <tr>
                    <Th numeric>Rank</Th>
                    <Th>Scenario</Th>
                    <Th numeric>Score</Th>
                    <Th>Reasons</Th>
                  </tr>
                </thead>
                <tbody>
                  {result.selections.map((s, i) => (
                    <Tr key={s.scenario_id}>
                      <Td label="Rank" numeric mono className="text-ink-2">
                        {i + 1}
                      </Td>
                      <Td label="Scenario" className="max-w-[26rem] font-medium">
                        {s.name}
                      </Td>
                      <Td label="Score" numeric mono>
                        {s.score.toFixed(2)}
                      </Td>
                      <Td label="Reasons">
                        <ul className="flex flex-col gap-0.5 py-1 text-xs text-ink-2">
                          {s.reasons.map((r, k) => (
                            <li key={k}>{r}</li>
                          ))}
                        </ul>
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
              {canRun ? (
                <form
                  className="flex flex-wrap items-end gap-4 border-t border-rule pt-4"
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (lastReq) run.mutate({ ...lastReq, environment_id: env || null, browser });
                  }}
                >
                  <Field label="Environment" className="w-56">
                    {(id) => (
                      <Select id={id} value={env} onChange={(e) => setEnv(e.target.value)}>
                        <option value="">Project default</option>
                        {(envs.data ?? []).map((x) => (
                          <option key={x.id} value={x.id}>
                            {x.name}
                            {x.is_default ? " (default)" : ""}
                          </option>
                        ))}
                      </Select>
                    )}
                  </Field>
                  <Field label="Browser" className="w-40">
                    {(id) => (
                      <Select id={id} value={browser} onChange={(e) => setBrowser(e.target.value as Schemas["Browser"])}>
                        {BROWSERS.map((b) => (
                          <option key={b} value={b}>
                            {b}
                          </option>
                        ))}
                      </Select>
                    )}
                  </Field>
                  <div className="flex min-h-[calc(36px+1lh+4px)] flex-col gap-1">
                    <Button type="submit" variant="primary" loading={run.isPending}>
                      <Play aria-hidden className="size-4" /> Run selected
                    </Button>
                  </div>
                  <p className="basis-full text-xs text-muted">The server re-selects with the same inputs when the run starts, so the run matches this list unless runs or scenarios changed in between.</p>
                  {run.error ? <p role="alert" className="basis-full text-sm text-error">{errorMessage(run.error)}</p> : null}
                </form>
              ) : null}
            </>
          )}
        </Section>
      ) : null}
    </div>
  );
}
