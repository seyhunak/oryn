"use client";
import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { cn } from "@/lib/cn";

/** URL-driven underline tabs: the selected tab lives in `?tab=` (design.md § Layout: state lives in the URL). */
export function UrlTabs({ tabs, param = "tab" }: { tabs: { id: string; label: string; count?: number }[]; param?: string }) {
  const path = usePathname();
  const sp = useSearchParams();
  const current = sp.get(param) ?? tabs[0]?.id;
  return (
    <nav aria-label="Sections" className="mb-6 overflow-x-auto border-b border-rule">
      <ul role="tablist" className="flex min-w-max gap-1">
        {tabs.map((t) => {
          const q = new URLSearchParams(sp.toString());
          q.set(param, t.id);
          const active = t.id === current;
          return (
            <li key={t.id} role="presentation">
              <Link
                role="tab"
                aria-selected={active}
                href={`${path}?${q.toString()}`}
                scroll={false}
                className={cn(
                  "relative flex h-11 items-center gap-2 px-3 text-sm font-medium no-underline",
                  active ? "text-ink after:absolute after:inset-x-2 after:bottom-[-1px] after:h-0.5 after:bg-accent" : "text-ink-2 hover:text-ink",
                )}
              >
                {t.label}
                {t.count != null ? <span className="mono text-xs text-muted">{t.count}</span> : null}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

export function useTab(fallback: string, param = "tab") {
  return useSearchParams().get(param) ?? fallback;
}
