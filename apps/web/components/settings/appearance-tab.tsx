"use client";
import { Monitor, Moon, Sun } from "lucide-react";
import { usePrefs } from "@/lib/store";
import { THEME_PREFS, useResolvedTheme, type ThemePref } from "@/lib/theme";
import { Section } from "@/components/ui/page";

const ICONS: Record<ThemePref, typeof Sun> = { system: Monitor, light: Sun, dark: Moon };

/** Per-person display settings. Stored in this browser only; nothing is sent to the server. */
export function AppearanceTab() {
  const theme = usePrefs((s) => s.theme);
  const setTheme = usePrefs((s) => s.setTheme);
  const resolved = useResolvedTheme();
  return (
    <Section title="Theme" description="Light, dark, or follow your system. Saved in this browser for you only.">
      <fieldset className="flex flex-col gap-2">
        <legend className="visually-hidden">Theme</legend>
        <div className="flex flex-wrap gap-2">
          {THEME_PREFS.map(({ id, label }) => {
            const Icon = ICONS[id];
            return (
              <button key={id} type="button" className="btn" aria-pressed={theme === id} onClick={() => setTheme(id)}>
                <Icon size={16} aria-hidden />
                {label}
              </button>
            );
          })}
        </div>
        <p className="text-xs text-muted" aria-live="polite">
          {theme === "system" ? `Following your system: ${resolved} right now.` : `Always ${resolved}.`}
        </p>
      </fieldset>
    </Section>
  );
}
