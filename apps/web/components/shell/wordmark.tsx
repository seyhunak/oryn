export function Wordmark({ inverse }: { inverse?: boolean }) {
  return (
    <span className={`inline-flex items-center gap-2 font-display text-lg font-bold tracking-tight ${inverse ? "text-paper" : "text-ink"}`}>
      <svg aria-hidden viewBox="0 0 24 24" className="size-6">
        <circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" strokeWidth="2.25" />
        <circle cx="12" cy="12" r="3" fill={inverse ? "var(--color-accent-soft)" : "var(--color-accent)"} />
      </svg>
      ORYN
    </span>
  );
}
