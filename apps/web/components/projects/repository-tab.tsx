"use client";
import { useEffect, useRef, useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Search } from "lucide-react";
import { api, unwrap, uploadRaw, type Schemas } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { bytes, dateTime, relative } from "@/lib/format";
import { Meta, Section } from "@/components/ui/page";
import { Empty, ErrorState, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { Field, Input, Select } from "@/components/ui/field";
import { can, InlineError, MonoList, type ProjectDetail } from "./common";

type Repository = Schemas["Repository"];
const BUSY = ["importing", "analyzing"];

/** Shape of `ApplicationContext.analysis` (crates/oryn-intelligence/src/analysis.rs `Analysis`). */
type Analysis = {
  file_count?: number;
  total_bytes?: number;
  truncated?: boolean;
  languages?: Record<string, { files: number; lines: number }>;
  frameworks?: string[];
  dependencies?: { ecosystem: string; name: string; version?: string | null; dev: boolean }[];
  routes?: { method: string; path: string; file: string; kind: string }[];
  openapi?: { file: string; title?: string | null; version?: string | null; endpoints: string[] }[];
  test_files?: string[];
  feature_files?: string[];
  docs?: string[];
  modules?: { name: string; path_prefix: string; kind: string; file_count: number }[];
  changes?: { added: string[]; modified: string[]; removed: string[] };
};

export function RepositoryTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const qc = useQueryClient();
  const ws = useWsId();
  const canWrite = can(project, "project:write");
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState<Repository | null>(null);
  const repos = useQuery({
    queryKey: keys.project(id, "repositories"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/repositories", { params: { path: { id } } })),
    refetchInterval: (q) => (q.state.data?.some((r) => BUSY.includes(r.status)) ? 2_000 : false),
  });
  const busy = !!repos.data?.some((r) => BUSY.includes(r.status));
  const wasBusy = useRef(busy);
  useEffect(() => {
    // An import or analysis just finished: the context changed with it.
    if (wasBusy.current && !busy) {
      qc.invalidateQueries({ queryKey: keys.project(id, "context") });
      qc.invalidateQueries({ queryKey: keys.projects(ws) });
    }
    wasBusy.current = busy;
  }, [busy, qc, id, ws]);
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: keys.project(id, "repositories") });
    qc.invalidateQueries({ queryKey: keys.project(id, "context") });
  };
  const analyze = useMutation({
    mutationFn: (repo_id: string) => unwrap(api.POST("/api/v1/projects/{id}/repositories/{repo_id}/analyze", { params: { path: { id, repo_id } } })),
    onSuccess: invalidate,
  });
  const del = useMutation({
    mutationFn: (repo_id: string) => unwrap(api.DELETE("/api/v1/projects/{id}/repositories/{repo_id}", { params: { path: { id, repo_id } } })),
    onSuccess: () => {
      setDeleting(null);
      invalidate();
    },
  });

  return (
    <div className="flex flex-col gap-10">
      <Section
        title="Repositories"
        description="Source the analyser reads to learn routes, modules and existing tests. Files are read, never executed."
        actions={
          canWrite ? (
            <Button variant="primary" onClick={() => setAdding(true)}>
              Add repository
            </Button>
          ) : null
        }
      >
        <QueryState
          query={repos}
          empty={
            <Empty title="No repository connected." action={canWrite ? <Button onClick={() => setAdding(true)}>Connect a repository</Button> : undefined}>
              Connect a git URL, a GitHub or GitLab repository, a server directory, or upload a ZIP so planning and impact analysis can use the code.
            </Empty>
          }
        >
          {(list) => (
            <Table>
              <thead>
                <tr>
                  <Th>Source</Th>
                  <Th>Status</Th>
                  <Th>Commit</Th>
                  <Th numeric>Files</Th>
                  <Th numeric>Size</Th>
                  <Th>Last analysed</Th>
                  <Th>
                    <span className="visually-hidden">Actions</span>
                  </Th>
                </tr>
              </thead>
              <tbody>
                {list.map((r) => (
                  <Tr key={r.id}>
                    <Td label="Source">
                      <div className="flex items-center gap-2">
                        <Tag>{r.source}</Tag>
                        <span className="mono min-w-0 truncate text-xs" title={r.url ?? undefined}>
                          {r.url ?? "–"}
                        </span>
                      </div>
                      {r.default_branch ? <div className="mono mt-1 text-xs text-muted">branch {r.default_branch}</div> : null}
                      {r.error ? <code className="mt-1 block text-xs break-words text-error">{r.error}</code> : null}
                    </Td>
                    <Td label="Status">
                      <StatusPill status={r.status} />
                    </Td>
                    <Td label="Commit" mono className="text-xs">
                      {r.last_commit ? r.last_commit.slice(0, 10) : "–"}
                    </Td>
                    <Td label="Files" numeric mono>
                      {r.file_count ?? "–"}
                    </Td>
                    <Td label="Size" numeric mono>
                      {bytes(r.size_bytes)}
                    </Td>
                    <Td label="Last analysed" className="text-ink-2">
                      <span title={dateTime(r.last_analyzed_at)}>{r.last_analyzed_at ? relative(r.last_analyzed_at) : "not yet"}</span>
                    </Td>
                    <Td label="Actions" className="text-right whitespace-nowrap">
                      {canWrite ? (
                        <>
                          <Button variant="ghost" size="sm" disabled={BUSY.includes(r.status)} loading={analyze.isPending && analyze.variables === r.id} onClick={() => analyze.mutate(r.id)}>
                            Re-analyse
                          </Button>
                          <Button variant="danger" size="sm" onClick={() => setDeleting(r)}>
                            Delete
                          </Button>
                        </>
                      ) : null}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          )}
        </QueryState>
        <InlineError error={analyze.error} />
        {busy ? (
          <p className="text-sm text-ink-2" aria-live="polite">
            Importing and analysing. This page refreshes on its own.
          </p>
        ) : null}
      </Section>

      <ContextSection projectId={id} busy={busy} />

      {adding ? <AddRepositoryDialog projectId={id} onClose={() => setAdding(false)} onAdded={invalidate} /> : null}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            del.reset();
          }
        }}
        title="Delete repository"
        body={
          <div className="flex flex-col gap-2">
            <p>
              The imported copy of <span className="mono">{deleting?.url ?? deleting?.source}</span> is removed from this project. Suites and scenarios stay. This cannot be undone.
            </p>
            <InlineError error={del.error} />
          </div>
        }
        confirmLabel="Delete repository"
        loading={del.isPending}
        onConfirm={() => deleting && del.mutate(deleting.id)}
      />
    </div>
  );
}

