"use client";
import { useEffect, useSyncExternalStore } from "react";
import { usePrefs } from "@/lib/store";

/** What the person picked in Settings → Appearance. `system` follows the OS. */
export type ThemePref = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_PREFS: { id: ThemePref; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

/** Light and dark set `data-theme` on <html>; system removes it so the CSS media query decides. */
export function applyTheme(pref: ThemePref) {
  const root = document.documentElement;
  if (pref === "system") delete root.dataset.theme;
  else root.dataset.theme = pref;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function subscribeSystem(onChange: () => void) {
  const mq = window.matchMedia(DARK_QUERY);
  mq.addEventListener("change", onChange);
  return () => mq.removeEventListener("change", onChange);
}

function systemIsDark() {
  return window.matchMedia(DARK_QUERY).matches;
}

/** The theme actually on screen, following the OS when the preference is `system`. */
export function useResolvedTheme(): ResolvedTheme {
  const pref = usePrefs((s) => s.theme);
  const osDark = useSyncExternalStore(subscribeSystem, systemIsDark, () => false);
  if (pref === "system") return osDark ? "dark" : "light";
  return pref;
}

/** Keeps <html data-theme> in step with the stored preference. Mounted once in Providers. */
export function ThemeSync() {
  const pref = usePrefs((s) => s.theme);
  useEffect(() => applyTheme(pref), [pref]);
  return null;
}
