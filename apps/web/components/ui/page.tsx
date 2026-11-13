import Link from "next/link";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/cn";

export function PageHeader({ title, description, actions, crumbs }: { title: React.ReactNode; description?: React.ReactNode; actions?: React.ReactNode; crumbs?: { href: string; label: string }[] }) {
  return (
    <header className="mb-6 flex flex-col gap-3">
      {crumbs?.length ? (
        <nav aria-label="Breadcrumb">
          <ol className="flex flex-wrap items-center gap-1 text-xs text-muted">
            {crumbs.map((c) => (
              <li key={c.href} className="flex items-center gap-1">
                <Link href={c.href} className="hover:text-ink">
                  {c.label}
                </Link>
                <ChevronRight aria-hidden className="size-3" />
              </li>
            ))}
          </ol>
        </nav>
      ) : null}
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="max-w-none">{title}</h1>
          {description ? <p className="max-w-[72ch] text-ink-2">{description}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
    </header>
  );
}

export function Section({ title, actions, children, className, description }: { title: string; actions?: React.ReactNode; children: React.ReactNode; className?: string; description?: React.ReactNode }) {
  return (
    <section className={cn("flex flex-col gap-3", className)}>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h2>{title}</h2>
          {description ? <p className="text-ink-2">{description}</p> : null}
        </div>
        {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
      </div>
      {children}
    </section>
  );
}

/** A figure and a label. No icon tile, no trend arrow (design.md § Data visualisation). */
export function Stat({ label, value, hint, href }: { label: string; value: React.ReactNode; hint?: React.ReactNode; href?: string }) {
  const body = (
    <>
      <span className="text-xs text-muted">{label}</span>
      <span className="font-display text-[1.6rem] leading-none font-bold tracking-tight tabular-nums">{value}</span>
      {hint ? <span className="text-xs text-ink-2">{hint}</span> : null}
    </>
  );
  return href ? (
    <Link href={href} className="flex flex-col gap-2 py-1 no-underline hover:text-ink">
      {body}
    </Link>
  ) : (
    <div className="flex flex-col gap-2 py-1">{body}</div>
  );
}

export function Meta({ items }: { items: { label: string; value: React.ReactNode }[] }) {
  return (
    <dl className="grid grid-cols-[minmax(0,auto)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
      {items.map((i) => (
        <div key={i.label} className="contents">
          <dt className="text-muted">{i.label}</dt>
          <dd className="m-0 min-w-0 break-words">{i.value}</dd>
        </div>
      ))}
    </dl>
  );
}
