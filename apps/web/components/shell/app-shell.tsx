"use client";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { DropdownMenu } from "radix-ui";
import { ChevronsUpDown, LogOut, Menu, X } from "lucide-react";
import { api, ApiError } from "@/lib/api/client";
import { useMe, useWorkspace } from "@/lib/hooks";
import { usePrefs } from "@/lib/store";
import { cn } from "@/lib/cn";
import { NAV } from "./nav";
import { Wordmark } from "./wordmark";

export function AppShell({ children }: { children: React.ReactNode }) {
  const me = useMe();
  const router = useRouter();
  const path = usePathname();
  // The mobile menu belongs to the path it was opened on, so navigating closes it.
  const [openAt, setOpenAt] = useState<string | null>(null);
  const open = openAt === path;
  const setOpen = (next: boolean | ((o: boolean) => boolean)) =>
    setOpenAt((typeof next === "function" ? next(open) : next) ? path : null);

  useEffect(() => {
    if (me.error instanceof ApiError && me.error.status === 401) router.replace(`/login?next=${encodeURIComponent(path)}`);
  }, [me.error, router, path]);

  if (me.isPending || (me.error instanceof ApiError && me.error.status === 401)) {
    return (
      <div className="grid min-h-dvh place-items-center" role="status">
        <Wordmark />
        <span className="visually-hidden">Loading your session</span>
      </div>
    );
  }
  if (me.error) {
    return (
      <div className="grid min-h-dvh place-items-center p-6">
        <div className="flex max-w-md flex-col gap-3">
          <Wordmark />
          <p>The ORYN API is not reachable from this page.</p>
          <code className="text-xs text-ink-2">{String((me.error as Error).message)}</code>
          <button className="btn w-fit" onClick={() => me.refetch()}>
            Try again
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-dvh">
      <a href="#main" className="btn sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[600]">
        Skip to content
      </a>
      <TopBar onMenu={() => setOpen((o) => !o)} menuOpen={open} />
      <div className="flex">
        <aside
          id="sidebar"
          className={cn(
            "fixed top-[var(--topbar)] bottom-0 left-0 z-[200] w-[var(--sidebar)] overflow-y-auto border-r border-rule bg-paper-2 p-3",
            "max-[60rem]:transition-transform max-[60rem]:duration-[var(--dur-short)]",
            open ? "max-[60rem]:translate-x-0" : "max-[60rem]:-translate-x-full",
          )}
        >
          <SideNav />
        </aside>
        {open ? <button aria-label="Close menu" className="fixed inset-0 top-[var(--topbar)] z-[150] bg-[var(--scrim)] min-[60rem]:hidden" onClick={() => setOpen(false)} /> : null}
        <main id="main" className="min-w-0 flex-1 min-[60rem]:pl-[var(--sidebar)]">
          <div className="mx-auto w-full max-w-[var(--container)] px-[var(--gutter)] py-6">{children}</div>
        </main>
      </div>
    </div>
  );
}

function SideNav() {
  const path = usePathname();
  return (
    <nav aria-label="Main">
      <ul className="flex flex-col gap-0.5">
        {NAV.map(({ href, label, icon: Icon }) => {
          const active = path === href || path.startsWith(`${href}/`);
          return (
            <li key={href}>
              <Link
                href={href}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "relative flex h-10 items-center gap-3 rounded-[var(--radius-input)] px-3 text-sm font-medium no-underline transition-colors duration-[var(--dur-micro)]",
                  active ? "bg-paper-3 text-ink before:absolute before:inset-y-2 before:left-0 before:w-0.5 before:rounded-full before:bg-accent" : "text-ink-2 hover:bg-paper-3 hover:text-ink",
                )}
              >
                <Icon aria-hidden className="size-4 shrink-0" strokeWidth={1.75} />
                {label}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

function TopBar({ onMenu, menuOpen }: { onMenu: () => void; menuOpen: boolean }) {
  const { ws, me } = useWorkspace();
  const setWorkspace = usePrefs((s) => s.setWorkspace);
  const qc = useQueryClient();
  const router = useRouter();
  const logout = async () => {
    await api.POST("/api/v1/auth/logout");
    qc.clear();
    router.replace("/login");
  };
  return (
    <header className="sticky top-0 z-[300] flex h-[var(--topbar)] items-center gap-3 border-b border-rule bg-paper px-[var(--gutter)]">
      <button className="btn btn-ghost btn-icon min-[60rem]:hidden" aria-label={menuOpen ? "Close menu" : "Open menu"} aria-expanded={menuOpen} aria-controls="sidebar" onClick={onMenu}>
        {menuOpen ? <X aria-hidden className="size-5" /> : <Menu aria-hidden className="size-5" />}
      </button>
      <Link href="/overview" className="no-underline" aria-label="ORYN overview">
        <Wordmark />
      </Link>
      <span aria-hidden className="h-6 w-px bg-rule max-[40rem]:hidden" />
      <DropdownMenu.Root>
        <DropdownMenu.Trigger className="btn btn-ghost max-w-[16rem] max-[40rem]:hidden">
          <span className="truncate">{ws?.name ?? "No workspace"}</span>
          <ChevronsUpDown aria-hidden className="size-3.5 shrink-0" />
        </DropdownMenu.Trigger>
        <Menu2>
          <DropdownMenu.Label className="px-2 py-1 text-xs text-muted">Workspaces</DropdownMenu.Label>
          {me?.workspaces.map((w) => (
            <DropdownMenu.Item
              key={w.id}
              className="menu-item"
              onSelect={() => {
                setWorkspace(w.id);
                qc.invalidateQueries();
                router.push("/overview");
              }}
            >
              <span className="truncate">{w.name}</span>
              <span className="ml-auto text-xs text-muted">{w.id === ws?.id ? "current" : w.role}</span>
            </DropdownMenu.Item>
          ))}
          <DropdownMenu.Separator className="my-1 h-px bg-rule" />
          <DropdownMenu.Item className="menu-item" onSelect={() => router.push("/settings?tab=workspace&new=1")}>
            New workspace
          </DropdownMenu.Item>
        </Menu2>
      </DropdownMenu.Root>
      <div className="ml-auto flex items-center gap-2">
        <DropdownMenu.Root>
          <DropdownMenu.Trigger className="btn btn-ghost">
            <span className="grid size-7 place-items-center rounded-full bg-p-thu text-xs font-medium text-ink">{initials(me?.user.name)}</span>
            <span className="max-w-[10rem] truncate max-[40rem]:hidden">{me?.user.name}</span>
          </DropdownMenu.Trigger>
          <Menu2 align="end">
            <div className="px-2 py-1">
              <p className="font-medium">{me?.user.name}</p>
              <p className="text-xs text-muted">{me?.user.email}</p>
            </div>
            <DropdownMenu.Separator className="my-1 h-px bg-rule" />
            <DropdownMenu.Item className="menu-item" onSelect={() => router.push("/settings?tab=tokens")}>
              API tokens
            </DropdownMenu.Item>
            <DropdownMenu.Item className="menu-item" onSelect={logout}>
              <LogOut aria-hidden className="size-4" /> Sign out
            </DropdownMenu.Item>
          </Menu2>
        </DropdownMenu.Root>
      </div>
    </header>
  );
}

function Menu2({ children, align = "start" }: { children: React.ReactNode; align?: "start" | "end" }) {
  return (
    <DropdownMenu.Portal>
      <DropdownMenu.Content align={align} sideOffset={6} className="z-[350] min-w-56 rounded-[var(--radius-input)] border border-rule bg-white p-1 shadow-lift">
        {children}
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  );
}

function initials(name?: string) {
  return (name ?? "?")
    .split(/\s+/)
    .map((p) => p[0])
    .slice(0, 2)
    .join("")
    .toUpperCase();
}
