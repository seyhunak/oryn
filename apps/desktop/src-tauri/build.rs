fn main() {
    // Declare the app's own commands so they need an explicit permission. Only the bundled
    // connect screen (see capabilities/default.json) gets one; the remote ORYN UI gets none.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["get_server_url", "connect"])),
    )
    .expect("failed to run tauri-build");
}
