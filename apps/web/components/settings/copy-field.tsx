"use client";
import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";

/** A read-only mono value with a copy button. Used for one-time secrets and commands. */
export function CopyField({ label, value, multiline }: { label: string; value: string; multiline?: boolean }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-sm font-medium text-ink-2">{label}</span>
      <div className="flex items-start gap-2">
        {multiline ? (
          <pre className="m-0 min-w-0 flex-1 overflow-x-auto rounded-[var(--radius-input)] border border-rule bg-paper-2 p-3 font-mono text-xs whitespace-pre">{value}</pre>
        ) : (
          <code className="min-w-0 flex-1 rounded-[var(--radius-input)] border border-rule bg-paper-2 px-3 py-2 font-mono text-sm break-all">{value}</code>
        )}
        <Button size="sm" onClick={copy} aria-label={`Copy ${label.toLowerCase()}`}>
          {copied ? <Check aria-hidden className="size-3.5" /> : <Copy aria-hidden className="size-3.5" />}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <span aria-live="polite" className="visually-hidden">
        {copied ? `${label} copied` : ""}
      </span>
    </div>
  );
}

/** The warning that sits next to any secret shown once. */
export function ShownOnce({ what }: { what: string }) {
  return (
    <p role="note" className="rounded-[var(--radius-input)] border border-rule bg-paper-2 p-3 text-sm text-ink">
      Copy the {what} now. You will not see it again: ORYN keeps only a hash. If you lose it, revoke it and create a new one.
    </p>
  );
}

/** Inline error near the action that failed. */
export function InlineError({ error }: { error: unknown }) {
  if (!error) return null;
  const msg = error instanceof Error ? error.message : String(error);
  return (
    <p role="alert" className="text-sm text-error">
      {msg}
    </p>
  );
}