const SOURCES = [
  { id: "git", label: "Git URL" },
  { id: "github", label: "GitHub" },
  { id: "gitlab", label: "GitLab" },
  { id: "local", label: "Server directory" },
  { id: "zip", label: "ZIP upload" },
];

function AddRepositoryDialog({ projectId, onClose, onAdded }: { projectId: string; onClose: () => void; onAdded: () => void }) {
  const id = projectId;
  const ws = useWsId();
  const [source, setSource] = useState("git");
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [repo, setRepo] = useState("");
  const [connection, setConnection] = useState("");
  const [path, setPath] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const hosted = source === "github" || source === "gitlab";
  const connections = useQuery({
    queryKey: keys.ws(ws, "connections"),
    enabled: !!ws && hosted,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/connections", { params: { path: { ws } } })),
  });
  const matching = (connections.data ?? []).filter((c) => c.kind === source);
  const add = useMutation({
    mutationFn: async (): Promise<Repository> => {
      if (source === "zip") {
        if (!file) throw new Error("Choose a ZIP archive.");
        return uploadRaw<Repository>(`/api/v1/projects/${id}/repositories/upload?name=${encodeURIComponent(file.name)}`, file);
      }
      const body: Schemas["AddRepository"] =
        source === "git"
          ? { source, url: url.trim(), branch: branch.trim() || null }
          : source === "local"
            ? { source, path: path.trim() }
            : { source, repo: repo.trim(), branch: branch.trim() || null, connection_id: connection || null };
      return unwrap(api.POST("/api/v1/projects/{id}/repositories", { params: { path: { id } }, body }));
    },
    onSuccess: () => {
      onAdded();
      onClose();
    },
  });
  const ready = source === "git" ? /^https:\/\//.test(url.trim()) : source === "local" ? !!path.trim() : source === "zip" ? !!file : /^[^/\s]+\/[^\s]+$/.test(repo.trim());

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title="Add repository"
      description="The import runs in the background; analysis starts when it finishes."
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form="add-repo" loading={add.isPending} disabled={!ready}>
            {source === "zip" ? "Upload and analyse" : "Import and analyse"}
          </Button>
        </>
      }
    >
      <form
        id="add-repo"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (ready) add.mutate();
        }}
      >
        <fieldset className="mb-4 flex flex-col gap-2">
          <legend className="mb-1 text-sm font-medium text-ink-2">Source</legend>
          <div className="flex flex-wrap gap-2">
            {SOURCES.map((s) => (
              <button key={s.id} type="button" className="btn btn-sm" aria-pressed={source === s.id} onClick={() => setSource(s.id)}>
                {s.label}
              </button>
            ))}
          </div>
        </fieldset>
        {source === "git" ? (
          <>
            <Field label="Clone URL" hint="Any https URL. Public, or include access in a connection instead.">
              {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" type="url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com/team/app.git" required />}
            </Field>
            <Field label="Branch" hint="Optional. Defaults to the repository's default branch.">
              {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" value={branch} onChange={(e) => setBranch(e.target.value)} placeholder="main" />}
            </Field>
          </>
        ) : null}
        {hosted ? (
          <>
            <Field label="Repository" hint="owner/name on the connection's host.">
              {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="owner/name" required />}
            </Field>
            <Field
              label="Connection"
              error={connections.error ? "Connections could not be loaded." : null}
              hint={!connections.isPending && matching.length === 0 ? `No ${source === "github" ? "GitHub" : "GitLab"} connection in this workspace; public repositories only. Add one under Integrations.` : "Optional. Supplies the access token for private repositories."}
            >
              {(fid, d) => (
                <Select id={fid} aria-describedby={d} value={connection} onChange={(e) => setConnection(e.target.value)} disabled={connections.isPending}>
                  <option value="">None (public)</option>
                  {matching.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} ({c.base_url})
                    </option>
                  ))}
                </Select>
              )}
            </Field>
            <Field label="Branch" hint="Optional.">
              {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" value={branch} onChange={(e) => setBranch(e.target.value)} placeholder="main" />}
            </Field>
          </>
        ) : null}
        {source === "local" ? (
          <Field label="Directory" hint="A path inside the server's ORYN_LOCAL_IMPORT_ROOT. Local import is off unless the server sets it.">
            {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" value={path} onChange={(e) => setPath(e.target.value)} placeholder="northwind" required />}
          </Field>
        ) : null}
        {source === "zip" ? (
          <Field label="ZIP archive" hint={file ? `${file.name}, ${bytes(file.size)}` : "Checked before extraction: no path traversal, absolute paths or symlinks."}>
            {(fid, d) => <input id={fid} aria-describedby={d} type="file" accept=".zip,application/zip" className="field py-1.5" onChange={(e) => setFile(e.target.files?.[0] ?? null)} required />}
          </Field>
        ) : null}
        <InlineError error={add.error} />
      </form>
    </Dialog>
  );
}

