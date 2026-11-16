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

/// Largest text a terminal program may put on the clipboard (OSC 52)
const MAX_WRITE_LEN: usize = 1 << 20;

/// Puts text on the clipboard for a terminal program's OSC 52 "copy".
/// Written natively because the web view's clipboard API needs a recent
/// user gesture (WKWebView refuses without one), and an OSC 52 copy from a
/// remote program arrives some time after the mouse-up that caused it.
#[tauri::command]
pub async fn clipboard_write_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
    if text.len() > MAX_WRITE_LEN {
        return Err("Text is too large for the clipboard".into());
    }
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}
