"use client";
import { Fragment, useState } from "react";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { Section } from "@/components/ui/page";
import { Empty, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Dialog } from "@/components/ui/dialog";
import { Field, Input, Select, Textarea } from "@/components/ui/field";
import { can, InlineError, type ProjectDetail } from "./common";

type Requirement = Schemas["Requirement"];
type Kind = Schemas["RequirementKind"];

export const KINDS: { id: Kind; label: string }[] = [
  { id: "requirement", label: "Requirement" },
  { id: "user_story", label: "User story" },
  { id: "acceptance_criteria", label: "Acceptance criteria" },
  { id: "document", label: "Document" },
  { id: "api_spec", label: "API spec" },
];
const PRIORITIES = ["low", "medium", "high", "critical"];
const kindLabel = (k: string) => KINDS.find((x) => x.id === k)?.label ?? k.replace(/_/g, " ");

export function useRequirementData(id: string) {
  const requirements = useQuery({
    queryKey: keys.project(id, "requirements"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/requirements", { params: { path: { id } } })),
  });
  const trace = useQuery({
    queryKey: keys.project(id, "traceability"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/traceability", { params: { path: { id } } })),
  });
  return { requirements, trace };
}

export function RequirementsTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const qc = useQueryClient();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const open = sp.get("req");
  const { requirements, trace } = useRequirementData(id);
  const canWrite = can(project, "project:write");
  const canLink = can(project, "test:write");
  const [editing, setEditing] = useState<Requirement | "new" | null>(null);
  const [importing, setImporting] = useState(false);
  const [deleting, setDeleting] = useState<Requirement | null>(null);

  const toggle = (rid: string) => {
    const q = new URLSearchParams(sp.toString());
    if (open === rid) q.delete("req");
    else q.set("req", rid);
    router.replace(`${path}?${q.toString()}`, { scroll: false });
  };
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: keys.project(id, "requirements") });
    qc.invalidateQueries({ queryKey: keys.project(id, "traceability") });
    qc.invalidateQueries({ queryKey: keys.project(id, "quality") });
  };
  const del = useMutation({
    mutationFn: (rid: string) => unwrap(api.DELETE("/api/v1/projects/{id}/requirements/{rid}", { params: { path: { id, rid } } })),
    onSuccess: () => {
      setDeleting(null);
      invalidate();
    },
  });

  return (
    <Section
      title="Requirements"
      description="What the application must do. Link scenarios to a requirement to cover it; tags such as @US-1 on a scenario link it automatically."
      actions={
        canWrite ? (
          <>
            <Button onClick={() => setImporting(true)}>Import</Button>
            <Button variant="primary" onClick={() => setEditing("new")}>
              Add requirement
            </Button>
          </>
        ) : null
      }
    >
      <QueryState
        query={requirements}
        empty={
          <Empty title="No requirements yet." action={canWrite ? <Button onClick={() => setImporting(true)}>Import a document</Button> : undefined}>
            Paste a markdown or plain-text requirements document to create one requirement per heading.
          </Empty>
        }
      >
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Key</Th>
                <Th>Requirement</Th>
                <Th>Priority</Th>
                <Th numeric>Linked scenarios</Th>
                <Th>
                  <span className="visually-hidden">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {list.map((r) => {
                const expanded = open === r.id;
                return (
                  <Fragment key={r.id}>
                    <Tr>
                      <Td label="Key" mono className="whitespace-nowrap">
                        <button type="button" className="inline-flex items-center gap-1 font-medium" aria-expanded={expanded} onClick={() => toggle(r.id)}>
                          {expanded ? <ChevronDown aria-hidden className="size-4" /> : <ChevronRight aria-hidden className="size-4" />}
                          {r.key}
                        </button>
                      </Td>
                      <Td label="Requirement">
                        <div className="font-medium">{r.title}</div>
                        <div className="text-xs text-muted">
                          {kindLabel(r.kind)}, source {r.source}
                        </div>
                      </Td>
                      <Td label="Priority">
                        <Tag>{r.priority}</Tag>
                      </Td>
                      <Td label="Linked scenarios" numeric mono>
                        {r.linked_scenarios}
                      </Td>
                      <Td label="Actions" className="whitespace-nowrap text-right">
                        {canWrite ? (
                          <>
                            <Button variant="ghost" size="sm" onClick={() => setEditing(r)}>
                              Edit
                            </Button>
                            <Button variant="danger" size="sm" onClick={() => setDeleting(r)}>
                              Delete
                            </Button>
                          </>
                        ) : null}
                      </Td>
                    </Tr>
                    {expanded ? (
                      <tr>
                        <td colSpan={5} className="border-b border-rule bg-paper-2 px-3 py-4">
                          <RequirementDetail projectId={id} requirement={r} links={trace.data?.links ?? null} linksPending={trace.isPending} canLink={canLink} onChanged={invalidate} />
                        </td>
                      </tr>
                    ) : null}
                  </Fragment>
                );
              })}
            </tbody>
          </Table>
        )}
      </QueryState>

      <RequirementDialog projectId={id} editing={editing} onClose={() => setEditing(null)} onSaved={invalidate} />
      <ImportDialog projectId={id} open={importing} onOpenChange={setImporting} onImported={invalidate} />
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            del.reset();
          }
        }}
        title={`Delete ${deleting?.key ?? "requirement"}`}
        body={
          <div className="flex flex-col gap-2">
            <p>
              {deleting?.title} and its scenario links are removed. The scenarios themselves stay. This cannot be undone.
            </p>
            <InlineError error={del.error} />
          </div>
        }
        confirmLabel="Delete requirement"
        loading={del.isPending}
        onConfirm={() => deleting && del.mutate(deleting.id)}
      />
    </Section>
  );
}

