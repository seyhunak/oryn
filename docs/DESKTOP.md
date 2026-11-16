# ORYN for macOS

`apps/desktop` builds **ORYN.app**, a native macOS app built with [Tauri 2](https://tauri.app).
It is a thin window: it opens the ORYN web UI from a server you choose. It doesn't run
the API, the worker, Postgres or the Playwright runner itself. Start those with
`make up` (Docker) or `make dev` (from source), or point the app at a shared server.

## Build

You need Rust, Node 20+ and Xcode's command-line tools.

```bash
make desktop
```

This writes `ORYN.app` and a `.dmg` to `apps/desktop/src-tauri/target/release/bundle/`.
The build is unsigned. The first time you open it, Control-click the app and choose
**Open**. Distributing it needs an Apple Developer ID and notarisation (see Tauri's
[macOS signing guide](https://tauri.app/distribute/sign/macos/)).

To run it while you work on it:

```bash
ORYN_SERVER_URL=http://localhost:3000 make desktop-dev
```

## Using it

- On launch the app checks the server's `/healthz`. If the server is healthy, it opens
  the web UI. Otherwise it shows the connect screen with the reason.
- **ORYN → Server…** (<kbd>⌘</kbd><kbd>,</kbd>) opens the connect screen again so you
  can switch servers. **Open anyway** skips the health check, for servers that don't
  expose `/healthz`.
- **View → Reload** (<kbd>⌘</kbd><kbd>R</kbd>) reloads the page after a server restart.
- The chosen address is saved in
  `~/Library/Application Support/org.oryn.desktop/settings.json`.
  `ORYN_SERVER_URL` overrides it.
- Plain-HTTP servers work, both on localhost and on your LAN. Use HTTPS for anything
  reachable beyond that.

## How it's built

| Path | Contents |
|---|---|
| `ui/` | The bundled connect screen (static HTML, CSS and JS). |
| `src-tauri/src/lib.rs` | Settings, URL normalisation, the health check, the menu, and the `get_server_url` / `connect` commands. |
| `src-tauri/capabilities/default.json` | Only the bundled connect screen may call those two commands. Pages loaded from the ORYN server get no Tauri IPC access. |
| `src-tauri/Info.plist` | App Transport Security exceptions for self-hosted HTTP servers. |
| `src-tauri/icons/` | Generated from `apps/web/public/icon.svg` with `npm run icons`. |

`src-tauri` is a separate Cargo workspace, so the server's `cargo build`, tests and CI
never compile Tauri. Run its unit tests with `cd apps/desktop/src-tauri && cargo test`.
