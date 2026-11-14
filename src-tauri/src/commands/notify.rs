use tauri_plugin_notification::NotificationExt;

/// A desktop notification, for a terminal tab that finished a long command
/// or rang its bell while LiteTerm wasn't in front. Called from Rust so the
/// web view needs no notification permission of its own.
#[tauri::command]
pub fn notify(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}
