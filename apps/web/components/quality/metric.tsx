export type MetricAnchor = "pass-rate" | "failure-rate" | "flakiness" | "coverage" | "mean-duration";

export const METRICS_DOC = "https://github.com/oryn-dev/oryn/blob/main/docs/METRICS.md";

/** Every figure links to its documented calculation (design.md § Data visualisation). */
export function MetricLink({ anchor, className }: { anchor: MetricAnchor; className?: string }) {
  return (
    <a href={`${METRICS_DOC}#${anchor}`} target="_blank" rel="noreferrer" className={className ?? "text-xs text-ink-2 underline underline-offset-2 hover:text-ink"}>
      How this is calculated
    </a>
  );
}

/** A figure, its label, a hint and the link to how it is computed. No icon tile, no trend arrow. */
export function MetricFigure({ label, value, hint, anchor }: { label: string; value: React.ReactNode; hint?: React.ReactNode; anchor?: MetricAnchor }) {
  return (
    <div className="flex flex-col gap-2 py-1">
      <span className="text-xs text-muted">{label}</span>
      <span className="font-display text-[1.6rem] leading-none font-bold tracking-tight tabular-nums">{value}</span>
      {hint ? <span className="text-xs text-ink-2">{hint}</span> : null}
      {anchor ? <MetricLink anchor={anchor} /> : null}
    </div>
  );
}
