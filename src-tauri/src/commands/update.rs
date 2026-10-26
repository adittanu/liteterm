//! Self-update from GitHub Releases: check the latest release, stream the
//! NSIS installer to a temp dir with progress events, then run it silently
//! and bring the app back up. Only the setup.exe asset is ever executed.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const REPO: &str = "adittanu/liteterm";
const API_BASE: &str = "https://api.github.com";
/// Progress events are throttled so the webview is not flooded mid-download.
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(80);

// Handed back to the frontend by check_update and accepted back as the
// download_update argument, so it travels in both directions.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAsset {
    pub name: String,
    pub size: u64,
    pub url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: Option<String>,
    pub notes: Option<String>,
    pub asset: Option<UpdateAsset>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    downloaded: u64,
    total: u64,
}

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("LiteTerm-Updater/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Could not create HTTP client: {e}"))
}

/// Strip a leading `v`/`V` from a release tag.
fn version_digits(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

/// Numeric segment compare ("1.10.0" > "1.9.2"). Unparseable tags fall back
/// to string inequality so an odd tag still surfaces as an update.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        v.split('.')
            .map(|s| s.trim().parse::<u64>().ok())
            .collect::<Option<_>>()
    };
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => {
            let n = l.len().max(c.len());
            for i in 0..n {
                let a = l.get(i).copied().unwrap_or(0);
                let b = c.get(i).copied().unwrap_or(0);
                if a != b {
                    return a > b;
                }
            }
            false
        }
        _ => latest != current,
    }
}

/// Pick the Windows NSIS setup asset: prefer the x64 build, then any setup.
fn pick_asset(assets: &[GhAsset]) -> Option<UpdateAsset> {
    let chosen = assets
        .iter()
        .find(|a| a.name.to_lowercase().ends_with("x64-setup.exe"))
        .or_else(|| assets.iter().find(|a| a.name.to_lowercase().ends_with("-setup.exe")))
        .or_else(|| assets.iter().find(|a| a.name.to_lowercase().ends_with(".exe")))?;
    Some(UpdateAsset {
        name: chosen.name.clone(),
        size: chosen.size,
        url: chosen.browser_download_url.clone(),
    })
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();

    let resp = build_client()?
        .get(format!("{API_BASE}/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;

    // 404 = the repo has no published release yet — that is "up to date",
    // not a failure.
    if resp.status().as_u16() == 404 {
        return Ok(UpdateInfo {
            available: false,
            latest_version: current.clone(),
            current_version: current,
            release_url: None,
            notes: None,
            asset: None,
        });
    }
    if !resp.status().is_success() {
        return Err(format!("GitHub returned HTTP {}", resp.status()));
    }

    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("Could not read release: {e}"))?;

    let latest = version_digits(&release.tag_name).to_string();
    let asset = pick_asset(&release.assets);

    Ok(UpdateInfo {
        available: asset.is_some() && is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
        notes: release.body,
        asset,
    })
}

fn update_dir() -> PathBuf {
    std::env::temp_dir().join("liteterm-update")
}

/// Keep the on-disk name boring: the asset name comes from GitHub, so strip
/// it to filename-safe characters before it ever touches a path.
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() { "liteterm-setup.exe".into() } else { cleaned }
}

#[tauri::command]
pub async fn download_update(app: AppHandle, asset: UpdateAsset) -> Result<String, String> {
    let dir = update_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create update folder: {e}"))?;

    let file_name = sanitize_name(&asset.name);
    let final_path = dir.join(&file_name);
    let part_path = dir.join(format!("{file_name}.part"));

    let client = build_client()?;
    let resp = client
        .get(&asset.url)
        .send()
        .await
        .map_err(|e| format!("Could not start download: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed with HTTP {}", resp.status()));
    }

    let total: u64 = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(asset.size);

    let mut file = std::fs::File::create(&part_path)
        .map_err(|e| format!("Could not write update file: {e}"))?;
    let mut downloaded: u64 = 0;
    let mut last_emit: Option<std::time::Instant> = None;

    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Download interrupted: {e}"))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("Could not write update file: {e}"))?;
        downloaded += chunk.len() as u64;

        let due = last_emit.map_or(true, |t| t.elapsed() >= PROGRESS_EMIT_INTERVAL);
        if due || downloaded >= total {
            let _ = app.emit("update:progress", ProgressEvent { downloaded, total });
            last_emit = Some(std::time::Instant::now());
        }
    }
    drop(file);

    if total > 0 && downloaded != total {
        let _ = std::fs::remove_file(&part_path);
        return Err(format!("Download incomplete ({downloaded} of {total} bytes)"));
    }

    std::fs::rename(&part_path, &final_path)
        .map_err(|e| format!("Could not finalize update file: {e}"))?;
    let _ = app.emit(
        "update:progress",
        ProgressEvent { downloaded: total, total },
    );

    Ok(final_path.to_string_lossy().into_owned())
}

/// `\\?\C:\foo` -> `C:\foo` (and `\\?\UNC\srv\share` -> `\\srv\share`).
fn strip_verbatim_prefix(p: &std::path::Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

#[tauri::command]
pub async fn install_update(app: AppHandle, path: String) -> Result<(), String> {
    let installer = PathBuf::from(path);

    // Only ever execute installers this app downloaded into its own temp
    // folder — a path from the frontend must not become arbitrary exec.
    let dir = update_dir().canonicalize().map_err(|e| format!("Update folder missing: {e}"))?;
    let installer = installer
        .canonicalize()
        .map_err(|e| format!("Update file is missing: {e}"))?;
    let is_exe = installer
        .extension()
        .map(|e| e.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);
    if !installer.starts_with(&dir) || !is_exe {
        return Err("Refusing to run an unexpected file".into());
    }

    let exe = std::env::current_exe().map_err(|e| format!("Could not locate LiteTerm: {e}"))?;

    // canonicalize() yields `\\?\C:\...` verbatim paths on Windows, which
    // cmd.exe / `start` cannot parse (they get cut down to `\\` and Windows
    // reports "cannot find '\\'"). Hand cmd plain drive-letter paths.
    let installer = strip_verbatim_prefix(&installer);
    let exe = strip_verbatim_prefix(&exe);

    // A detached cmd wrapper survives this process being killed by the
    // silent installer: it waits for the installer to finish, then starts
    // the freshly replaced LiteTerm at the same path.
    let script = format!(
        "start /wait \"\" \"{}\" /S & timeout /t 2 /nobreak >nul & start \"\" \"{}\"",
        installer.display(),
        exe.display()
    );

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // raw_arg: Rust's normal arg quoting escapes `"` as `\"`, which cmd
        // does not understand. The outer quotes are stripped by cmd /C.
        std::process::Command::new("cmd.exe")
            .raw_arg(format!("/C \"{script}\""))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Could not start the installer: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = script;
        return Err("Automatic updates are only supported on Windows".into());
    }

    // Give the wrapper a beat to come alive before the app tears down.
    tokio::time::sleep(Duration::from_millis(250)).await;
    app.exit(0);
    Ok(())
}
