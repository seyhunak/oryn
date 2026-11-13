import { cn } from "@/lib/cn";

/**
 * The signature object (design.md § Signature): a pharmacy-style label for a run or step.
 * `verified` is only ever true when the Rust engine validated every assertion; the
 * stamp is never derived from AI output.
 */
export function EvidenceLabel({
  title,
  lines,
  verified,
  verifiedAt,
  by,
  failure,
  animate,
}: {
  title: string;
  lines: [React.ReactNode, React.ReactNode][];
  verified: boolean | null;
  verifiedAt?: string;
  by?: string | null;
  failure?: string | null;
  animate?: boolean;
}) {
  return (
    <figure className="m-0 flex flex-col gap-2 rounded-[var(--radius-input)] border border-rule bg-white p-4 font-mono text-sm" aria-label="Evidence label">
      <p className="font-medium text-ink">{title}</p>
      {lines.map(([a, b], i) => (
        <div key={i} className="flex justify-between gap-4 text-ink-2">
          <span className="min-w-0 truncate">{a}</span>
          <span className="shrink-0 tabular-nums">{b}</span>
        </div>
      ))}
      <div aria-hidden className="my-1 border-t border-dashed border-rule" />
      {verified === null ? (
        <p className="text-muted">Awaiting evidence</p>
      ) : verified ? (
        <div className={cn("flex flex-col items-start gap-1", animate && "stamp-land")} style={{ transform: "rotate(-2deg)" }}>
          <span className="inline-flex items-center rounded-full border-2 border-accent-deep p-0.5">
            <span className="rounded-full border border-accent-deep px-3 py-1 font-medium text-accent-deep">Verified{verifiedAt ? ` ${verifiedAt}` : ""}</span>
          </span>
          {by ? <span className="pl-3 text-xs text-ink-2">by worker {by}</span> : null}
        </div>
      ) : (
        <div className="flex flex-col items-start gap-1">
          <span className="rounded-full border-2 border-dashed border-muted px-3 py-1 text-muted">Not verified</span>
          {failure ? <span className="text-xs break-words text-error">{failure}</span> : null}
        </div>
      )}
    </figure>
  );
}
