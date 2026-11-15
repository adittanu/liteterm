//! Self-update from GitHub Releases: check the latest release, stream the
//! right asset for this install to a temp dir with progress events, check it
//! against the SHA-256 digest GitHub publishes, then put it in place and
//! bring the app back up.
//!
//! How it goes in depends on how LiteTerm was installed ([`Install`]):
//! - Windows: the NSIS setup runs silently.
//! - macOS: the .app is copied out of the dmg next to the running bundle, and
//!   swapped in once the app has quit.
//! - Linux AppImage: the new AppImage replaces the file at `$APPIMAGE`.
//! - Linux .deb/.rpm: the package manager installs it through pkexec, which
//!   asks for the user's password.
//!
//! Anything else (a build from source, a bundle that cannot be written to)
//! gets `auto_install: false`, and the app opens the release page instead.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const REPO: &str = "adittanu/liteterm";
const API_BASE: &str = "https://api.github.com";
/// Progress events are throttled so the webview is not flooded mid-download.
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(80);

// Handed back to the frontend by check_update and accepted back as the
// download_update argument, so it travels in both directions.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAsset {
    pub name: String,
    pub size: u64,
    pub url: String,
    /// Lowercase hex SHA-256 from the release; the download must match it
    #[serde(default)]
    pub sha256: Option<String>,
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
    /// True when the app can download and install the update itself
    pub auto_install: bool,
    /// How it installs: "installer" (Windows), "app" (macOS), "appimage",
    /// "package" (deb/rpm, asks for a password) or "page" (the release page)
    pub install_method: &'static str,
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
    /// "sha256:<hex>", on assets uploaded since GitHub started computing them
    #[serde(default)]
    digest: Option<String>,
}

/// The asset the last `check_update` picked. `download_update` takes its
/// address and digest from here, not from what the frontend sends back.
static OFFERED: Mutex<Option<UpdateAsset>> = Mutex::new(None);

/// The file the last `download_update` finished, with the SHA-256 it had.
/// `install_update` hashes the file again and runs it only if it still matches.
static DOWNLOADED: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    downloaded: u64,
    total: u64,
}

/// How this copy of LiteTerm was installed: which asset updates it, and how
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Install {
    /// The NSIS installer
    Windows,
    /// An .app bundle, updated from the dmg
    MacApp,
    /// The AppImage file named by `$APPIMAGE`
    AppImage,
    /// A package from the .deb or .rpm
    Deb,
    Rpm,
    /// A build from source or anything unrecognised: release page only
    Other,
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

/// The asset suffixes that update `install`, best first. An install the app
/// cannot update itself still names one, so the user hears about the release.
fn wanted_suffixes(install: Install, os: &str) -> &'static [&'static str] {
    match install {
        Install::Windows => &["x64-setup.exe", "-setup.exe", ".exe"],
        Install::MacApp => &[".dmg"],
        Install::AppImage => &[".appimage"],
        Install::Deb => &[".deb"],
        Install::Rpm => &[".rpm"],
        Install::Other => match os {
            "windows" => &["x64-setup.exe", "-setup.exe", ".exe"],
            "macos" => &[".dmg"],
            "linux" => &[".appimage", ".deb", ".rpm"],
            _ => &[],
        },
    }
}

/// The release asset meant for this install. A release that lacks it (its
/// build failed, or has not finished) offers no update here at all.
fn pick_asset_for(install: Install, os: &str, assets: &[GhAsset]) -> Option<UpdateAsset> {
    let chosen = wanted_suffixes(install, os)
        .iter()
        .find_map(|suffix| assets.iter().find(|a| a.name.to_lowercase().ends_with(suffix)))?;
    Some(UpdateAsset {
        name: chosen.name.clone(),
        size: chosen.size,
        url: chosen.browser_download_url.clone(),
        sha256: chosen.digest.as_deref().and_then(sha256_of_digest),
    })
}

/// "sha256:ABC…" -> "abc…"; any other algorithm, or a malformed value, is None
fn sha256_of_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

/// `.../LiteTerm.app/Contents/MacOS/liteterm` -> `.../LiteTerm.app`
fn mac_bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let is_app = bundle.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (is(macos, "MacOS") && is(contents, "Contents") && is_app).then(|| bundle.to_path_buf())
}