function ContextSection({ projectId, busy }: { projectId: string; busy: boolean }) {
  const id = projectId;
  const ctx = useQuery({
    queryKey: keys.project(id, "context"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/context", { params: { path: { id } } })),
    refetchInterval: busy ? 3_000 : false,
  });

  return (
    <Section title="Application context" description="What the latest analysis found. Planning and regression selection read from this.">
      <QueryState
        query={ctx}
        skeleton={<Skeleton className="h-48" />}
        isEmpty={(v) => !v.context}
        empty={<Empty title="Not analysed yet.">Connect a repository; its analysis appears here when the import finishes.</Empty>}
      >
        {(v) => <ContextBody view={v} />}
      </QueryState>
      <ContextSearch projectId={id} />
    </Section>
  );
}

function ContextBody({ view }: { view: Schemas["ContextView"] }) {
  const c = view.context!;
  const a = c.analysis as unknown as Analysis;
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const routeKind = sp.get("routes") ?? "";
  const setRouteKind = (v: string) => {
    const q = new URLSearchParams(sp.toString());
    if (v) q.set("routes", v);
    else q.delete("routes");
    router.replace(`${path}?${q.toString()}`, { scroll: false });
  };
  const languages = Object.entries(a.languages ?? {}).sort((x, y) => y[1].lines - x[1].lines);
  const routes = (a.routes ?? []).filter((r) => !routeKind || r.kind === routeKind);
  const deps = a.dependencies ?? [];
  const changes = a.changes ?? { added: [], modified: [], removed: [] };
  const modules = view.modules.length ? view.modules : (a.modules ?? []);

  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-col gap-3 border-y border-rule py-4">
        <p className="max-w-[80ch]">{c.summary}</p>
        <Meta
          items={[
            { label: "Analysed", value: <span className="mono">{dateTime(c.analyzed_at)}</span> },
            { label: "Version", value: <span className="mono">{c.version}</span> },
            { label: "Commit", value: <span className="mono">{c.commit_sha ? c.commit_sha.slice(0, 12) : "–"}</span> },
            { label: "Files", value: <span className="mono">{a.file_count ?? "–"}{a.truncated ? " (truncated)" : ""}, {bytes(a.total_bytes)}</span> },
            { label: "Frameworks", value: a.frameworks?.length ? a.frameworks.join(", ") : <span className="text-muted">None detected</span> },
          ]}
        />
      </div>

      <div className="grid grid-cols-1 gap-8 lg:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-2">
          <h3>Languages</h3>
          {languages.length === 0 ? (
            <p className="text-muted">None detected.</p>
          ) : (
            <Table>
              <thead>
                <tr>
                  <Th>Language</Th>
                  <Th numeric>Files</Th>
                  <Th numeric>Lines</Th>
                </tr>
              </thead>
              <tbody>
                {languages.map(([name, s]) => (
                  <Tr key={name}>
                    <Td label="Language">{name}</Td>
                    <Td label="Files" numeric mono>
                      {s.files}
                    </Td>
                    <Td label="Lines" numeric mono>
                      {s.lines.toLocaleString()}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          <h3>Modules</h3>
          {modules.length === 0 ? (
            <p className="text-muted">None detected.</p>
          ) : (
            <Table>
              <thead>
                <tr>
                  <Th>Module</Th>
                  <Th>Kind</Th>
                  <Th numeric>Files</Th>
                </tr>
              </thead>
              <tbody>
                {modules.map((m) => (
                  <Tr key={m.path_prefix}>
                    <Td label="Module" mono className="text-xs">
                      {m.path_prefix}
                    </Td>
                    <Td label="Kind">
                      <Tag>{m.kind}</Tag>
                    </Td>
                    <Td label="Files" numeric mono>
                      {m.file_count}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <h3>
            Detected routes <span className="mono text-xs font-normal text-muted">{(a.routes ?? []).length}</span>
          </h3>
          <label className="flex items-center gap-2 text-sm text-ink-2">
            Kind
            <select className="field h-9 w-auto" value={routeKind} onChange={(e) => setRouteKind(e.target.value)}>
              <option value="">Pages and API</option>
              <option value="page">Pages</option>
              <option value="api">API</option>
            </select>
          </label>
        </div>
        {routes.length === 0 ? (
          <p className="text-muted">No routes detected.</p>
        ) : (
          <div className="max-h-[28rem] overflow-y-auto">
            <Table>
              <thead>
                <tr>
                  <Th>Method</Th>
                  <Th>Path</Th>
                  <Th>Kind</Th>
                  <Th>File</Th>
                </tr>
              </thead>
              <tbody>
                {routes.map((r, i) => (
                  <Tr key={`${r.method}-${r.path}-${r.file}-${i}`}>
                    <Td label="Method" mono className="text-xs">
                      {r.method}
                    </Td>
                    <Td label="Path" mono className="text-xs break-all">
                      {r.path}
                    </Td>
                    <Td label="Kind">
                      <Tag>{r.kind}</Tag>
                    </Td>
                    <Td label="File" mono className="text-xs text-ink-2">
                      {r.file}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          </div>
        )}
      </div>

      <div className="grid grid-cols-1 gap-8 lg:grid-cols-3">
        <div className="flex min-w-0 flex-col gap-2">
          <h3>OpenAPI</h3>
          {(a.openapi ?? []).length === 0 ? (
            <p className="text-muted">No OpenAPI document found.</p>
          ) : (
            <ul className="flex flex-col gap-2">
              {(a.openapi ?? []).map((o) => (
                <li key={o.file} className="flex flex-col gap-0.5">
                  <span className="font-medium">
                    {o.title ?? o.file}
                    {o.version ? <span className="mono ml-2 text-xs text-muted">v{o.version}</span> : null}
                  </span>
                  <span className="mono text-xs text-ink-2">
                    {o.file}, {o.endpoints.length} endpoints
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          <h3>Feature files</h3>
          <MonoList items={a.feature_files ?? []} />
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          <h3>Test files</h3>
          <MonoList items={a.test_files ?? []} />
          {a.docs?.length ? (
            <>
              <h3 className="mt-3">Documents</h3>
              <MonoList items={a.docs} />
            </>
          ) : null}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <h3>
          Dependencies <span className="mono text-xs font-normal text-muted">{deps.length}</span>
        </h3>
        {deps.length === 0 ? (
          <p className="text-muted">No dependency manifests found.</p>
        ) : (
          <div className="max-h-80 overflow-y-auto">
            <Table>
              <thead>
                <tr>
                  <Th>Package</Th>
                  <Th>Version</Th>
                  <Th>Ecosystem</Th>
                  <Th>Scope</Th>
                </tr>
              </thead>
              <tbody>
                {deps.map((d) => (
                  <Tr key={`${d.ecosystem}-${d.name}-${d.dev}`}>
                    <Td label="Package" mono className="text-xs">
                      {d.name}
                    </Td>
                    <Td label="Version" mono className="text-xs text-ink-2">
                      {d.version ?? "–"}
                    </Td>
                    <Td label="Ecosystem">{d.ecosystem}</Td>
                    <Td label="Scope" className="text-ink-2">
                      {d.dev ? "development" : "runtime"}
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          </div>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <h3>Last change set</h3>
        <p className="text-sm text-ink-2">
          <span className="mono">{c.files_changed_last_run}</span> files changed since the previous analysis.
        </p>
        {changes.added.length + changes.modified.length + changes.removed.length === 0 ? null : (
          <div className="grid grid-cols-1 gap-6 md:grid-cols-3">
            <div className="flex min-w-0 flex-col gap-1">
              <span className="text-xs text-muted">Added {changes.added.length}</span>
              <MonoList items={changes.added} empty="None" />
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <span className="text-xs text-muted">Modified {changes.modified.length}</span>
              <MonoList items={changes.modified} empty="None" />
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <span className="text-xs text-muted">Removed {changes.removed.length}</span>
              <MonoList items={changes.removed} empty="None" />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function ContextSearch({ projectId }: { projectId: string }) {
  const id = projectId;
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const q = sp.get("cq") ?? "";
  const [draft, setDraft] = useState(q);
  const results = useQuery({
    queryKey: keys.project(id, "context-search", q),
    enabled: !!q,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/context/search", { params: { path: { id }, query: { q, limit: 20 } } })),
  });
  const submit = (v: string) => {
    const p = new URLSearchParams(sp.toString());
    if (v.trim()) p.set("cq", v.trim());
    else p.delete("cq");
    router.replace(`${path}?${p.toString()}`, { scroll: false });
  };

  return (
    <div className="mt-6 flex flex-col gap-3">
      <h3>Search the code</h3>
      <form
        role="search"
        className="flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          submit(draft);
        }}
      >
        <Field label="Full-text search over analysed files" className="min-w-0 flex-1" hint="Secrets are redacted before indexing.">
          {(fid, d) => <Input id={fid} aria-describedby={d} type="search" value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="checkout total" />}
        </Field>
        <Button type="submit" className="mb-[calc(1lh+4px)]">
          <Search aria-hidden className="size-4" /> Search
        </Button>
      </form>
      {!q ? null : results.isPending ? (
        <Skeleton className="h-24" />
      ) : results.error ? (
        <ErrorState error={results.error} retry={() => results.refetch()} />
      ) : (results.data ?? []).length === 0 ? (
        <Empty title={`Nothing matches “${q}”.`}>Try a route, a label on the page, or a function name.</Empty>
      ) : (
        <ul className="flex flex-col gap-3">
          {results.data!.map((r, i) => (
            <li key={`${r.source_path}-${i}`} className="flex flex-col gap-1 border-b border-rule pb-3">
              <span className="mono text-xs font-medium">{r.source_path}</span>
              <pre className="max-h-48 overflow-auto rounded-[var(--radius-input)] bg-paper-2 p-3 text-xs whitespace-pre-wrap">{r.content}</pre>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
