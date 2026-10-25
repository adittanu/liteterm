use crate::store::models::FileEntry;

#[tauri::command]
pub fn local_drives() -> Vec<String> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Storage::FileSystem::GetLogicalDriveStringsW;
        let mut buf = [0u16; 512];
        let len = GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr());
        if len == 0 || len as usize > buf.len() {
            return vec![];
        }
        String::from_utf16_lossy(&buf[..len as usize])
            .split('\0')
            .filter(|d| !d.is_empty())
            .map(|d| d.to_string())
            .collect()
    }
    #[cfg(not(windows))]
    vec!["/".to_string()]
}

#[tauri::command]
pub fn local_list(path: String) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    let read = std::fs::read_dir(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('$') || name == "System Volume Information" {
            continue; // Windows system noise
        }
        let kind = if meta.is_dir() {
            "dir"
        } else if meta.is_symlink() {
            "symlink"
        } else {
            "file"
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(FileEntry {
            name,
            kind: kind.to_string(),
            size: meta.len() as i64,
            mtime,
            permissions: None,
        });
    }
    Ok(entries)
}

#[derive(serde::Serialize)]
pub struct LocalPlace {
    pub label: String,
    pub path: String,
}

/// Familiar starting points for the local pane: the user's home and the
/// standard folders inside it that actually exist.
#[tauri::command]
pub fn local_places() -> Vec<LocalPlace> {
    let mut out = Vec::new();
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return out;
    };
    let home = std::path::PathBuf::from(home);
    if !home.is_dir() {
        return out;
    }
    out.push(LocalPlace { label: "Home".into(), path: home.to_string_lossy().into_owned() });
    for name in ["Desktop", "Documents", "Downloads"] {
        let p = home.join(name);
        if p.is_dir() {
            out.push(LocalPlace { label: name.into(), path: p.to_string_lossy().into_owned() });
        }
    }
    out
}

#[tauri::command]
pub fn local_mkdir(path: String) -> Result<(), String> {
    std::fs::create_dir(&path).map_err(|e| format!("Could not create folder: {e}"))
}

/// Rename within the local filesystem. Refuses to overwrite: std::fs::rename
/// replaces an existing file on Windows, which a rename must never do.
#[tauri::command]
pub fn local_rename(from: String, to: String) -> Result<(), String> {
    let same_name_case_change = from.eq_ignore_ascii_case(&to);
    if !same_name_case_change && std::path::Path::new(&to).exists() {
        return Err("An item with that name already exists".into());
    }
    std::fs::rename(&from, &to).map_err(|e| format!("Rename failed: {e}"))
}