fn detect_install() -> Install {
    static INSTALL: std::sync::OnceLock<Install> = std::sync::OnceLock::new();
    *INSTALL.get_or_init(|| {
        if cfg!(windows) {
            return Install::Windows;
        }
        let exe = std::env::current_exe().ok();
        if cfg!(target_os = "macos") {
            return if exe.as_deref().and_then(mac_bundle_of).is_some() { Install::MacApp } else { Install::Other };
        }
        if cfg!(target_os = "linux") {
            if appimage_path().is_some() {
                return Install::AppImage;
            }
            if let Some(exe) = exe {
                // The package manager that owns the running binary
                if quietly_succeeds("dpkg", &[exe.as_os_str()]) {
                    return Install::Deb;
                }
                if quietly_succeeds("rpm", &[std::ffi::OsStr::new("-qf"), exe.as_os_str()]) {
                    return Install::Rpm;
                }
            }
        }
        Install::Other
    })
}

/// Runs `dpkg -S <path>` (the "-S" is added for dpkg) or the given command,
/// and says whether it exited 0
fn quietly_succeeds(program: &str, args: &[&std::ffi::OsStr]) -> bool {
    let mut cmd = std::process::Command::new(program);
    if program == "dpkg" {
        cmd.arg("-S");
    }
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The AppImage this process runs from, if it does. The AppImage runtime
/// exports `APPIMAGE` and `APPDIR`, and every child inherits them, so a
/// LiteTerm started from another AppImage (a terminal, a launcher) sees that
/// program's file. It counts only when the running binary sits inside the
/// mount `APPDIR` names.
fn appimage_path() -> Option<PathBuf> {
    let image = PathBuf::from(std::env::var_os("APPIMAGE")?);
    let appdir = PathBuf::from(std::env::var_os("APPDIR")?);
    let exe = std::env::current_exe().ok()?;
    runs_from_appimage(&exe, &appdir).then_some(())?;
    image.is_file().then_some(image)
}

fn runs_from_appimage(exe: &Path, appdir: &Path) -> bool {
    if appdir.as_os_str().is_empty() {
        return false;
    }
    // Compared both as given and resolved: the mount path may contain a link
    let resolved = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    exe.starts_with(appdir) || resolved(exe).starts_with(resolved(appdir))
}

/// An executable on PATH, as an absolute path
fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Whether this process can create files in `dir`
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".liteterm-update-check-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Can this install update itself? Checked when the update is offered, so a
/// copy that cannot gets the release page instead of a failure at the end.
fn self_install_possible(install: Install) -> bool {
    match install {
        Install::Windows => true,
        Install::MacApp => std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(mac_bundle_of)
            .is_some_and(|bundle| {
                // Opened from Downloads without moving it, macOS runs a
                // read-only copy from a random path; started from the dmg,
                // the bundle sits on a read-only volume
                !bundle.to_string_lossy().contains("/AppTranslocation/")
                    && bundle.parent().is_some_and(writable)
            }),
        Install::AppImage => appimage_path().and_then(|p| p.parent().map(writable)).unwrap_or(false),
        Install::Deb | Install::Rpm => find_in_path("pkexec").is_some(),
        Install::Other => false,
    }
}

fn install_method(install: Install, auto: bool) -> &'static str {
    if !auto {
        return "page";
    }
    match install {
        Install::Windows => "installer",
        Install::MacApp => "app",
        Install::AppImage => "appimage",
        Install::Deb | Install::Rpm => "package",
        Install::Other => "page",
    }
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let install = detect_install();
    let auto_install = self_install_possible(install);
    let method = install_method(install, auto_install);

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
            auto_install,
            install_method: method,
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
    let asset = pick_asset_for(install, std::env::consts::OS, &release.assets);
    *OFFERED.lock().unwrap_or_else(|e| e.into_inner()) = asset.clone();

    Ok(UpdateInfo {
        available: asset.is_some() && is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
        notes: release.body,
        asset,
        auto_install,
        install_method: method,
    })
}

