// Server-safe (no "use client"): the root layout inlines THEME_BOOT_SCRIPT into <head>.

/** localStorage key written by the `usePrefs` store (zustand persist). */
export const PREFS_STORAGE_KEY = "oryn.prefs";

/**
 * Runs in <head> before first paint so a dark preference never flashes light.
 * Mirrors `applyTheme` in lib/theme.ts; keep the two in step.
 */
export const THEME_BOOT_SCRIPT = `(function(){try{var p=JSON.parse(localStorage.getItem(${JSON.stringify(PREFS_STORAGE_KEY)})||"null");var t=p&&p.state&&p.state.theme;if(t==="light"||t==="dark")document.documentElement.dataset.theme=t;}catch(e){}})();`;
