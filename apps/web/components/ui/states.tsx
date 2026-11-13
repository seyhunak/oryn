import { RotateCw } from "lucide-react";
import { errorMessage } from "@/lib/api/client";
import { Button } from "./button";
import { cn } from "@/lib/cn";

export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden className={cn("skeleton h-4", className)} />;
}

export function TableSkeleton({ rows = 5, cols = 4 }: { rows?: number; cols?: number }) {
  return (
    <div role="status" aria-label="Loading" className="flex flex-col">
      {Array.from({ length: rows }).map((_, r) => (
        <div key={r} className="flex h-11 items-center gap-6 border-b border-rule px-3">
          {Array.from({ length: cols }).map((_, c) => (
            <Skeleton key={c} className={c === 0 ? "w-48" : "w-20"} />
          ))}
        </div>
      ))}
    </div>
  );
}

export function Empty({ title, children, action, className }: { title: string; children?: React.ReactNode; action?: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex flex-col items-start gap-3 rounded-[var(--radius-card)] border border-dashed border-rule p-6", className)}>
      <p className="font-medium text-ink">{title}</p>
      {children ? <div className="max-w-prose text-ink-2">{children}</div> : null}
      {action}
    </div>
  );
}

export function ErrorState({ error, retry, className }: { error: unknown; retry?: () => void; className?: string }) {
  return (
    <div role="alert" className={cn("flex flex-col items-start gap-3 rounded-[var(--radius-card)] border border-error/40 bg-error-soft p-4", className)}>
      <p className="font-medium text-error">This could not be loaded.</p>
      <code className="text-xs break-all text-ink-2">{errorMessage(error)}</code>
      {retry ? (
        <Button size="sm" onClick={retry}>
          <RotateCw aria-hidden className="size-3.5" /> Try again
        </Button>
      ) : null}
    </div>
  );
}

/** Render loading / error / empty / full for one query (design.md § Empty, loading and error states). */
export function QueryState<T>({
  query,
  empty,
  isEmpty,
  skeleton,
  children,
}: {
  query: { data: T | undefined; isPending: boolean; error: unknown; refetch: () => unknown };
  empty?: React.ReactNode;
  isEmpty?: (d: T) => boolean;
  skeleton?: React.ReactNode;
  children: (d: T) => React.ReactNode;
}) {
  if (query.isPending) return <>{skeleton ?? <TableSkeleton />}</>;
  if (query.error) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  const d = query.data as T;
  const blank = isEmpty ? isEmpty(d) : Array.isArray(d) && d.length === 0;
  if (blank && empty) return <>{empty}</>;
  return <>{children(d)}</>;
}