/// Where updates are downloaded. Windows and macOS temp folders are already
/// per-user. On Linux the temp folder is `/tmp`, shared by every account, and
/// a .deb from there is later run as root, so updates go to the app's cache
/// folder in the user's home instead.
fn update_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if cfg!(any(windows, target_os = "macos")) {
        return Ok(std::env::temp_dir().join("liteterm-update"));
    }
    let cache = app.path().app_cache_dir().map_err(|e| format!("Could not find the cache folder: {e}"))?;
    Ok(cache.join("update"))
}

/// Create the update folder if needed, and make sure only this user can
/// write to it. Refuses a folder that is a link, or that belongs to someone
/// else (changing its mode fails then).
fn prepare_update_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create update folder: {e}"))?;
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("Could not check the update folder: {e}"))?;
    if !meta.is_dir() {
        return Err("The update folder is not a plain folder, so LiteTerm won't use it".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "The update folder belongs to another user, so LiteTerm won't use it".to_string())?;
        let mode = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err("Other users can write to the update folder, so LiteTerm won't use it".into());
        }
    }
    Ok(())
}

/// Lowercase hex SHA-256 of a file
fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Could not read the update file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("Could not read the update file: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// Keep the on-disk name boring: the asset name comes from GitHub, so strip
/// it to filename-safe characters before it ever touches a path.
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() || cleaned.starts_with('.') { format!("liteterm-update{cleaned}") } else { cleaned }
}

#[tauri::command]
pub async fn download_update(app: AppHandle, asset: UpdateAsset) -> Result<String, String> {
    // The address and digest come from what check_update picked; the
    // frontend only says which one it wants
    let asset = OFFERED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|offered| offered.name == asset.name)
        .ok_or("Check for updates again, then download")?;

    let dir = update_dir(&app)?;
    prepare_update_dir(&dir)?;
    *DOWNLOADED.lock().unwrap_or_else(|e| e.into_inner()) = None;

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
    let mut hasher = Sha256::new();
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
        hasher.update(&chunk);
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
    let got = hex_lower(&hasher.finalize());
    if let Some(want) = &asset.sha256 {
        if &got != want {
            let _ = std::fs::remove_file(&part_path);
            return Err("The download does not match the checksum GitHub published for it, so it was deleted. Try again.".into());
        }
    }

    std::fs::rename(&part_path, &final_path)
        .map_err(|e| format!("Could not finalize update file: {e}"))?;
    *DOWNLOADED.lock().unwrap_or_else(|e| e.into_inner()) = Some((final_path.clone(), got));
    let _ = app.emit(
        "update:progress",
        ProgressEvent { downloaded: total, total },
    );

    Ok(final_path.to_string_lossy().into_owned())
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The file type each install takes
fn expected_extension(install: Install) -> Option<&'static str> {
    match install {
        Install::Windows => Some("exe"),
        Install::MacApp => Some("dmg"),
        Install::AppImage => Some("appimage"),
        Install::Deb => Some("deb"),
        Install::Rpm => Some("rpm"),
        Install::Other => None,
    }
}

#[tauri::command]
pub async fn install_update(app: AppHandle, path: String) -> Result<(), String> {
    let install = detect_install();
    if !self_install_possible(install) {
        return Err("This copy of LiteTerm can't update itself; download the new version from the release page".into());
    }

    // Only ever use the file this app downloaded into its own update folder,
    // unchanged since: a path from the frontend must not become arbitrary exec.
    let dir = update_dir(&app)?;
    prepare_update_dir(&dir)?;
    let dir = dir.canonicalize().map_err(|e| format!("Update folder missing: {e}"))?;
    let file = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("Update file is missing: {e}"))?;
    let ext_ok = match (file.extension(), expected_extension(install)) {
        (Some(e), Some(want)) => e.eq_ignore_ascii_case(want),
        _ => false,
    };
    if !file.starts_with(&dir) || !ext_ok {
        return Err("Refusing to run an unexpected file".into());
    }
    let (downloaded, want) = DOWNLOADED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or("Download the update again, then install")?;
    if downloaded.canonicalize().ok().as_deref() != Some(file.as_path()) {
        return Err("Refusing to run an unexpected file".into());
    }
    let check = file.clone();
    let got = tokio::task::spawn_blocking(move || sha256_file(&check))
        .await
        .map_err(|e| e.to_string())??;
    if got != want {
        let _ = std::fs::remove_file(&file);
        return Err("The update file changed after it was downloaded, so it was deleted. Download it again.".into());
    }

    match install {
        Install::Windows => install_windows(&file)?,
        Install::MacApp => install_mac(&file, &dir).await?,
        Install::AppImage => install_appimage(&file)?,
        Install::Deb | Install::Rpm => install_package(install, &file).await?,
        Install::Other => unreachable!("checked by self_install_possible"),
    }

    // Give the helper a beat to come alive before the app tears down.
    tokio::time::sleep(Duration::from_millis(250)).await;
    app.exit(0);
    Ok(())
}

