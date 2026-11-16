# Session Notes

## 2026-11-16

- `da84249` feat(desktop): **ORYN.app**, a Tauri 2 thin client for macOS in `apps/desktop` (see [DESKTOP.md](DESKTOP.md)).
  - Opens the web UI of a chosen server after a `/healthz` check. ⌘, switches server, ⌘R reloads.
  - The saved server lives in `~/Library/Application Support/org.oryn.desktop/settings.json` (currently `http://localhost:3100`).
  - `src-tauri` is a separate Cargo workspace. Build with `make desktop`. Unsigned; distributing it needs Developer ID + notarisation.
  - Not done: links opening a new window/tab (`target=_blank`) do nothing in the app.
- `bcf26c6` feat(web): **dark mode**. Settings → Appearance offers System / Light / Dark.
  - Tokens are in `apps/web/styles/theme-dark.css`. The pre-paint script is `lib/theme-boot.ts`. Documented in `design.md` § Dark mode.
  - Not checked by eye: the Gherkin (Monaco) editor in dark.
- Local environment:
  - Port 3000 is taken by another project's container (`aure-workspace`), so the Docker stack runs with the web UI on **:3100**: `ORYN_WEB_PORT=3100 ORYN_PUBLIC_URL=http://localhost:3100 make up`.
  - The demo is seeded. One Cart and checkout run went 2/3, failing on the planted shipping-total defect, as expected.
  - Stop with `make down`.
- Lesson: don't drive ORYN.app with `osascript` keystrokes. They land in whichever window is frontmost (one leaked the demo password into the Claude chat). Use the browser pane against the same server instead.
- Baseline: `main` in sync with `origin/main` at `bcf26c6`.

## 2026-11-13

- No code changes made.
- Baseline: `main` clean, in sync with `origin/main` at `0344229` (repo links and image names point at `seyhunak/oryn`).
- If HEAD has moved past `0344229`, check `git log 0344229..HEAD` for what changed since.