function RequirementDetail({
  projectId,
  requirement,
  links,
  linksPending,
  canLink,
  onChanged,
}: {
  projectId: string;
  requirement: Requirement;
  links: Schemas["TraceLink"][] | null;
  linksPending: boolean;
  canLink: boolean;
  onChanged: () => void;
}) {
  const id = projectId;
  const rid = requirement.id;
  const mine = (links ?? []).filter((l) => l.requirement_id === rid);
  const [pick, setPick] = useState("");
  const scenarios = useQuery({
    queryKey: keys.project(id, "scenarios"),
    enabled: canLink,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/scenarios", { params: { path: { id } } })),
  });
  const statusOf = (sid: string) => scenarios.data?.find((s) => s.id === sid)?.last_status ?? null;
  const unlink = useMutation({
    mutationFn: (scenario_id: string) => unwrap(api.DELETE("/api/v1/projects/{id}/requirements/{rid}/links/{scenario_id}", { params: { path: { id, rid, scenario_id } } })),
    onSuccess: onChanged,
  });
  const link = useMutation({
    mutationFn: (scenario_id: string) => unwrap(api.POST("/api/v1/projects/{id}/requirements/{rid}/links", { params: { path: { id, rid } }, body: { scenario_id } })),
    onSuccess: () => {
      setPick("");
      onChanged();
    },
  });
  const linked = new Set(mine.map((l) => l.scenario_id));
  const candidates = (scenarios.data ?? []).filter((s) => !linked.has(s.id));

  return (
    <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
      <div className="flex min-w-0 flex-col gap-2">
        <h3>Description</h3>
        {requirement.body.trim() ? (
          <div className="max-h-72 overflow-y-auto text-sm leading-relaxed text-ink-2">
            <PlainMarkdown text={requirement.body} />
          </div>
        ) : (
          <p className="text-muted">No description.</p>
        )}
      </div>
      <div className="flex min-w-0 flex-col gap-3">
        <h3>Linked scenarios</h3>
        {linksPending ? (
          <Skeleton className="h-16" />
        ) : mine.length === 0 ? (
          <p className="text-ink-2">No scenario covers this requirement yet.</p>
        ) : (
          <ul className="flex flex-col">
            {mine.map((l) => {
              const st = statusOf(l.scenario_id);
              return (
                <li key={l.scenario_id} className="flex items-center justify-between gap-3 border-b border-rule py-2">
                  <div className="min-w-0">
                    <Link href={`/studio?project=${id}&scenario=${l.scenario_id}`} className="line-clamp-2 no-underline hover:underline">
                      {l.scenario_name}
                    </Link>
                    <span className="text-xs text-muted">linked by {l.source}</span>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {st ? <StatusPill status={st} /> : null}
                    {canLink ? (
                      <Button variant="ghost" size="sm" onClick={() => unlink.mutate(l.scenario_id)} loading={unlink.isPending && unlink.variables === l.scenario_id}>
                        Unlink
                      </Button>
                    ) : null}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        <InlineError error={unlink.error} />
        {canLink ? (
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (pick) link.mutate(pick);
            }}
          >
            <Field label="Link a scenario" className="min-w-0 flex-1">
              {(fid, d) => (
                <Select id={fid} aria-describedby={d} value={pick} onChange={(e) => setPick(e.target.value)} disabled={scenarios.isPending}>
                  <option value="">{scenarios.isPending ? "Loading scenarios" : candidates.length ? "Choose a scenario" : "No other scenarios"}</option>
                  {candidates.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.suite_name}: {s.name}
                    </option>
                  ))}
                </Select>
              )}
            </Field>
            <Button type="submit" disabled={!pick} loading={link.isPending} className="mb-[calc(1lh+4px)]">
              Link
            </Button>
          </form>
        ) : null}
        <InlineError error={link.error ?? scenarios.error} />
      </div>
    </div>
  );
}

function RequirementDialog({ projectId, editing, onClose, onSaved }: { projectId: string; editing: Requirement | "new" | null; onClose: () => void; onSaved: () => void }) {
  return editing ? <RequirementForm key={editing === "new" ? "new" : editing.id} projectId={projectId} editing={editing} onClose={onClose} onSaved={onSaved} /> : null;
}

function RequirementForm({ projectId, editing, onClose, onSaved }: { projectId: string; editing: Requirement | "new"; onClose: () => void; onSaved: () => void }) {
  const id = projectId;
  const isNew = editing === "new";
  const [key, setKey] = useState("");
  const [title, setTitle] = useState(isNew ? "" : editing.title);
  const [kind, setKind] = useState<Kind>(isNew ? "requirement" : (editing.kind as Kind));
  const [priority, setPriority] = useState(isNew ? "medium" : editing.priority);
  const [body, setBody] = useState(isNew ? "" : editing.body);
  const save = useMutation({
    mutationFn: () =>
      isNew
        ? unwrap(api.POST("/api/v1/projects/{id}/requirements", { params: { path: { id } }, body: { key: key.trim() || null, title: title.trim(), kind, priority, body } }))
        : unwrap(api.PATCH("/api/v1/projects/{id}/requirements/{rid}", { params: { path: { id, rid: editing.id } }, body: { title: title.trim(), kind, priority, body } })),
    onSuccess: () => {
      onSaved();
      onClose();
    },
  });

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={isNew ? "Add requirement" : `Edit ${editing.key}`}
      wide
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form="requirement-form" loading={save.isPending} disabled={!title.trim()}>
            {isNew ? "Add requirement" : "Save changes"}
          </Button>
        </>
      }
    >
      <form
        id="requirement-form"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (title.trim()) save.mutate();
        }}
      >
        <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-3">
          {isNew ? (
            <Field label="Key" hint="Optional, like US-12. Defaults to the next REQ-n.">
              {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" value={key} onChange={(e) => setKey(e.target.value)} placeholder="REQ-1" />}
            </Field>
          ) : (
            <Field label="Key" hint="Keys do not change.">
              {(fid, d) => <Input id={fid} aria-describedby={d} className="mono" value={editing.key} disabled readOnly />}
            </Field>
          )}
          <Field label="Kind">
            {(fid, d) => (
              <Select id={fid} aria-describedby={d} value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
                {KINDS.map((k) => (
                  <option key={k.id} value={k.id}>
                    {k.label}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Priority">
            {(fid, d) => (
              <Select id={fid} aria-describedby={d} value={priority} onChange={(e) => setPriority(e.target.value)}>
                {PRIORITIES.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </Select>
            )}
          </Field>
        </div>
        <Field label="Title">{(fid, d) => <Input id={fid} aria-describedby={d} value={title} onChange={(e) => setTitle(e.target.value)} required autoFocus />}</Field>
        <Field label="Description" hint="Markdown. Acceptance criteria as a list work best for planning.">
          {(fid, d) => <Textarea id={fid} aria-describedby={d} rows={10} className="mono text-xs" value={body} onChange={(e) => setBody(e.target.value)} />}
        </Field>
        <InlineError error={save.error} />
      </form>
    </Dialog>
  );
}

function ImportDialog({ projectId, open, onOpenChange, onImported }: { projectId: string; open: boolean; onOpenChange: (o: boolean) => void; onImported: () => void }) {
  const id = projectId;
  const [content, setContent] = useState("");
  const [source, setSource] = useState("");
  const [kind, setKind] = useState<Kind>("user_story");
  const run = useMutation({
    mutationFn: () => unwrap(api.POST("/api/v1/projects/{id}/requirements/import", { params: { path: { id } }, body: { content, kind, source: source.trim() || null } })),
    onSuccess: onImported,
  });
  const close = (o: boolean) => {
    if (!o) {
      setContent("");
      setSource("");
      setKind("user_story");
      run.reset();
    }
    onOpenChange(o);
  };
  const result = run.data;

  return (
    <Dialog
      open={open}
      onOpenChange={close}
      wide
      title="Import requirements"
      description="Paste markdown or plain text. Each ## heading (or # when there are none) becomes a requirement; a leading key such as US-8 in the heading becomes its key. Existing keys are skipped."
      footer={
        result ? (
          <Button variant="primary" onClick={() => close(false)}>
            Done
          </Button>
        ) : (
          <>
            <Button onClick={() => close(false)}>Cancel</Button>
            <Button variant="primary" type="submit" form="import-form" loading={run.isPending} disabled={!content.trim()}>
              Import
            </Button>
          </>
        )
      }
    >
      {result ? (
        <div className="flex flex-col gap-3" aria-live="polite">
          <p>
            Created <span className="mono">{result.created.length}</span>, skipped <span className="mono">{result.skipped.length}</span>.
          </p>
          {result.created.length ? (
            <ul className="flex flex-col">
              {result.created.map((r) => (
                <li key={r.id} className="flex gap-3 border-b border-rule py-1.5 text-sm">
                  <span className="mono w-20 shrink-0">{r.key}</span>
                  <span className="min-w-0 truncate">{r.title}</span>
                </li>
              ))}
            </ul>
          ) : null}
          {result.skipped.length ? (
            <div className="flex flex-col gap-1">
              <h3>Skipped</h3>
              <ul className="flex flex-col gap-1">
                {result.skipped.map((s, i) => (
                  <li key={i} className="mono text-xs text-ink-2">
                    {s}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>
      ) : (
        <form
          id="import-form"
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (content.trim()) run.mutate();
          }}
        >
          <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
            <Field label="Source document" hint="Optional. Recorded on each requirement.">
              {(fid, d) => <Input id={fid} aria-describedby={d} value={source} onChange={(e) => setSource(e.target.value)} placeholder="requirements.md" />}
            </Field>
            <Field label="Kind">
              {(fid, d) => (
                <Select id={fid} aria-describedby={d} value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
                  {KINDS.map((k) => (
                    <option key={k.id} value={k.id}>
                      {k.label}
                    </option>
                  ))}
                </Select>
              )}
            </Field>
          </div>
          <Field label="Document">
            {(fid, d) => (
              <Textarea
                id={fid}
                aria-describedby={d}
                rows={14}
                className="mono text-xs"
                value={content}
                onChange={(e) => setContent(e.target.value)}
                placeholder={"## US-9 Wishlist\nAs a customer I want to save items for later.\n\n## US-10 Order history\n..."}
                required
              />
            )}
          </Field>
          <InlineError error={run.error} />
        </form>
      )}
    </Dialog>
  );
}

const ENTITIES: Record<string, string> = { "&lt;": "<", "&gt;": ">", "&amp;": "&", "&quot;": '"', "&#39;": "'" };
const decode = (s: string) => s.replace(/&(lt|gt|amp|quot|#39);/g, (m) => ENTITIES[m] ?? m);

/** Inline markdown subset for requirement bodies: paragraphs, "- " lists, **bold** and `code`. Text only, no HTML. */
function Inline({ text }: { text: string }) {
  const parts = decode(text).split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("**") && p.endsWith("**") && p.length > 4 ? (
          <strong key={i} className="font-medium text-ink">
            {p.slice(2, -2)}
          </strong>
        ) : p.startsWith("`") && p.endsWith("`") && p.length > 2 ? (
          <code key={i}>{p.slice(1, -1)}</code>
        ) : (
          <Fragment key={i}>{p}</Fragment>
        ),
      )}
    </>
  );
}

function PlainMarkdown({ text }: { text: string }) {
  const blocks: { list: boolean; lines: string[] }[] = [];
  for (const raw of text.split("\n")) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      blocks.push({ list: false, lines: [] });
      continue;
    }
    const item = /^\s*[-*]\s+/.test(line);
    const last = blocks[blocks.length - 1];
    if (last && last.lines.length && last.list === item) last.lines.push(item ? line.replace(/^\s*[-*]\s+/, "") : line);
    else blocks.push({ list: item, lines: [item ? line.replace(/^\s*[-*]\s+/, "") : line] });
  }
  return (
    <div className="flex flex-col gap-2">
      {blocks
        .filter((b) => b.lines.length)
        .map((b, i) =>
          b.list ? (
            <ul key={i} className="flex list-disc flex-col gap-1 pl-5">
              {b.lines.map((l, j) => (
                <li key={j}>
                  <Inline text={l} />
                </li>
              ))}
            </ul>
          ) : (
            <p key={i} className="whitespace-pre-wrap">
              <Inline text={b.lines.join("\n")} />
            </p>
          ),
        )}
    </div>
  );
}