// ---------- Windows ----------

/// `\\?\C:\foo` -> `C:\foo` (and `\\?\UNC\srv\share` -> `\\srv\share`).
fn strip_verbatim_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn install_windows(installer: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not locate LiteTerm: {e}"))?;

    // canonicalize() yields `\\?\C:\...` verbatim paths on Windows, which
    // cmd.exe / `start` cannot parse (they get cut down to `\\` and Windows
    // reports "cannot find '\\'"). Hand cmd plain drive-letter paths.
    let installer = strip_verbatim_prefix(installer);
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
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = script;
        Err("The Windows installer only runs on Windows".into())
    }
}

// ---------- macOS and Linux ----------

/// Waits (up to 30 s) for the app, pid "$1", to quit, then runs the rest of
/// its arguments as the command. Used to start the updated LiteTerm.
const RELAUNCH_SCRIPT: &str = r#"pid="$1"; shift
i=0
while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 300 ]; do sleep 0.1; i=$((i+1)); done
exec "$@""#;

/// Waits for the app, pid "$1", to quit, swaps the staged bundle "$2" in for
/// the installed one "$3" (putting the old one back if that fails), and
/// opens whichever is there. If the app is still running after two minutes
/// nothing is swapped: moving a running bundle and calling `open` would only
/// bring the old instance to the front.
const MAC_SWAP_SCRIPT: &str = r#"pid="$1"; staged="$2"; app="$3"
i=0
while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 1200 ]; do sleep 0.1; i=$((i+1)); done
if kill -0 "$pid" 2>/dev/null; then rm -rf "$staged"; exit 1; fi
old="$app.old.$$"
if mv "$app" "$old"; then
  if mv "$staged" "$app"; then rm -rf "$old"; else mv "$old" "$app"; fi
fi
open "$app""#;

/// Run `sh -c <script> sh <args…>` so it outlives this process. `env` lists
/// variables to set (Some) or remove (None) for it.
fn spawn_detached_sh(script: &str, args: &[&std::ffi::OsStr], env: &[(String, Option<String>)]) -> Result<(), String> {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(script)
        .arg("sh")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (k, v) in env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: nothing sent to the app's group reaches it
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not start the update helper: {e}"))
}

/// Run a helper program to the end; its error output becomes the message
fn run_tool(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Could not run {program}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    Err(format!("{program} failed{}", if last.is_empty() { String::new() } else { format!(": {last}") }))
}

/// macOS: copy the new LiteTerm.app out of the dmg next to the installed one
/// while the app still runs (so a failure can still be reported), then let a
/// helper swap it in once the app has quit.
async fn install_mac(dmg: &Path, update_dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not locate LiteTerm: {e}"))?;
    let bundle = mac_bundle_of(&exe).ok_or("LiteTerm is not running from an app bundle")?;
    let dmg = dmg.to_path_buf();
    let bundle2 = bundle.clone();
    let mount = update_dir.join(format!("mount-{}", std::process::id()));
    let staged = tokio::task::spawn_blocking(move || stage_mac_bundle(&dmg, &bundle2, &mount))
        .await
        .map_err(|e| e.to_string())??;
    let pid = std::process::id().to_string();
    spawn_detached_sh(
        MAC_SWAP_SCRIPT,
        &[pid.as_ref(), staged.as_os_str(), bundle.as_os_str()],
        &[],
    )
}

