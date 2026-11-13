"use client";
import { useQuery } from "@tanstack/react-query";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { cn } from "@/lib/cn";

export type ProjectDetail = Schemas["ProjectDetail"];

/** Plastic identity tints (design.md § Colour tokens). Identity only, never state. */
export const TINTS: { id: string; label: string; cls: string }[] = [
  { id: "mon", label: "Rose", cls: "bg-p-mon" },
  { id: "tue", label: "Apricot", cls: "bg-p-tue" },
  { id: "wed", label: "Lemon", cls: "bg-p-wed" },
  { id: "thu", label: "Mint", cls: "bg-p-thu" },
  { id: "fri", label: "Sky", cls: "bg-p-fri" },
  { id: "sat", label: "Lavender", cls: "bg-p-sat" },
  { id: "sun", label: "Orchid", cls: "bg-p-sun" },
];

export function tintClass(color: string | null | undefined) {
  return TINTS.find((t) => t.id === color)?.cls ?? "bg-p-mon";
}

/** The project's identity chip: a plastic tint carrying the first letter. */
export function ProjectChip({ name, color, size = "md" }: { name: string; color: string; size?: "md" | "lg" }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-[6px] border border-rule font-display font-bold text-ink shadow-[inset_0_1px_0_var(--glint)]",
        tintClass(color),
        size === "lg" ? "size-9 text-base" : "size-6 text-xs",
      )}
    >
      {name.trim().charAt(0).toUpperCase() || "P"}
    </span>
  );
}

export function useProjectDetail(id: string) {
  return useQuery({
    queryKey: keys.project(id),
    enabled: !!id,
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}", { params: { path: { id } } })),
  });
}

/** UI hint from the caller's effective project role. The API enforces every permission. */
export function can(project: ProjectDetail | undefined, permission: string) {
  return !!project?.permissions.includes(permission);
}

/** The server's message, shown next to the action that failed. */
export function InlineError({ error, className }: { error: unknown; className?: string }) {
  if (!error) return null;
  return (
    <p role="alert" className={cn("text-sm text-error", className)}>
      <code className="break-words">{errorMessage(error)}</code>
    </p>
  );
}

/** A short list of strings as mono tags or a muted dash. */
export function MonoList({ items, empty = "None detected" }: { items: string[]; empty?: string }) {
  if (items.length === 0) return <span className="text-muted">{empty}</span>;
  return (
    <ul className="flex flex-col gap-1">
      {items.map((i) => (
        <li key={i} className="mono text-xs break-all text-ink-2">
          {i}
        </li>
      ))}
    </ul>
  );
}

/** Editable key/value rows (environment variables). */
export function KeyValueRows({
  rows,
  onChange,
  disabled,
  keyLabel = "Name",
  valueLabel = "Value",
}: {
  rows: { k: string; v: string }[];
  onChange: (rows: { k: string; v: string }[]) => void;
  disabled?: boolean;
  keyLabel?: string;
  valueLabel?: string;
}) {
  return (
    <div className="flex flex-col gap-2">
      {rows.map((r, i) => (
        <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto] items-center gap-2">
          <input
            className="field mono"
            aria-label={`${keyLabel} ${i + 1}`}
            placeholder={keyLabel.toLowerCase()}
            value={r.k}
            disabled={disabled}
            onChange={(e) => onChange(rows.map((x, j) => (j === i ? { ...x, k: e.target.value } : x)))}
          />
          <input
            className="field mono"
            aria-label={`${valueLabel} ${i + 1}`}
            placeholder={valueLabel.toLowerCase()}
            value={r.v}
            disabled={disabled}
            onChange={(e) => onChange(rows.map((x, j) => (j === i ? { ...x, v: e.target.value } : x)))}
          />
          <button type="button" className="btn btn-ghost btn-sm" disabled={disabled} onClick={() => onChange(rows.filter((_, j) => j !== i))}>
            Remove
          </button>
        </div>
      ))}
      <div>
        <button type="button" className="btn btn-sm" disabled={disabled} onClick={() => onChange([...rows, { k: "", v: "" }])}>
          Add {keyLabel.toLowerCase()}
        </button>
      </div>
    </div>
  );
}
