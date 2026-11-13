import { Check, Clock, LoaderCircle, Minus, Waves, X } from "lucide-react";
import { cn } from "@/lib/cn";

type Tone = "pass" | "fail" | "run" | "wait" | "off" | "flaky";

const TONES: Record<string, Tone> = {
  passed: "pass", verified: "pass", online: "pass", ready: "pass", approved: "pass", accepted: "pass", delivered: "pass", success: "pass", covered: "pass", healthy: "pass",
  failed: "fail", error: "fail", timed_out: "fail", rejected: "fail", unhealthy: "fail",
  running: "run", claimed: "run", analyzing: "run", importing: "run", recording: "run", busy: "run",
  queued: "wait", pending: "wait", draft: "wait", proposed: "wait", uncovered: "wait", not_run: "wait", review: "wait",
  skipped: "off", cancelled: "off", offline: "off", revoked: "off", draining: "off", disabled: "off", archived: "off", deprecated: "off", stopped: "off",
  flaky: "flaky",
};

const LABELS: Record<string, string> = { timed_out: "timed out", not_run: "not run" };

/** A status is a pill with a glyph and a word, never colour alone (design.md § Status indicators). */
export function StatusPill({ status, label, className }: { status: string; label?: string; className?: string }) {
  const tone = TONES[status] ?? "wait";
  const Icon = { pass: Check, fail: X, run: LoaderCircle, wait: Clock, off: Minus, flaky: Waves }[tone];
  return (
    <span
      className={cn(
        "inline-flex h-6 shrink-0 items-center gap-1 rounded-full px-2 text-xs font-medium whitespace-nowrap",
        tone === "pass" && "bg-accent-soft text-accent-deep",
        tone === "fail" && "bg-error-soft text-error",
        (tone === "run" || tone === "flaky") && "bg-paper-3 text-ink",
        tone === "wait" && "border border-rule text-muted",
        tone === "off" && "text-muted",
        className,
      )}
    >
      <Icon aria-hidden className={cn("size-3.5", tone === "run" && "spin")} strokeWidth={2.25} />
      {label ?? LABELS[status] ?? status.replace(/_/g, " ")}
    </span>
  );
}

export function Tag({ children, className }: { children: React.ReactNode; className?: string }) {
  return <span className={cn("inline-flex h-6 items-center rounded-full border border-rule px-2 text-xs text-ink-2", className)}>{children}</span>;
}

/** AI-sourced content is always labelled, with its confidence as a figure. */
export function AiTag({ confidence }: { confidence?: number | null }) {
  return (
    <span className="inline-flex h-6 items-center gap-1 rounded-full bg-p-sat px-2 text-xs font-medium text-ink">
      AI interpretation{confidence != null ? <span className="mono">{Math.round(confidence * 100)}%</span> : null}
    </span>
  );
}