fn stage_mac_bundle(dmg: &Path, bundle: &Path, mount: &Path) -> Result<PathBuf, String> {
    let os = |s: &str| std::ffi::OsString::from(s);
    std::fs::create_dir_all(mount).map_err(|e| format!("Could not prepare the update: {e}"))?;
    run_tool(
        "hdiutil",
        &[
            &os("attach"),
            &os("-nobrowse"),
            &os("-readonly"),
            &os("-noautoopen"),
            &os("-mountpoint"),
            mount.as_os_str(),
            dmg.as_os_str(),
        ],
    )?;

    let staged = (|| {
        let app = std::fs::read_dir(&mount)
            .map_err(|e| format!("Could not read the disk image: {e}"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("app")))
            .ok_or("The disk image has no app in it")?;
        let parent = bundle.parent().ok_or("The app bundle has no folder")?;
        let name = bundle.file_name().ok_or("The app bundle has no name")?.to_string_lossy();
        // Next to the installed bundle, so the final swap is a rename on one disk
        let staged = parent.join(format!(".{name}.update"));
        if staged.exists() {
            std::fs::remove_dir_all(&staged).map_err(|e| format!("Could not clear an earlier update: {e}"))?;
        }
        if let Err(e) = run_tool("ditto", &[app.as_os_str(), staged.as_os_str()]) {
            // A copy that broke off (a full disk) would sit next to the app
            let _ = std::fs::remove_dir_all(&staged);
            return Err(e);
        }
        // Not downloaded by a browser, so there is normally nothing to clear;
        // a leftover flag would make Gatekeeper refuse the unsigned app
        let _ = run_tool("xattr", &[&os("-dr"), &os("com.apple.quarantine"), staged.as_os_str()]);
        Ok::<_, String>(staged)
    })();

    if run_tool("hdiutil", &[&os("detach"), mount.as_os_str(), &os("-quiet")]).is_err() {
        let _ = run_tool("hdiutil", &[&os("detach"), mount.as_os_str(), &os("-force")]);
    }
    let _ = std::fs::remove_dir(mount);
    staged
}

/// Variables the AppImage runtime set for the old image. Entries that point
/// into its mount (`appdir`) go; anything else the user had stays.
fn appimage_env_cleanup(vars: impl Iterator<Item = (String, String)>, appdir: &str) -> Vec<(String, Option<String>)> {
    const RUNTIME: [&str; 4] = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"];
    let mut out = Vec::new();
    for (k, v) in vars {
        if RUNTIME.contains(&k.as_str()) {
            out.push((k, None));
            continue;
        }
        if appdir.is_empty() || !v.contains(appdir) {
            continue;
        }
        let kept: Vec<&str> = v.split(':').filter(|part| !part.is_empty() && !part.starts_with(appdir)).collect();
        out.push((k, if kept.is_empty() { None } else { Some(kept.join(":")) }));
    }
    out
}

/// Linux AppImage: put the new image where the running one is, then start it
/// once this one has quit. Replacing the file is safe while it runs: this
/// process keeps the old one mounted until it exits.
fn install_appimage(downloaded: &Path) -> Result<(), String> {
    let target = appimage_path().ok_or("Could not find the AppImage LiteTerm runs from")?;
    let dir = target.parent().ok_or("The AppImage has no folder")?;
    let name = target.file_name().ok_or("The AppImage has no name")?.to_string_lossy();
    // Copied into the same folder first: a rename across disks fails
    let tmp = dir.join(format!(".{name}.update"));
    let placed = std::fs::copy(downloaded, &tmp)
        .map_err(|e| format!("Could not copy the new AppImage: {e}"))
        .and_then(|_| make_executable(&tmp));
    if let Err(e) = placed {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not replace the AppImage: {e}")
    })?;
    let _ = std::fs::remove_file(downloaded);

    let appdir = std::env::var("APPDIR").unwrap_or_default();
    let env = appimage_env_cleanup(std::env::vars(), &appdir);
    let pid = std::process::id().to_string();
    spawn_detached_sh(RELAUNCH_SCRIPT, &[pid.as_ref(), target.as_os_str()], &env)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Could not make the new AppImage executable: {e}"))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Err("AppImages only run on Linux".into())
}

