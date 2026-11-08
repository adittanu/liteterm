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

/// Windows keeps `$Recycle.Bin`, `$WinREAgent`, `System Volume Information`
/// and friends at drive roots. Hide those, but not a user's own `$Archive`
/// folder or `$env.ps1` — only the *system* attribute tells them apart.
fn is_system_noise(name: &str, meta: &std::fs::Metadata) -> bool {
    if name == "System Volume Information" {
        return true;
    }
    if !name.starts_with('$') {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        meta.file_attributes() & FILE_ATTRIBUTE_SYSTEM != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

#[tauri::command]
pub fn local_list(path: String) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    let read = std::fs::read_dir(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
    for entry in read.flatten() {
        let Ok(mut meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        // `metadata` describes the link itself. A link to a folder has to list
        // (and open) as a folder, a link to a file as a file; only a link that
        // leads nowhere stays a "symlink". (Unix home directories are full of
        // them; on Windows this covers junctions.)
        if meta.is_symlink() {
            if let Ok(target) = std::fs::metadata(entry.path()) {
                meta = target;
            }
        }
        if is_system_noise(&name, &meta) {
            continue;
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
    let Some(home) = crate::paths::home_dir() else {
        return out;
    };
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

/// Whether `a` and `b` are one and the same entry, so that renaming one to
/// the other only changes how it is spelled.
///
/// Unix compares device and inode: `Photo.jpg` → `photo.jpg` is the same file
/// on a case-insensitive volume (default APFS) but two different files on a
/// case-sensitive one (most Linux disks), and a name match alone would let the
/// second case silently overwrite the other file. Windows keeps the name rule,
/// since NTFS is case-insensitive.
#[cfg(unix)]
fn is_same_entry(a: &str, b: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn is_same_entry(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Rename within the local filesystem. Refuses to overwrite: std::fs::rename
/// replaces an existing file (on Windows and Unix alike), which a rename must
/// never do.
#[tauri::command]
pub fn local_rename(from: String, to: String) -> Result<(), String> {
    let target_taken = std::fs::symlink_metadata(&to).is_ok();
    if target_taken && !is_same_entry(&from, &to) {
        return Err("An item with that name already exists".into());
    }
    std::fs::rename(&from, &to).map_err(|e| format!("Rename failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("liteterm-localfs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(dir: &Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    fn kind_of(listing: &[FileEntry], name: &str) -> String {
        listing.iter().find(|e| e.name == name).map(|e| e.kind.clone()).unwrap_or_default()
    }

    /// A junction is how Windows says "this folder is really that one".
    #[cfg(windows)]
    #[test]
    fn a_junction_to_a_folder_lists_as_a_folder() {
        let dir = scratch("junction");
        std::fs::create_dir(dir.join("real")).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &at(&dir, "link"), &at(&dir, "real")])
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed: {made:?}");
        let listing = local_list(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(kind_of(&listing, "real"), "dir");
        assert_eq!(kind_of(&listing, "link"), "dir");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_list_as_what_they_point_to() {
        use std::os::unix::fs::symlink;
        let dir = scratch("symlinks");
        std::fs::create_dir(dir.join("folder")).unwrap();
        std::fs::write(dir.join("file.txt"), "x").unwrap();
        symlink(dir.join("folder"), dir.join("to-folder")).unwrap();
        symlink(dir.join("file.txt"), dir.join("to-file")).unwrap();
        symlink(dir.join("nowhere"), dir.join("broken")).unwrap();
        let listing = local_list(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(kind_of(&listing, "to-folder"), "dir");
        assert_eq!(kind_of(&listing, "to-file"), "file");
        assert_eq!(kind_of(&listing, "broken"), "symlink");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_never_replaces_another_entry() {
        let dir = scratch("clobber");
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        assert!(local_rename(at(&dir, "a.txt"), at(&dir, "b.txt")).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_case_only_rename_goes_through() {
        let dir = scratch("case");
        std::fs::write(dir.join("photo.jpg"), "x").unwrap();
        local_rename(at(&dir, "photo.jpg"), at(&dir, "Photo.jpg")).unwrap();
        assert_eq!(names(&dir), vec!["Photo.jpg"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The data-loss case: on a case-sensitive disk `a` and `A` are two files,
    /// and renaming one over the other must be refused.
    #[cfg(unix)]
    #[test]
    fn a_case_variant_that_is_a_different_file_is_not_overwritten() {
        let dir = scratch("sensitive");
        std::fs::write(dir.join("a"), "lower").unwrap();
        std::fs::write(dir.join("A"), "upper").unwrap();
        if names(&dir).len() == 2 {
            assert!(local_rename(at(&dir, "a"), at(&dir, "A")).is_err());
            assert_eq!(std::fs::read_to_string(dir.join("A")).unwrap(), "upper");
        } // else: a case-insensitive volume (default macOS) — only one file exists
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
