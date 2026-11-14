use tauri_plugin_clipboard_manager::ClipboardExt;

/// Longest text that can still be an address or a pasted `ssh` command
const MAX_LEN: usize = 512;

/// The clipboard's text, for quick connect's "From clipboard" suggestion.
/// Read natively because the web view's clipboard API asks first (macOS
/// shows a Paste button) when the user didn't just paste. Only short,
/// single-line text is handed over: anything else can't be a target, and
/// the rest of the clipboard has no business reaching the interface.
#[tauri::command]
pub async fn clipboard_text(app: tauri::AppHandle) -> Option<String> {
    let text = app.clipboard().read_text().ok()?;
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_LEN || text.contains(['\n', '\r']) {
        return None;
    }
    Some(text.to_string())
}