/// The package manager command for a .deb or .rpm, run as root by pkexec.
/// A front end (apt-get, dnf, zypper) can fetch a dependency the new version
/// added; dpkg and rpm are the fallback.
fn package_command(install: Install, file: &Path, have: impl Fn(&str) -> Option<PathBuf>) -> Option<Vec<std::ffi::OsString>> {
    let f = file.as_os_str().to_os_string();
    let cmd = |tool: PathBuf, args: &[&str]| {
        let mut v = vec![tool.into_os_string()];
        v.extend(args.iter().map(std::ffi::OsString::from));
        v.push(f.clone());
        v
    };
    match install {
        Install::Deb => have("apt-get")
            .map(|t| cmd(t, &["install", "-y"]))
            .or_else(|| have("dpkg").map(|t| cmd(t, &["-i"]))),
        Install::Rpm => have("dnf")
            .map(|t| cmd(t, &["install", "-y"]))
            .or_else(|| have("zypper").map(|t| cmd(t, &["--non-interactive", "install", "--allow-unsigned-rpm"])))
            .or_else(|| have("rpm").map(|t| cmd(t, &["-U"]))),
        _ => None,
    }
}

/// Linux .deb/.rpm: the package manager installs the new version as root.
/// pkexec shows the desktop's password prompt; the app is still running, so
/// a cancelled prompt or a failed install is reported and nothing changes.
async fn install_package(install: Install, file: &Path) -> Result<(), String> {
    // Read before the package replaces it: afterwards Linux reports the
    // running binary's path with " (deleted)" on the end
    let exe = std::env::current_exe().map_err(|e| format!("Could not locate LiteTerm: {e}"))?;
    let exe = PathBuf::from(exe.to_string_lossy().trim_end_matches(" (deleted)"));

    let pkexec = find_in_path("pkexec").ok_or("pkexec is not installed, so LiteTerm can't ask for your password")?;
    let command = package_command(install, file, find_in_path).ok_or("No package manager found to install the update")?;
    let out = tokio::process::Command::new(&pkexec)
        .args(&command)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("Could not start pkexec: {e}"))?;
    match out.status.code() {
        Some(0) => {}
        // pkexec: the prompt was dismissed, or there is no agent to show one
        Some(126) => return Err("The password prompt was cancelled, so the update was not installed".into()),
        // pkexec uses 127 both when authorization failed (a wrong password,
        // a polkit rule) and when it could not ask at all
        Some(127) => {
            return Err("The update was not installed: the password was not accepted, or LiteTerm couldn't ask for it. \
                        Try again, install the downloaded package yourself, or update from the release page"
                .into())
        }
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            return Err(format!(
                "The package manager could not install the update{}",
                if last.is_empty() { String::new() } else { format!(": {last}") }
            ));
        }
    }

    let pid = std::process::id().to_string();
    spawn_detached_sh(RELAUNCH_SCRIPT, &[pid.as_ref(), exe.as_os_str()], &[])
}

/// Only this repo's own GitHub pages: an address from the frontend must not
/// turn this into "open anything".
fn is_release_url(url: &str) -> bool {
    let prefix = format!("https://github.com/{REPO}/");
    url.starts_with(&prefix) && !url.chars().any(char::is_whitespace)
}

