//! ORYN for macOS: a native window onto an ORYN server.
//!
//! The app bundles one local page, the connect screen (`ui/`). It finds the server URL
//! (`ORYN_SERVER_URL`, then the saved choice, then `http://localhost:3000`), checks the
//! server's `/healthz`, saves the URL and navigates the window to the server's web UI.
//! The server's pages get no IPC access; only the connect screen can call the commands
//! below (see `build.rs` and `capabilities/default.json`).

use std::{fs, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Manager, Url, WebviewWindow,
};

const DEFAULT_SERVER_URL: &str = "http://localhost:3000";
const MAIN_WINDOW: &str = "main";
#[cfg(not(windows))]
const CONNECT_PAGE: &str = "tauri://localhost/index.html";
#[cfg(windows)]
const CONNECT_PAGE: &str = "http://tauri.localhost/index.html";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    server_url: Option<String>,
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app).ok_or("no config directory for this app")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// Accepts `host:port`, `http://…` or `https://…`; returns the URL without a trailing slash.
fn normalize_server_url(input: &str) -> Result<Url, String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Enter the address of your ORYN server.".into());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };
    let url = Url::parse(&with_scheme).map_err(|e| format!("That address isn't valid: {e}."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Use an http:// or https:// address.".into());
    }
    if url.host_str().is_none() {
        return Err("The address needs a host name.".into());
    }
    Ok(url)
}

fn display_url(url: &Url) -> String {
    url.as_str().trim_end_matches('/').to_owned()
}

#[derive(Serialize)]
struct ConnectError {
    /// `invalid` (bad address), `unreachable` (no answer) or `unhealthy` (answered, but not OK).
    kind: &'static str,
    message: String,
}

impl ConnectError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

async fn check_health(server: &Url) -> Result<(), ConnectError> {
    let health = server
        .join("healthz")
        .map_err(|e| ConnectError::new("invalid", e.to_string()))?;
    let client = reqwest::Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .build()
        .map_err(|e| ConnectError::new("unreachable", e.to_string()))?;
    match client.get(health).send().await {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => Err(ConnectError::new(
            "unhealthy",
            format!(
                "The server answered, but its health check returned HTTP {}.",
                response.status().as_u16()
            ),
        )),
        Err(e) if e.is_timeout() => Err(ConnectError::new(
            "unreachable",
            "The server didn't answer within 4 seconds.",
        )),
        Err(_) => Err(ConnectError::new(
            "unreachable",
            "Couldn't reach the server. Is it running?",
        )),
    }
}

/// The URL the connect screen should start from.
#[tauri::command]
fn get_server_url(app: AppHandle) -> String {
    std::env::var("ORYN_SERVER_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| load_settings(&app).server_url)
        .unwrap_or_else(|| DEFAULT_SERVER_URL.to_owned())
}

/// Checks the server (unless `force`), remembers it and opens its web UI in this window.
#[tauri::command]
async fn connect(
    app: AppHandle,
    window: WebviewWindow,
    url: String,
    force: bool,
) -> Result<(), ConnectError> {
    let server = normalize_server_url(&url).map_err(|m| ConnectError::new("invalid", m))?;
    // `join` resolves against the last path segment, so make sure the base ends in `/`.
    let base = Url::parse(&format!("{}/", display_url(&server)))
        .map_err(|e| ConnectError::new("invalid", e.to_string()))?;
    if !force {
        check_health(&base).await?;
    }
    save_settings(
        &app,
        &Settings {
            server_url: Some(display_url(&server)),
        },
    )
    .map_err(|e| ConnectError::new("invalid", format!("Couldn't save the address: {e}")))?;
    window
        .navigate(base)
        .map_err(|e| ConnectError::new("unreachable", e.to_string()))
}

fn show_connect_screen(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        if let Ok(url) = Url::parse(&format!("{CONNECT_PAGE}?manual=1")) {
            let _ = window.navigate(url);
        }
        let _ = window.set_focus();
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let items = menu.items()?;

    // App menu: "Server…" sits where macOS apps keep Settings.
    if let Some(app_menu) = items.first().and_then(|item| item.as_submenu()) {
        let server = MenuItem::with_id(app, "change-server", "Server…", true, Some("CmdOrCtrl+,"))?;
        app_menu.insert_items(&[&server, &PredefinedMenuItem::separator(app)?], 2)?;
    }

    // View menu: a reload for when the server restarts underneath the window.
    let view_menu = items
        .iter()
        .filter_map(|item| item.as_submenu())
        .find(|submenu| submenu.text().map(|text| text == "View").unwrap_or(false));
    if let Some(view_menu) = view_menu {
        let reload = MenuItem::with_id(app, "reload", "Reload", true, Some("CmdOrCtrl+R"))?;
        view_menu.insert_items(&[&reload, &PredefinedMenuItem::separator(app)?], 0)?;
    }

    Ok(menu)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .menu(build_menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "change-server" => show_connect_screen(app),
            "reload" => {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    let _ = window.eval("window.location.reload()");
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![get_server_url, connect])
        .run(tauri::generate_context!())
        .expect("error while running ORYN");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_http_when_scheme_is_missing() {
        let url = normalize_server_url("localhost:3000").unwrap();
        assert_eq!(display_url(&url), "http://localhost:3000");
    }

    #[test]
    fn strips_trailing_slashes_and_whitespace() {
        let url = normalize_server_url("  https://oryn.example.com/  ").unwrap();
        assert_eq!(display_url(&url), "https://oryn.example.com");
    }

    #[test]
    fn keeps_a_path_prefix() {
        let url = normalize_server_url("https://example.com/oryn/").unwrap();
        assert_eq!(display_url(&url), "https://example.com/oryn");
    }

    #[test]
    fn rejects_other_schemes_and_empty_input() {
        assert!(normalize_server_url("ftp://example.com").is_err());
        assert!(normalize_server_url("   ").is_err());
    }
}