/// When the app can't install an update itself: open the release page in
/// the default browser and let the user take it from there.
#[tauri::command]
pub fn open_release_page(url: String) -> Result<(), String> {
    if !is_release_url(&url) {
        return Err("Refusing to open an unexpected address".into());
    }
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(not(any(target_os = "macos", windows)))]
    let program = "xdg-open";

    let mut child = std::process::Command::new(program)
        .arg(&url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not open the browser ({program}): {e}"))?;
    // Reap the launcher once it exits instead of leaving a zombie behind
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets(names: &[&str]) -> Vec<GhAsset> {
        names
            .iter()
            .map(|n| GhAsset {
                name: (*n).to_string(),
                size: 1,
                browser_download_url: format!("https://example.invalid/{n}"),
                digest: None,
            })
            .collect()
    }

    const ALL: &[&str] = &[
        "LiteTerm_1.9.0_universal.dmg",
        "LiteTerm_1.9.0_amd64.deb",
        "LiteTerm_1.9.0_amd64.AppImage",
        "LiteTerm-1.9.0-1.x86_64.rpm",
        "LiteTerm_1.9.0_x64-setup.exe",
    ];

    fn pick(install: Install, os: &str, names: &[&str]) -> Option<String> {
        pick_asset_for(install, os, &assets(names)).map(|a| a.name)
    }

    #[test]
    fn each_install_gets_its_own_asset_from_a_shared_release() {
        assert_eq!(pick(Install::Windows, "windows", ALL).as_deref(), Some("LiteTerm_1.9.0_x64-setup.exe"));
        assert_eq!(pick(Install::MacApp, "macos", ALL).as_deref(), Some("LiteTerm_1.9.0_universal.dmg"));
        assert_eq!(pick(Install::AppImage, "linux", ALL).as_deref(), Some("LiteTerm_1.9.0_amd64.AppImage"));
        // A package install updates from its own kind of package
        assert_eq!(pick(Install::Deb, "linux", ALL).as_deref(), Some("LiteTerm_1.9.0_amd64.deb"));
        assert_eq!(pick(Install::Rpm, "linux", ALL).as_deref(), Some("LiteTerm-1.9.0-1.x86_64.rpm"));
    }

    #[test]
    fn an_unknown_linux_install_still_hears_about_the_release() {
        let no_appimage = &ALL[..2];
        assert_eq!(pick(Install::Other, "linux", no_appimage).as_deref(), Some("LiteTerm_1.9.0_amd64.deb"));
        assert_eq!(pick(Install::Other, "linux", &["LiteTerm-1.9.0-1.x86_64.rpm"]).as_deref(), Some("LiteTerm-1.9.0-1.x86_64.rpm"));
        assert_eq!(pick(Install::Other, "linux", ALL).as_deref(), Some("LiteTerm_1.9.0_amd64.AppImage"));
    }

    #[test]
    fn a_release_without_a_build_for_this_install_offers_nothing() {
        // The macOS build failed (or is still running): Mac users see no update
        let windows_only = &["LiteTerm_1.9.0_x64-setup.exe"];
        assert_eq!(pick(Install::MacApp, "macos", windows_only), None);
        assert_eq!(pick(Install::AppImage, "linux", windows_only), None);
        assert_eq!(pick(Install::Deb, "linux", &["LiteTerm_1.9.0_amd64.AppImage"]), None);
        assert_eq!(pick(Install::Windows, "windows", &["LiteTerm_1.9.0_universal.dmg"]), None);
        assert_eq!(pick(Install::Other, "freebsd", ALL), None);
    }

    #[test]
    fn windows_still_prefers_the_x64_setup() {
        assert_eq!(
            pick(Install::Windows, "windows", &["a-setup.exe", "LiteTerm_x64-setup.exe", "b.exe"]).as_deref(),
            Some("LiteTerm_x64-setup.exe")
        );
        assert_eq!(pick(Install::Windows, "windows", &["a-setup.exe", "b.exe"]).as_deref(), Some("a-setup.exe"));
        assert_eq!(pick(Install::Windows, "windows", &["b.exe"]).as_deref(), Some("b.exe"));
    }

    #[test]
    fn the_published_digest_travels_with_the_asset() {
        let hex = "601B6772623DE3289DF1A57C4914B445F6E3FA5499D43B647D7D312090D28568";
        let mut a = assets(&["LiteTerm_1.11.0_universal.dmg"]);
        a[0].digest = Some(format!("sha256:{hex}"));
        let picked = pick_asset_for(Install::MacApp, "macos", &a).unwrap();
        assert_eq!(picked.sha256.as_deref(), Some(hex.to_ascii_lowercase().as_str()));
        assert_eq!(sha256_of_digest("md5:abc"), None);
        assert_eq!(sha256_of_digest("sha256:xyz"), None);
        assert_eq!(hex_lower(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn the_app_bundle_is_found_from_its_binary() {
        assert_eq!(
            mac_bundle_of(Path::new("/Applications/LiteTerm.app/Contents/MacOS/liteterm")),
            Some(PathBuf::from("/Applications/LiteTerm.app"))
        );
        assert_eq!(mac_bundle_of(Path::new("/usr/local/bin/liteterm")), None);
        assert_eq!(mac_bundle_of(Path::new("/x/Contents/MacOS/liteterm")), None);
    }

    #[test]
    fn an_appimage_counts_only_when_this_binary_runs_from_it() {
        let appdir = Path::new("/tmp/.mount_LiteTeAbc");
        assert!(runs_from_appimage(Path::new("/tmp/.mount_LiteTeAbc/usr/bin/liteterm"), appdir));
        // Started from another AppImage's terminal: its APPDIR, our binary elsewhere
        assert!(!runs_from_appimage(Path::new("/usr/bin/liteterm"), appdir));
        assert!(!runs_from_appimage(Path::new("/tmp/.mount_LiteTeAbcd/usr/bin/liteterm"), appdir));
        assert!(!runs_from_appimage(Path::new("/usr/bin/liteterm"), Path::new("")));
    }

    #[cfg(unix)]
    #[test]
    fn the_update_folder_is_made_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("liteterm-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        prepare_update_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        let link = dir.with_extension("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(prepare_update_dir(&link).is_err(), "a link is refused");
        let _ = std::fs::remove_file(&link);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_new_appimage_starts_without_the_old_ones_mount() {
        let vars = [
            ("APPIMAGE", "/home/me/LiteTerm.AppImage"),
            ("APPDIR", "/tmp/.mount_LiteTeAbc"),
            ("OWD", "/home/me"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_LiteTeAbc/usr/lib:/opt/mine/lib"),
            ("GTK_PATH", "/tmp/.mount_LiteTeAbc/usr/lib/gtk-3.0"),
            ("HOME", "/home/me"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let out = appimage_env_cleanup(vars.into_iter(), "/tmp/.mount_LiteTeAbc");
        let get = |k: &str| out.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone());
        assert_eq!(get("APPIMAGE"), Some(None));
        assert_eq!(get("APPDIR"), Some(None));
        assert_eq!(get("LD_LIBRARY_PATH"), Some(Some("/opt/mine/lib".into())));
        assert_eq!(get("GTK_PATH"), Some(None));
        assert_eq!(get("HOME"), None, "unrelated variables are left alone");
    }

    #[test]
    fn packages_go_through_the_front_end_when_there_is_one() {
        let file = Path::new("/tmp/liteterm-update/LiteTerm_1.12.0_amd64.deb");
        let only = |tools: &'static [&'static str]| {
            move |name: &str| tools.contains(&name).then(|| PathBuf::from(format!("/usr/bin/{name}")))
        };
        let as_strings = |v: Option<Vec<std::ffi::OsString>>| {
            v.map(|v| v.into_iter().map(|s| s.to_string_lossy().into_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            as_strings(package_command(Install::Deb, file, only(&["apt-get", "dpkg"]))),
            Some(vec!["/usr/bin/apt-get".into(), "install".into(), "-y".into(), file.to_string_lossy().into_owned()])
        );
        assert_eq!(
            as_strings(package_command(Install::Deb, file, only(&["dpkg"]))).unwrap()[..2],
            ["/usr/bin/dpkg".to_string(), "-i".to_string()]
        );
        let rpm = Path::new("/tmp/liteterm-update/LiteTerm-1.12.0-1.x86_64.rpm");
        assert_eq!(as_strings(package_command(Install::Rpm, rpm, only(&["dnf", "rpm"]))).unwrap()[1], "install");
        assert_eq!(as_strings(package_command(Install::Rpm, rpm, only(&["zypper"]))).unwrap()[3], "--allow-unsigned-rpm");
        assert_eq!(as_strings(package_command(Install::Rpm, rpm, only(&["rpm"]))).unwrap()[1], "-U");
        assert_eq!(package_command(Install::Rpm, rpm, only(&[])), None);
    }

    #[test]
    fn download_names_stay_inside_the_update_folder() {
        assert_eq!(sanitize_name("LiteTerm_1.12.0_amd64.AppImage"), "LiteTerm_1.12.0_amd64.AppImage");
        assert_eq!(sanitize_name("../../etc/passwd"), "liteterm-update....etcpasswd");
        assert_eq!(sanitize_name("///"), "liteterm-update");
    }

    #[test]
    fn only_this_repos_release_pages_may_be_opened() {
        assert!(is_release_url("https://github.com/adittanu/liteterm/releases/tag/v1.9.0"));
        assert!(!is_release_url("https://github.com/adittanu/liteterm-fork/releases"));
        assert!(!is_release_url("https://github.com/someone/else/releases"));
        assert!(!is_release_url("https://evil.example/https://github.com/adittanu/liteterm/"));
        assert!(!is_release_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_release_url("-oProxyCommand=x"));
        assert!(!is_release_url("https://github.com/adittanu/liteterm/ x"));
        assert!(!is_release_url(""));
    }
}
